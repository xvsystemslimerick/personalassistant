//! Native credential and trust boundary for the Raspberry Pi kiosk.
//!
//! The browser never reads this configuration. A later launcher gate will use
//! it to proxy authenticated TLS on the Pi loopback interface.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use display_api::{validate_snapshot, DisplaySnapshot};
use rand::Rng;
use reqwest::{header, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use thiserror::Error;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const MAX_CONFIG_BYTES: usize = 4096;
const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024;
const DISPLAY_ID_HEADER: &str = "x-personal-assistant-display-id";

pub struct PairingBootstrap {
    pub host_url: String,
    pub certificate_sha256: String,
    pub certificate_der_base64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingRequest<'a> {
    code: &'a str,
    display_name: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PairingResponse {
    display_id: String,
    token: String,
}

pub struct PairingBundle {
    pub display_id: String,
    pub host_url: String,
    pub token: Zeroizing<String>,
    pub certificate_sha256: String,
    pub certificate_der_base64: String,
}

impl std::fmt::Debug for PairingBundle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingBundle")
            .field("display_id", &self.display_id)
            .field("host_url", &self.host_url)
            .field("token", &"[REDACTED]")
            .field("certificate_sha256", &"[REDACTED]")
            .field("certificate_der_base64", &"[REDACTED]")
            .finish()
    }
}

pub struct DisplayClientConfig {
    pub display_id: String,
    pub host_url: Url,
    pub token: Zeroizing<String>,
    pub certificate_sha256: [u8; 32],
    pub certificate_der: Vec<u8>,
}

impl std::fmt::Debug for DisplayClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DisplayClientConfig")
            .field("display_id", &self.display_id)
            .field("host_url", &self.host_url)
            .field("token", &"[REDACTED]")
            .field("certificate_sha256", &"[REDACTED]")
            .field("certificate_der", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StoredConfig {
    schema_version: u16,
    display_id: String,
    host_url: String,
    token: String,
    certificate_sha256: String,
    certificate_der_base64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfigRef<'a> {
    schema_version: u16,
    display_id: &'a str,
    host_url: &'a str,
    token: &'a str,
    certificate_sha256: String,
    certificate_der_base64: &'a str,
}

#[derive(Debug, Error)]
pub enum LauncherConfigError {
    #[error("invalid family display configuration")]
    Invalid,
    #[error("family display configuration is unavailable")]
    Unavailable,
    #[error("family display configuration storage is unsafe")]
    UnsafeStorage,
    #[error("family display configuration could not be stored")]
    Storage,
}

pub fn install_pairing_bundle(
    config_path: &Path,
    bundle: PairingBundle,
) -> Result<(), LauncherConfigError> {
    let validated = validate_bundle(bundle)?;
    let parent = config_path
        .parent()
        .ok_or(LauncherConfigError::UnsafeStorage)?;
    ensure_private_directory(parent)?;
    reject_symlink(config_path)?;

    let host_url = validated.host_url.to_string();
    let certificate_der_base64 = STANDARD.encode(&validated.certificate_der);
    let stored = StoredConfigRef {
        schema_version: 1,
        display_id: &validated.display_id,
        host_url: &host_url,
        token: validated.token.as_str(),
        certificate_sha256: encode_hex(&validated.certificate_sha256),
        certificate_der_base64: &certificate_der_base64,
    };
    let mut encoded =
        Zeroizing::new(serde_json::to_vec(&stored).map_err(|_| LauncherConfigError::Invalid)?);
    if encoded.len() > MAX_CONFIG_BYTES {
        return Err(LauncherConfigError::Invalid);
    }

    let suffix: u64 = rand::rng().random();
    let temp_path = parent.join(format!(".family-display-config-{suffix:016x}.tmp"));
    let write_result = write_private_file(&temp_path, &encoded).and_then(|_| {
        fs::rename(&temp_path, config_path).map_err(|_| LauncherConfigError::Storage)
    });
    encoded.zeroize();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

pub fn load_config(config_path: &Path) -> Result<DisplayClientConfig, LauncherConfigError> {
    reject_symlink(config_path)?;
    ensure_private_file(config_path)?;
    let mut encoded =
        Zeroizing::new(fs::read(config_path).map_err(|_| LauncherConfigError::Unavailable)?);
    if encoded.is_empty() || encoded.len() > MAX_CONFIG_BYTES {
        return Err(LauncherConfigError::Invalid);
    }
    let mut stored: StoredConfig =
        serde_json::from_slice(&encoded).map_err(|_| LauncherConfigError::Invalid)?;
    encoded.zeroize();
    let bundle = PairingBundle {
        display_id: stored.display_id,
        host_url: stored.host_url,
        token: Zeroizing::new(std::mem::take(&mut stored.token)),
        certificate_sha256: stored.certificate_sha256,
        certificate_der_base64: stored.certificate_der_base64,
    };
    validate_bundle(bundle)
}

fn validate_bundle(bundle: PairingBundle) -> Result<DisplayClientConfig, LauncherConfigError> {
    if !valid_identifier(&bundle.display_id)
        || bundle.token.len() < 43
        || bundle.token.len() > 128
        || !bundle
            .token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(LauncherConfigError::Invalid);
    }
    let host_url = Url::parse(&bundle.host_url).map_err(|_| LauncherConfigError::Invalid)?;
    if host_url.scheme() != "https"
        || host_url.username() != ""
        || host_url.password().is_some()
        || host_url.query().is_some()
        || host_url.fragment().is_some()
        || host_url.path() != "/"
        || host_url.port_or_known_default() != Some(8765)
        || !allowed_local_host(&host_url)
    {
        return Err(LauncherConfigError::Invalid);
    }
    let certificate_sha256 = decode_sha256(&bundle.certificate_sha256)?;
    let certificate_der = STANDARD
        .decode(bundle.certificate_der_base64.as_bytes())
        .map_err(|_| LauncherConfigError::Invalid)?;
    if certificate_der.is_empty()
        || certificate_der.len() > MAX_CERTIFICATE_BYTES
        || <[u8; 32]>::from(Sha256::digest(&certificate_der)) != certificate_sha256
    {
        return Err(LauncherConfigError::Invalid);
    }
    Ok(DisplayClientConfig {
        display_id: bundle.display_id,
        host_url,
        token: bundle.token,
        certificate_sha256,
        certificate_der,
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotFetchError {
    #[error("family display host connection failed")]
    Connection,
    #[error("family display host rejected the request")]
    Rejected,
    #[error("family display host returned an invalid snapshot")]
    InvalidResponse,
}

/// Fetches the sole read-only endpoint over certificate-pinned HTTPS.
pub async fn fetch_snapshot(
    config: &DisplayClientConfig,
) -> Result<DisplaySnapshot, SnapshotFetchError> {
    let endpoint = config
        .host_url
        .join("api/v1/snapshot")
        .map_err(|_| SnapshotFetchError::InvalidResponse)?;
    let certificate = reqwest::Certificate::from_der(&config.certificate_der)
        .map_err(|_| SnapshotFetchError::InvalidResponse)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .tls_built_in_root_certs(false)
        .add_root_certificate(certificate)
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| SnapshotFetchError::Connection)?;
    let mut response = client
        .get(endpoint)
        .bearer_auth(config.token.as_str())
        .header(DISPLAY_ID_HEADER, &config.display_id)
        .header(header::ACCEPT, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .send()
        .await
        .map_err(|_| SnapshotFetchError::Connection)?;
    if !response.status().is_success() {
        return Err(SnapshotFetchError::Rejected);
    }
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value.to_ascii_lowercase().starts_with("application/json;")
        });
    if !is_json
        || response
            .content_length()
            .is_some_and(|size| size > MAX_SNAPSHOT_BYTES as u64)
    {
        return Err(SnapshotFetchError::InvalidResponse);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| SnapshotFetchError::Connection)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotFetchError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    let snapshot: DisplaySnapshot =
        serde_json::from_slice(&body).map_err(|_| SnapshotFetchError::InvalidResponse)?;
    validate_snapshot(&snapshot).map_err(|_| SnapshotFetchError::InvalidResponse)?;
    Ok(snapshot)
}

pub async fn pair_display(
    bootstrap: PairingBootstrap,
    code: &str,
    display_name: &str,
) -> Result<PairingBundle, SnapshotFetchError> {
    if code.len() != 6
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || display_name.trim().is_empty()
        || display_name.trim().chars().count() > 80
        || display_name.chars().any(char::is_control)
    {
        return Err(SnapshotFetchError::InvalidResponse);
    }
    let host_url =
        Url::parse(&bootstrap.host_url).map_err(|_| SnapshotFetchError::InvalidResponse)?;
    if host_url.scheme() != "https"
        || !host_url.username().is_empty()
        || host_url.password().is_some()
        || host_url.query().is_some()
        || host_url.fragment().is_some()
        || host_url.path() != "/"
        || host_url.port_or_known_default() != Some(8765)
        || !allowed_local_host(&host_url)
    {
        return Err(SnapshotFetchError::InvalidResponse);
    }
    let expected = decode_sha256(&bootstrap.certificate_sha256)
        .map_err(|_| SnapshotFetchError::InvalidResponse)?;
    let certificate_der = STANDARD
        .decode(bootstrap.certificate_der_base64.as_bytes())
        .map_err(|_| SnapshotFetchError::InvalidResponse)?;
    if certificate_der.is_empty()
        || certificate_der.len() > MAX_CERTIFICATE_BYTES
        || <[u8; 32]>::from(Sha256::digest(&certificate_der)) != expected
    {
        return Err(SnapshotFetchError::InvalidResponse);
    }
    let certificate = reqwest::Certificate::from_der(&certificate_der)
        .map_err(|_| SnapshotFetchError::InvalidResponse)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .tls_built_in_root_certs(false)
        .add_root_certificate(certificate)
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| SnapshotFetchError::Connection)?;
    let mut response = client
        .post(
            host_url
                .join("api/v1/pair")
                .map_err(|_| SnapshotFetchError::InvalidResponse)?,
        )
        .header(header::ACCEPT, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .json(&PairingRequest {
            code,
            display_name: display_name.trim(),
        })
        .send()
        .await
        .map_err(|_| SnapshotFetchError::Connection)?;
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value.to_ascii_lowercase().starts_with("application/json;")
        });
    if !response.status().is_success()
        || !is_json
        || response
            .content_length()
            .is_some_and(|length| length > 1024)
    {
        return Err(SnapshotFetchError::Rejected);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| SnapshotFetchError::Connection)?
    {
        if body.len().saturating_add(chunk.len()) > 1024 {
            return Err(SnapshotFetchError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    let mut paired: PairingResponse =
        serde_json::from_slice(&body).map_err(|_| SnapshotFetchError::InvalidResponse)?;
    let token = Zeroizing::new(std::mem::take(&mut paired.token));
    let bundle = PairingBundle {
        display_id: paired.display_id,
        host_url: host_url.to_string(),
        token,
        certificate_sha256: bootstrap.certificate_sha256,
        certificate_der_base64: bootstrap.certificate_der_base64,
    };
    if !valid_identifier(&bundle.display_id)
        || !(43..=128).contains(&bundle.token.len())
        || !bundle
            .token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SnapshotFetchError::InvalidResponse);
    }
    Ok(bundle)
}

fn allowed_local_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_private() || address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_unique_local() || address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("personal-assistant.local"),
        None => false,
    }
}

fn valid_identifier(value: &str) -> bool {
    (16..=100).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn decode_sha256(value: &str) -> Result<[u8; 32], LauncherConfigError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LauncherConfigError::Invalid);
    }
    let mut digest = [0_u8; 32];
    for (index, target) in digest.iter_mut().enumerate() {
        *target = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| LauncherConfigError::Invalid)?;
    }
    Ok(digest)
}

fn encode_hex(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reject_symlink(path: &Path) -> Result<(), LauncherConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(LauncherConfigError::UnsafeStorage)
        }
        Ok(_) | Err(_) => Ok(()),
    }
}

#[cfg(unix)]
fn ensure_private_directory(path: &Path) -> Result<(), LauncherConfigError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !path.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| LauncherConfigError::Storage)?;
    }
    let metadata = fs::metadata(path).map_err(|_| LauncherConfigError::UnsafeStorage)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(LauncherConfigError::UnsafeStorage);
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_directory(path: &Path) -> Result<(), LauncherConfigError> {
    fs::create_dir_all(path).map_err(|_| LauncherConfigError::Storage)
}

#[cfg(unix)]
fn write_private_file(path: &Path, value: &[u8]) -> Result<(), LauncherConfigError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| LauncherConfigError::Storage)?;
    file.write_all(value)
        .and_then(|_| file.sync_all())
        .map_err(|_| LauncherConfigError::Storage)
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, value: &[u8]) -> Result<(), LauncherConfigError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| LauncherConfigError::Storage)?;
    file.write_all(value)
        .and_then(|_| file.sync_all())
        .map_err(|_| LauncherConfigError::Storage)
}

fn ensure_private_file(path: &Path) -> Result<(), LauncherConfigError> {
    let metadata = fs::metadata(path).map_err(|_| LauncherConfigError::Unavailable)?;
    if !metadata.is_file() {
        return Err(LauncherConfigError::UnsafeStorage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(LauncherConfigError::UnsafeStorage);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> PairingBundle {
        let certificate = b"test certificate placeholder";
        PairingBundle {
            display_id: "display-abcdefghijklmnop".into(),
            host_url: "https://192.168.1.20:8765/".into(),
            token: Zeroizing::new("A".repeat(43)),
            certificate_sha256: encode_hex(&Sha256::digest(certificate).into()),
            certificate_der_base64: STANDARD.encode(certificate),
        }
    }

    #[test]
    fn configuration_is_atomic_private_and_redacted() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("config.json");
        install_pairing_bundle(&path, bundle()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.display_id, "display-abcdefghijklmnop");
        assert_eq!(loaded.host_url.as_str(), "https://192.168.1.20:8765/");
        assert!(!format!("{loaded:?}").contains(&"A".repeat(43)));
        assert!(!format!("{:?}", bundle()).contains(&"A".repeat(43)));
        assert_eq!(loaded.certificate_der, b"test certificate placeholder");

        let mut rotated = bundle();
        rotated.token = Zeroizing::new("B".repeat(43));
        install_pairing_bundle(&path, rotated).unwrap();
        assert_eq!(load_config(&path).unwrap().token.as_str(), "B".repeat(43));
        assert!(directory.path().read_dir().unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
    }

    #[test]
    fn rejects_public_hosts_credentials_queries_and_unsafe_storage() {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = directory.path().join("config.json");
        for url in [
            "http://192.168.1.20:8765/",
            "https://8.8.8.8:8765/",
            "https://user@192.168.1.20:8765/",
            "https://192.168.1.20:8765/?token=secret",
            "https://personal-assistant.local:443/",
        ] {
            let mut invalid = bundle();
            invalid.host_url = url.into();
            assert!(matches!(
                install_pairing_bundle(&path, invalid),
                Err(LauncherConfigError::Invalid)
            ));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let unsafe_directory = directory.path().join("unsafe");
            fs::create_dir(&unsafe_directory).unwrap();
            fs::set_permissions(&unsafe_directory, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(
                install_pairing_bundle(&unsafe_directory.join("config.json"), bundle()),
                Err(LauncherConfigError::UnsafeStorage)
            ));
        }
    }

    #[test]
    fn rejects_certificate_material_that_does_not_match_the_pin() {
        let mut invalid = bundle();
        invalid.certificate_sha256 = "12".repeat(32);
        assert!(matches!(
            validate_bundle(invalid),
            Err(LauncherConfigError::Invalid)
        ));
    }

    #[test]
    fn pairing_bootstrap_rejects_untrusted_material_before_network_access() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(pair_display(
            PairingBootstrap {
                host_url: "https://8.8.8.8:8765/".into(),
                certificate_sha256: "12".repeat(32),
                certificate_der_base64: STANDARD.encode(b"not a certificate"),
            },
            "123456",
            "Kitchen",
        ));
        assert_eq!(result.unwrap_err(), SnapshotFetchError::InvalidResponse);
    }

    #[test]
    fn pinned_https_fetch_accepts_only_the_bounded_snapshot_contract() {
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::{
            rustls::{
                pki_types::{CertificateDer, PrivatePkcs8KeyDer},
                ServerConfig,
            },
            TlsAcceptor,
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
            let CertifiedKey { cert, signing_key } =
                generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
            let certificate_der = cert.der().to_vec();
            let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
            let server_config = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(certificate_der.clone())],
                    private_key.into(),
                )
                .unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut stream = TlsAcceptor::from(Arc::new(server_config))
                    .accept(socket)
                    .await
                    .unwrap();
                let mut request = [0_u8; 4096];
                let read = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                assert!(request.starts_with("GET /api/v1/snapshot HTTP/1.1\r\n"));
                assert!(request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
                assert!(request.to_ascii_lowercase().contains(
                    "x-personal-assistant-display-id: display-abcdefghijklmnop"
                ));
                let body = r#"{"schemaVersion":1,"generatedAt":"2026-09-02T12:00:00+01:00","mode":"today","items":[]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            });
            let config = DisplayClientConfig {
                display_id: "display-abcdefghijklmnop".into(),
                host_url: Url::parse(&format!("https://127.0.0.1:{address_port}/", address_port = address.port())).unwrap(),
                token: Zeroizing::new("A".repeat(43)),
                certificate_sha256: Sha256::digest(&certificate_der).into(),
                certificate_der,
            };
            let snapshot = fetch_snapshot(&config).await.unwrap();
            assert_eq!(snapshot.schema_version, 1);
            assert!(snapshot.items.is_empty());
            server.await.unwrap();
        });
    }
}
