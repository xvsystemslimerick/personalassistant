use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"PABACK01";
const HEADER_BYTES: usize = 8 + 2 + 16 + 24 + 8;
const MAX_DATABASE_BYTES: usize = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const TAG_BYTES: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BackupError {
    #[error("invalid backup input")]
    InvalidInput,
    #[error("backup authentication failed")]
    AuthenticationFailed,
    #[error("backup format is unsupported")]
    UnsupportedFormat,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format_version: u16,
    created_at: String,
    database_bytes: u64,
    database_sha256: String,
    includes_raw_email: bool,
}

pub fn create_encrypted_backup(
    database: &[u8],
    password: &mut [u8],
    created_at: &str,
) -> Result<Vec<u8>, BackupError> {
    let result = create_inner(database, password, created_at);
    password.zeroize();
    result
}

fn create_inner(
    database: &[u8],
    password: &[u8],
    created_at: &str,
) -> Result<Vec<u8>, BackupError> {
    validate_password(password)?;
    if database.len() < 16
        || database.len() > MAX_DATABASE_BYTES
        || !database.starts_with(b"SQLite format 3\0")
    {
        return Err(BackupError::InvalidInput);
    }
    chrono::DateTime::parse_from_rfc3339(created_at).map_err(|_| BackupError::InvalidInput)?;
    let digest = Sha256::digest(database);
    let manifest = Manifest {
        format_version: 1,
        created_at: created_at.to_owned(),
        database_bytes: database.len() as u64,
        database_sha256: hex(&digest),
        includes_raw_email: false,
    };
    let manifest = serde_json::to_vec(&manifest).map_err(|_| BackupError::InvalidInput)?;
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(BackupError::InvalidInput);
    }
    let mut plaintext = Zeroizing::new(Vec::with_capacity(4 + manifest.len() + database.len()));
    plaintext.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
    plaintext.extend_from_slice(&manifest);
    plaintext.extend_from_slice(database);

    let mut salt = [0_u8; 16];
    let mut nonce = [0_u8; 24];
    rand::rng().fill_bytes(&mut salt);
    rand::rng().fill_bytes(&mut nonce);
    let key = derive_key(password, &salt)?;
    let mut header = Vec::with_capacity(HEADER_BYTES);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&1_u16.to_be_bytes());
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    header.extend_from_slice(&((plaintext.len() + TAG_BYTES) as u64).to_be_bytes());
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key[..]).map_err(|_| BackupError::InvalidInput)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .map_err(|_| BackupError::InvalidInput)?;
    let mut output = header;
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

pub fn decrypt_backup(container: &[u8], password: &mut [u8]) -> Result<Vec<u8>, BackupError> {
    let result = decrypt_inner(container, password);
    password.zeroize();
    result
}

fn decrypt_inner(container: &[u8], password: &[u8]) -> Result<Vec<u8>, BackupError> {
    validate_password(password)?;
    if container.len() < HEADER_BYTES + TAG_BYTES || &container[..8] != MAGIC {
        return Err(BackupError::UnsupportedFormat);
    }
    let version = u16::from_be_bytes(
        container[8..10]
            .try_into()
            .map_err(|_| BackupError::UnsupportedFormat)?,
    );
    if version != 1 {
        return Err(BackupError::UnsupportedFormat);
    }
    let declared = usize::try_from(u64::from_be_bytes(
        container[50..58]
            .try_into()
            .map_err(|_| BackupError::UnsupportedFormat)?,
    ))
    .map_err(|_| BackupError::InvalidInput)?;
    if declared != container.len() - HEADER_BYTES
        || declared > MAX_DATABASE_BYTES + MAX_MANIFEST_BYTES + TAG_BYTES + 4
    {
        return Err(BackupError::InvalidInput);
    }
    let key = derive_key(password, &container[10..26])?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key[..]).map_err(|_| BackupError::InvalidInput)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&container[26..50]),
                Payload {
                    msg: &container[HEADER_BYTES..],
                    aad: &container[..HEADER_BYTES],
                },
            )
            .map_err(|_| BackupError::AuthenticationFailed)?,
    );
    if plaintext.len() < 4 {
        return Err(BackupError::InvalidInput);
    }
    let manifest_len = u32::from_be_bytes(
        plaintext[..4]
            .try_into()
            .map_err(|_| BackupError::InvalidInput)?,
    ) as usize;
    if manifest_len == 0 || manifest_len > MAX_MANIFEST_BYTES || 4 + manifest_len > plaintext.len()
    {
        return Err(BackupError::InvalidInput);
    }
    let manifest: Manifest = serde_json::from_slice(&plaintext[4..4 + manifest_len])
        .map_err(|_| BackupError::InvalidInput)?;
    if manifest.format_version != 1
        || manifest.includes_raw_email
        || chrono::DateTime::parse_from_rfc3339(&manifest.created_at).is_err()
    {
        return Err(BackupError::InvalidInput);
    }
    let database = &plaintext[4 + manifest_len..];
    if usize::try_from(manifest.database_bytes).ok() != Some(database.len())
        || database.len() > MAX_DATABASE_BYTES
        || !database.starts_with(b"SQLite format 3\0")
        || hex(&Sha256::digest(database)) != manifest.database_sha256
    {
        return Err(BackupError::InvalidInput);
    }
    Ok(database.to_vec())
}

fn validate_password(password: &[u8]) -> Result<(), BackupError> {
    if !(12..=1024).contains(&password.len()) {
        return Err(BackupError::InvalidInput);
    }
    Ok(())
}

fn derive_key(password: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|_| BackupError::InvalidInput)?;
    let mut key = Zeroizing::new([0_u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|_| BackupError::InvalidInput)?;
    Ok(key)
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Vec<u8> {
        let mut value = b"SQLite format 3\0".to_vec();
        value.extend_from_slice(&[7_u8; 4096]);
        value
    }

    #[test]
    fn round_trip_uses_randomized_authenticated_encryption_and_zeroizes_passwords() {
        let data = database();
        let mut first_password = b"correct horse battery staple".to_vec();
        let first =
            create_encrypted_backup(&data, &mut first_password, "2026-09-06T12:00:00+01:00")
                .unwrap();
        assert!(first_password.iter().all(|byte| *byte == 0));
        let mut second_password = b"correct horse battery staple".to_vec();
        let second =
            create_encrypted_backup(&data, &mut second_password, "2026-09-06T12:00:00+01:00")
                .unwrap();
        assert_ne!(first, second);
        assert!(!first
            .windows(16)
            .any(|window| window == b"SQLite format 3\0"));
        let mut restore_password = b"correct horse battery staple".to_vec();
        assert_eq!(decrypt_backup(&first, &mut restore_password).unwrap(), data);
        assert!(restore_password.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn wrong_password_tampering_truncation_and_unsupported_versions_fail_closed() {
        let mut password = b"correct horse battery staple".to_vec();
        let backup =
            create_encrypted_backup(&database(), &mut password, "2026-09-06T12:00:00Z").unwrap();
        let mut wrong = b"this password is incorrect".to_vec();
        assert_eq!(
            decrypt_backup(&backup, &mut wrong),
            Err(BackupError::AuthenticationFailed)
        );
        let mut tampered = backup.clone();
        *tampered.last_mut().unwrap() ^= 1;
        let mut password = b"correct horse battery staple".to_vec();
        assert_eq!(
            decrypt_backup(&tampered, &mut password),
            Err(BackupError::AuthenticationFailed)
        );
        let mut password = b"correct horse battery staple".to_vec();
        assert!(decrypt_backup(&backup[..backup.len() - 1], &mut password).is_err());
        let mut unsupported = backup;
        unsupported[9] = 2;
        let mut password = b"correct horse battery staple".to_vec();
        assert_eq!(
            decrypt_backup(&unsupported, &mut password),
            Err(BackupError::UnsupportedFormat)
        );
    }

    #[test]
    fn rejects_weak_passwords_non_sqlite_data_and_invalid_timestamps() {
        let mut weak = b"too-short".to_vec();
        assert_eq!(
            create_encrypted_backup(&database(), &mut weak, "2026-09-06T12:00:00Z"),
            Err(BackupError::InvalidInput)
        );
        assert!(weak.iter().all(|byte| *byte == 0));
        let mut password = b"correct horse battery staple".to_vec();
        assert_eq!(
            create_encrypted_backup(b"not a database!!!", &mut password, "2026-09-06T12:00:00Z"),
            Err(BackupError::InvalidInput)
        );
        let mut password = b"correct horse battery staple".to_vec();
        assert_eq!(
            create_encrypted_backup(&database(), &mut password, "tomorrow"),
            Err(BackupError::InvalidInput)
        );
    }
}
