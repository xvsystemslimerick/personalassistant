//! Fail-closed request boundary for the Family Display host service.
//!
//! TLS socket ownership and desktop opt-in are separate activation gates. This
//! module deliberately has one method, one path, and no mutation surface.

use database::Database;
use display_api::{DisplayMode, DisplaySnapshot, PairingChallenge, PairingSession};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    rustls::{pki_types::CertificateDer, pki_types::PrivateKeyDer, ServerConfig},
    TlsAcceptor,
};
use zeroize::{Zeroize, Zeroizing};

pub const SNAPSHOT_PATH: &str = "/api/v1/snapshot";
pub const PAIRING_PATH: &str = "/api/v1/pair";
pub const DISPLAY_ID_HEADER: &str = "x-personal-assistant-display-id";
pub const MAX_REQUEST_BODY_BYTES: usize = 0;
const MAX_HTTP_HEADER_BYTES: usize = 8 * 1024;
const MAX_PAIRING_BODY_BYTES: usize = 256;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const RATE_WINDOW: Duration = Duration::from_secs(60);
const MAX_REQUESTS_PER_ADDRESS: usize = 60;
const MAX_REQUESTS_GLOBAL: usize = 600;
const MAX_TRACKED_ADDRESSES: usize = 256;
const MAX_CONCURRENT_CONNECTIONS: usize = 16;

#[derive(Default)]
pub struct PairingCoordinator {
    session: Mutex<PairingSession>,
}

pub struct CompletedPairing {
    pub display_id: String,
    pub token: Zeroizing<String>,
}

impl std::fmt::Debug for CompletedPairing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompletedPairing")
            .field("display_id", &self.display_id)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl PairingCoordinator {
    pub fn begin(&self, now_unix: i64) -> Result<PairingChallenge, HostRequestError> {
        self.session
            .lock()
            .map(|mut session| session.begin(now_unix))
            .map_err(|_| HostRequestError::Unavailable)
    }

    pub fn complete(
        &self,
        database: &Database,
        submitted_code: &str,
        display_name: &str,
        now_unix: i64,
    ) -> Result<CompletedPairing, HostRequestError> {
        let display_name = display_name.trim();
        if display_name.is_empty()
            || display_name.chars().count() > 80
            || display_name.chars().any(char::is_control)
        {
            return Err(HostRequestError::Rejected);
        }
        let credential = self
            .session
            .lock()
            .map_err(|_| HostRequestError::Unavailable)?
            .confirm(submitted_code, now_unix)
            .map_err(|_| HostRequestError::Rejected)?;
        database
            .register_family_display(
                &credential.display_id,
                display_name,
                &credential.token_sha256,
                &display_api::DisplayPolicy::default(),
                &format!("paired-event-{}", credential.display_id),
            )
            .map_err(|_| HostRequestError::Rejected)?;
        Ok(CompletedPairing {
            display_id: credential.display_id,
            token: credential.token,
        })
    }
}

pub struct SnapshotRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub display_id: Option<&'a str>,
    pub authorization: Option<&'a str>,
    pub body_bytes: usize,
    pub mode: DisplayMode,
}

impl std::fmt::Debug for SnapshotRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("display_id", &"[REDACTED]")
            .field("authorization", &"[REDACTED]")
            .field("body_bytes", &self.body_bytes)
            .field("mode", &self.mode)
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostRequestError {
    #[error("family display request was rejected")]
    Rejected,
    #[error("family display service is unavailable")]
    Unavailable,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ListenerError {
    #[error("family display service configuration is invalid")]
    InvalidConfiguration,
    #[error("family display service could not start")]
    StartFailed,
}

pub struct ListenerConfig {
    pub enabled: bool,
    pub bind_address: SocketAddr,
    pub certificate_der: Vec<u8>,
    pub private_key_der: Zeroizing<Vec<u8>>,
}

pub struct TlsIdentity {
    pub certificate_der: Vec<u8>,
    pub certificate_sha256: [u8; 32],
    pub private_key_der: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for TlsIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TlsIdentity")
            .field("certificate_der", &"[REDACTED]")
            .field("certificate_sha256", &"[REDACTED]")
            .field("private_key_der", &"[REDACTED]")
            .finish()
    }
}

pub fn generate_tls_identity(bind_address: SocketAddr) -> Result<TlsIdentity, ListenerError> {
    if !allowed_bind_address(bind_address) {
        return Err(ListenerError::InvalidConfiguration);
    }
    let names = vec![
        bind_address.ip().to_string(),
        "personal-assistant.local".to_owned(),
    ];
    let rcgen::CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(names)
        .map_err(|_| ListenerError::InvalidConfiguration)?;
    let certificate_der = cert.der().to_vec();
    let certificate_sha256 = Sha256::digest(&certificate_der).into();
    Ok(TlsIdentity {
        certificate_der,
        certificate_sha256,
        private_key_der: Zeroizing::new(signing_key.serialize_der()),
    })
}

impl std::fmt::Debug for ListenerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ListenerConfig")
            .field("enabled", &self.enabled)
            .field("bind_address", &self.bind_address)
            .field("certificate_der", &"[REDACTED]")
            .field("private_key_der", &"[REDACTED]")
            .finish()
    }
}

pub struct ListenerHandle {
    local_address: SocketAddr,
    stopping: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl ListenerHandle {
    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub async fn shutdown(self) {
        self.stopping.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.task).await;
    }
}

pub async fn start_listener(
    database: Arc<Database>,
    config: ListenerConfig,
) -> Result<ListenerHandle, ListenerError> {
    start_listener_inner(
        database,
        config,
        Arc::new(PairingCoordinator::default()),
        false,
    )
    .await
}

pub async fn start_listener_with_pairing(
    database: Arc<Database>,
    config: ListenerConfig,
    pairing: Arc<PairingCoordinator>,
) -> Result<ListenerHandle, ListenerError> {
    start_listener_inner(database, config, pairing, false).await
}

async fn start_listener_inner(
    database: Arc<Database>,
    mut config: ListenerConfig,
    pairing: Arc<PairingCoordinator>,
    allow_ephemeral_loopback: bool,
) -> Result<ListenerHandle, ListenerError> {
    let ephemeral_loopback = allow_ephemeral_loopback
        && config.bind_address.port() == 0
        && config.bind_address.ip().is_loopback();
    if !config.enabled || (!allowed_bind_address(config.bind_address) && !ephemeral_loopback) {
        return Err(ListenerError::InvalidConfiguration);
    }
    let private_key = PrivateKeyDer::try_from(config.private_key_der.to_vec())
        .map_err(|_| ListenerError::InvalidConfiguration)?;
    config.private_key_der.zeroize();
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(config.certificate_der)],
            private_key,
        )
        .map_err(|_| ListenerError::InvalidConfiguration)?;
    let listener = tokio::net::TcpListener::bind(config.bind_address)
        .await
        .map_err(|_| ListenerError::StartFailed)?;
    let local_address = listener
        .local_addr()
        .map_err(|_| ListenerError::StartFailed)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let task_stopping = stopping.clone();
    let tls = TlsAcceptor::from(Arc::new(server_config));
    let limiter = Arc::new(Mutex::new(RateLimiter::default()));
    let concurrency = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let task = tokio::spawn(async move {
        let mut connections = Vec::new();
        while !task_stopping.load(Ordering::Acquire) {
            connections.retain(|task: &tokio::task::JoinHandle<()>| !task.is_finished());
            let accepted =
                tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
            let (socket, peer) = match accepted {
                Ok(Ok(value)) => value,
                Ok(Err(_)) => break,
                Err(_) => continue,
            };
            let allowed = limiter
                .lock()
                .map(|mut value| value.allow(peer.ip(), Instant::now()))
                .unwrap_or(false);
            let Ok(permit) = concurrency.clone().try_acquire_owned() else {
                continue;
            };
            if !allowed {
                continue;
            }
            let tls = tls.clone();
            let database = database.clone();
            let pairing = pairing.clone();
            connections.push(tokio::spawn(async move {
                let _permit = permit;
                let operation = async {
                    let mut stream = tls.accept(socket).await.map_err(|_| ())?;
                    handle_connection(&database, &pairing, &mut stream).await
                };
                let _ = tokio::time::timeout(CONNECTION_TIMEOUT, operation).await;
            }));
        }
        for connection in &connections {
            connection.abort();
        }
        for connection in connections {
            let _ = connection.await;
        }
    });
    Ok(ListenerHandle {
        local_address,
        stopping,
        task,
    })
}

fn allowed_bind_address(address: SocketAddr) -> bool {
    address.port() == 8765
        && match address.ip() {
            IpAddr::V4(value) => value.is_private() || value.is_loopback(),
            IpAddr::V6(value) => value.is_unique_local() || value.is_loopback(),
        }
}

#[derive(Default)]
struct RateLimiter {
    global: VecDeque<Instant>,
    addresses: HashMap<IpAddr, VecDeque<Instant>>,
}

impl RateLimiter {
    fn allow(&mut self, address: IpAddr, now: Instant) -> bool {
        prune(&mut self.global, now);
        self.addresses.retain(|_, entries| {
            prune(entries, now);
            !entries.is_empty()
        });
        if self.global.len() >= MAX_REQUESTS_GLOBAL
            || (!self.addresses.contains_key(&address)
                && self.addresses.len() >= MAX_TRACKED_ADDRESSES)
        {
            return false;
        }
        let entries = self.addresses.entry(address).or_default();
        if entries.len() >= MAX_REQUESTS_PER_ADDRESS {
            return false;
        }
        self.global.push_back(now);
        entries.push_back(now);
        true
    }
}

fn prune(entries: &mut VecDeque<Instant>, now: Instant) {
    while entries
        .front()
        .is_some_and(|entry| now.saturating_duration_since(*entry) >= RATE_WINDOW)
    {
        entries.pop_front();
    }
}

async fn handle_connection<S>(
    database: &Database,
    pairing: &PairingCoordinator,
    stream: &mut S,
) -> Result<(), ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut input = vec![0_u8; MAX_HTTP_HEADER_BYTES];
    let mut used = 0;
    loop {
        if used == input.len() {
            return write_error(stream).await;
        }
        let read = stream.read(&mut input[used..]).await.map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        used += read;
        if input[..used].windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&input[..used]).map_err(|_| ())?;
    let Some((head, trailing)) = request.split_once("\r\n\r\n") else {
        return write_error(stream).await;
    };
    let mut lines = head.split("\r\n");
    let Some(request_line) = lines.next() else {
        return write_error(stream).await;
    };
    let parts = request_line.split(' ').collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return write_error(stream).await;
    }
    let mut display_id = None;
    let mut authorization = None;
    let mut content_length = None;
    let mut content_type = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return write_error(stream).await;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case(DISPLAY_ID_HEADER) {
            if display_id.replace(value).is_some() {
                return write_error(stream).await;
            }
        } else if name.eq_ignore_ascii_case("authorization") {
            if authorization.replace(value).is_some() {
                return write_error(stream).await;
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return write_error(stream).await;
            }
            content_length = Some(value.parse::<usize>().map_err(|_| ())?);
        } else if name.eq_ignore_ascii_case("content-type") {
            if content_type.replace(value).is_some() {
                return write_error(stream).await;
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return write_error(stream).await;
        }
    }
    if parts[0] == "POST" && parts[1] == PAIRING_PATH {
        return handle_pairing_request(
            database,
            pairing,
            stream,
            content_length,
            content_type,
            trailing.as_bytes(),
        )
        .await;
    }
    let result = process_snapshot_request(
        database,
        SnapshotRequest {
            method: parts[0],
            path: parts[1],
            display_id,
            authorization,
            body_bytes: content_length.unwrap_or(0).max(trailing.len()),
            mode: DisplayMode::Today,
        },
        &chrono::Local::now().to_rfc3339(),
    );
    let snapshot = match result {
        Ok(value) => value,
        Err(_) => return write_error(stream).await,
    };
    let body = serde_json::to_vec(&snapshot).map_err(|_| ())?;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(|_| ())?;
    stream.write_all(&body).await.map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PairingRequestBody {
    code: String,
    display_name: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingResponseBody<'a> {
    display_id: &'a str,
    token: &'a str,
}

async fn handle_pairing_request<S>(
    database: &Database,
    pairing: &PairingCoordinator,
    stream: &mut S,
    content_length: Option<usize>,
    content_type: Option<&str>,
    initial_body: &[u8],
) -> Result<(), ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let Some(length) = content_length else {
        return write_error(stream).await;
    };
    if length == 0
        || length > MAX_PAIRING_BODY_BYTES
        || initial_body.len() > length
        || !content_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json"))
    {
        return write_error(stream).await;
    }
    let mut body = Zeroizing::new(Vec::with_capacity(length));
    body.extend_from_slice(initial_body);
    while body.len() < length {
        let remaining = length - body.len();
        let mut chunk = [0_u8; 256];
        let capacity = remaining.min(chunk.len());
        let read = stream.read(&mut chunk[..capacity]).await.map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let request: PairingRequestBody = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return write_error(stream).await,
    };
    let completed = match pairing.complete(
        database,
        &request.code,
        &request.display_name,
        chrono::Utc::now().timestamp(),
    ) {
        Ok(value) => value,
        Err(_) => return write_error(stream).await,
    };
    let response = PairingResponseBody {
        display_id: &completed.display_id,
        token: completed.token.as_str(),
    };
    let response_body = Zeroizing::new(serde_json::to_vec(&response).map_err(|_| ())?);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(|_| ())?;
    stream.write_all(&response_body).await.map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}

async fn write_error<S>(stream: &mut S) -> Result<(), ()>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    stream
        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}

pub fn process_snapshot_request(
    database: &Database,
    request: SnapshotRequest<'_>,
    now_rfc3339: &str,
) -> Result<DisplaySnapshot, HostRequestError> {
    if request.method != "GET"
        || request.path != SNAPSHOT_PATH
        || request.body_bytes != MAX_REQUEST_BODY_BYTES
        || chrono::DateTime::parse_from_rfc3339(now_rfc3339).is_err()
    {
        return Err(HostRequestError::Rejected);
    }
    let display_id = request.display_id.ok_or(HostRequestError::Rejected)?;
    let authorization = request.authorization.ok_or(HostRequestError::Rejected)?;
    let token = authorization
        .strip_prefix("Bearer ")
        .filter(|value| {
            (43..=128).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or(HostRequestError::Rejected)?;
    let mut digest = Zeroizing::new(<[u8; 32]>::from(Sha256::digest(token.as_bytes())));
    let authenticated = database
        .authenticate_family_display(display_id, &digest, now_rfc3339)
        .map_err(|_| HostRequestError::Unavailable)?;
    digest.zeroize();
    if !authenticated {
        return Err(HostRequestError::Rejected);
    }
    database
        .family_display_snapshot(display_id, now_rfc3339, request.mode)
        .map_err(|_| HostRequestError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use display_api::DisplayPolicy;

    fn database_with_display(token: &str) -> Database {
        let database = Database::open_in_memory().unwrap();
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        database
            .register_family_display(
                "display-abcdefghijklmnop",
                "Kitchen",
                &digest,
                &DisplayPolicy::default(),
                "event-abcdefghijklmnop",
            )
            .unwrap();
        database
    }

    fn request<'a>(token: &'a str) -> SnapshotRequest<'a> {
        SnapshotRequest {
            method: "GET",
            path: SNAPSHOT_PATH,
            display_id: Some("display-abcdefghijklmnop"),
            authorization: Some(token),
            body_bytes: 0,
            mode: DisplayMode::Today,
        }
    }

    #[test]
    fn authenticated_get_returns_only_the_projected_contract() {
        let token = "A".repeat(43);
        let database = database_with_display(&token);
        let authorization = format!("Bearer {token}");
        let snapshot = process_snapshot_request(
            &database,
            request(&authorization),
            "2026-09-02T12:00:00+01:00",
        )
        .unwrap();
        assert_eq!(snapshot.schema_version, 1);
        assert!(!format!("{:?}", request(&authorization)).contains(&token));
    }

    #[test]
    fn rejects_wrong_credentials_mutations_and_request_bodies() {
        let database = database_with_display(&"A".repeat(43));
        let wrong = format!("Bearer {}", "B".repeat(43));
        assert_eq!(
            process_snapshot_request(&database, request(&wrong), "2026-09-02T12:00:00+01:00"),
            Err(HostRequestError::Rejected)
        );
        let valid = format!("Bearer {}", "A".repeat(43));
        for (method, path, body_bytes) in [
            ("POST", SNAPSHOT_PATH, 0),
            ("GET", "/api/v1/action", 0),
            ("GET", SNAPSHOT_PATH, 1),
        ] {
            let mut invalid = request(&valid);
            invalid.method = method;
            invalid.path = path;
            invalid.body_bytes = body_bytes;
            assert_eq!(
                process_snapshot_request(&database, invalid, "2026-09-02T12:00:00+01:00"),
                Err(HostRequestError::Rejected)
            );
        }
    }

    #[test]
    fn limiter_is_per_address_bounded_and_recovers_after_the_window() {
        let now = Instant::now();
        let address = "192.168.1.20".parse().unwrap();
        let mut limiter = RateLimiter::default();
        for _ in 0..MAX_REQUESTS_PER_ADDRESS {
            assert!(limiter.allow(address, now));
        }
        assert!(!limiter.allow(address, now));
        assert!(limiter.allow(address, now + RATE_WINDOW));
    }

    #[test]
    fn listener_requires_explicit_opt_in_private_address_and_fixed_port() {
        let database = Arc::new(Database::open_in_memory().unwrap());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for (enabled, address) in [
            (false, "127.0.0.1:8765"),
            (true, "0.0.0.0:8765"),
            (true, "8.8.8.8:8765"),
            (true, "127.0.0.1:443"),
        ] {
            let result = runtime.block_on(start_listener(
                database.clone(),
                ListenerConfig {
                    enabled,
                    bind_address: address.parse().unwrap(),
                    certificate_der: vec![],
                    private_key_der: Zeroizing::new(vec![]),
                },
            ));
            assert!(matches!(result, Err(ListenerError::InvalidConfiguration)));
        }
    }

    #[test]
    fn generated_identity_is_bound_to_a_safe_host_and_redacted() {
        let identity = generate_tls_identity("192.168.1.20:8765".parse().unwrap()).unwrap();
        assert!(!identity.certificate_der.is_empty());
        assert!(!identity.private_key_der.is_empty());
        assert_eq!(
            identity.certificate_sha256,
            <[u8; 32]>::from(Sha256::digest(&identity.certificate_der))
        );
        let debug = format!("{identity:?}");
        assert!(!debug.contains(&format!("{:x}", Sha256::digest(&identity.certificate_der))));
        assert!(matches!(
            generate_tls_identity("0.0.0.0:8765".parse().unwrap()),
            Err(ListenerError::InvalidConfiguration)
        ));
    }

    #[test]
    fn real_tls_listener_interoperates_with_the_pinned_launcher() {
        use family_display_launcher::{fetch_snapshot, DisplayClientConfig};
        use rcgen::{generate_simple_self_signed, CertifiedKey};
        use url::Url;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
            let database = Arc::new(Database::open_in_memory().unwrap());
            let pairing = Arc::new(PairingCoordinator::default());
            let challenge = pairing.begin(chrono::Utc::now().timestamp()).unwrap();
            let CertifiedKey { cert, signing_key } =
                generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
            let certificate_der = cert.der().to_vec();
            let listener = start_listener_inner(
                database,
                ListenerConfig {
                    enabled: true,
                    bind_address: "127.0.0.1:0".parse().unwrap(),
                    certificate_der: certificate_der.clone(),
                    private_key_der: Zeroizing::new(signing_key.serialize_der()),
                },
                pairing,
                true,
            )
            .await
            .unwrap();
            let port = listener.local_address().port();
            assert_ne!(port, 0);
            let host_url = format!("https://127.0.0.1:{port}/");
            let certificate = reqwest::Certificate::from_der(&certificate_der).unwrap();
            let client = reqwest::Client::builder()
                .https_only(true)
                .tls_built_in_root_certs(false)
                .add_root_certificate(certificate)
                .build()
                .unwrap();
            let paired: serde_json::Value = client
                .post(format!("{host_url}api/v1/pair"))
                .json(&serde_json::json!({ "code": challenge.code, "displayName": "Kitchen" }))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            let config = DisplayClientConfig {
                display_id: paired["displayId"].as_str().unwrap().to_owned(),
                host_url: Url::parse(&host_url).unwrap(),
                token: Zeroizing::new(paired["token"].as_str().unwrap().to_owned()),
                certificate_sha256: Sha256::digest(&certificate_der).into(),
                certificate_der,
            };
            let snapshot = fetch_snapshot(&config).await.unwrap();
            assert_eq!(snapshot.schema_version, 1);
            listener.shutdown().await;
        });
    }

    #[test]
    fn pairing_is_single_use_registered_and_secret_redacted() {
        let database = Database::open_in_memory().unwrap();
        let coordinator = PairingCoordinator::default();
        let challenge = coordinator.begin(1_000).unwrap();
        let completed = coordinator
            .complete(&database, &challenge.code, "Kitchen", 1_100)
            .unwrap();
        assert_eq!(database.family_displays().unwrap().len(), 1);
        assert!(!format!("{completed:?}").contains(completed.token.as_str()));
        assert!(matches!(
            coordinator.complete(&database, &challenge.code, "Second", 1_101),
            Err(HostRequestError::Rejected)
        ));
    }

    #[test]
    fn invalid_pairing_name_does_not_consume_the_challenge() {
        let database = Database::open_in_memory().unwrap();
        let coordinator = PairingCoordinator::default();
        let challenge = coordinator.begin(2_000).unwrap();
        assert!(matches!(
            coordinator.complete(&database, &challenge.code, "\n", 2_001),
            Err(HostRequestError::Rejected)
        ));
        assert!(coordinator
            .complete(&database, &challenge.code, "Hall", 2_002)
            .is_ok());
    }
}
