use database::Database;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIVE_DATABASE: &str = "personal-assistant.db";
const MARKER: &str = ".restore-pending.json";
const MAX_MARKER_BYTES: u64 = 1024;

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("restore state is invalid")]
    Invalid,
    #[error("restore file operation failed")]
    Io(#[from] std::io::Error),
    #[error("restore database validation failed")]
    Database(#[from] database::DatabaseError),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingRestore {
    version: u8,
    id: String,
    staged_sha256: String,
    rollback_sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    None,
    Applied { rollback_path: PathBuf },
    Rejected,
}

pub fn stage(
    data_directory: &Path,
    current: &Database,
    restored_database: &[u8],
    id: &str,
) -> Result<(), RestoreError> {
    validate_id(id)?;
    let marker = data_directory.join(MARKER);
    let staged = staged_path(data_directory, id);
    let rollback = rollback_path(data_directory, id);
    if marker.exists() || staged.exists() || rollback.exists() {
        return Err(RestoreError::Invalid);
    }
    let result = (|| {
        write_new_private(&staged, restored_database)?;
        validate_snapshot_clean(&staged)?;
        current.create_consistent_backup_snapshot(&rollback)?;
        validate_snapshot_clean(&rollback)?;
        let pending = PendingRestore {
            version: 1,
            id: id.to_owned(),
            staged_sha256: digest_file(&staged)?,
            rollback_sha256: digest_file(&rollback)?,
        };
        let encoded = serde_json::to_vec(&pending).map_err(|_| RestoreError::Invalid)?;
        write_new_private(&marker, &encoded)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
        let _ = fs::remove_file(&rollback);
        let _ = fs::remove_file(&marker);
    }
    result
}

pub fn apply_pending(data_directory: &Path) -> Result<ApplyOutcome, RestoreError> {
    let marker = data_directory.join(MARKER);
    if !marker.exists() {
        return Ok(ApplyOutcome::None);
    }
    let pending: PendingRestore = match read_pending(&marker) {
        Ok(value) => value,
        Err(_) => {
            // An untrusted marker must never prevent a valid live database from opening.
            if data_directory.join(LIVE_DATABASE).is_file() {
                fs::remove_file(marker)?;
                sync_directory(data_directory)?;
                return Ok(ApplyOutcome::Rejected);
            }
            return Err(RestoreError::Invalid);
        }
    };
    let staged = staged_path(data_directory, &pending.id);
    let rollback = rollback_path(data_directory, &pending.id);
    let live = data_directory.join(LIVE_DATABASE);
    let replaced = data_directory.join(format!(".restore-{}-replaced.db", pending.id));

    let rollback_valid = validates_as(&rollback, &pending.rollback_sha256);
    let staged_valid = validates_as(&staged, &pending.staged_sha256);
    let live_is_restored = validates_as(&live, &pending.staged_sha256);

    // A previous launch completed the swap and stopped before deleting its marker.
    if live_is_restored && !staged.exists() && replaced.is_file() && rollback_valid {
        return finalize(data_directory, &marker, &replaced, rollback);
    }

    // A previous launch stopped after moving the original database aside.
    if !live.exists() && replaced.is_file() {
        if staged_valid && rollback_valid {
            if let Err(error) = fs::rename(&staged, &live) {
                restore_replaced(&replaced, &live)?;
                reject_known(data_directory, &marker, &staged)?;
                return Err(error.into());
            }
            sync_directory(data_directory)?;
            if validates_as(&live, &pending.staged_sha256) {
                return finalize(data_directory, &marker, &replaced, rollback);
            }
            let _ = fs::remove_file(&live);
        }
        restore_replaced(&replaced, &live)?;
        reject_known(data_directory, &marker, &staged)?;
        return Ok(ApplyOutcome::Rejected);
    }

    // Normal prepared state. Validate every artifact before touching the live file.
    if live.is_file() && !replaced.exists() && staged_valid && rollback_valid {
        fs::rename(&live, &replaced)?;
        sync_directory(data_directory)?;
        remove_sqlite_sidecars(data_directory);
        if let Err(error) = fs::rename(&staged, &live) {
            restore_replaced(&replaced, &live)?;
            reject_known(data_directory, &marker, &staged)?;
            return Err(error.into());
        }
        sync_directory(data_directory)?;
        if validates_as(&live, &pending.staged_sha256) {
            return finalize(data_directory, &marker, &replaced, rollback);
        }
        let _ = fs::remove_file(&live);
        restore_replaced(&replaced, &live)?;
        reject_known(data_directory, &marker, &staged)?;
        return Ok(ApplyOutcome::Rejected);
    }

    // Ambiguous or tampered state: preserve a valid live database and rollback.
    if live.is_file() {
        reject_known(data_directory, &marker, &staged)?;
        return Ok(ApplyOutcome::Rejected);
    }
    Err(RestoreError::Invalid)
}

fn read_pending(marker: &Path) -> Result<PendingRestore, RestoreError> {
    let pending: PendingRestore =
        serde_json::from_slice(&read_bounded_regular(marker, MAX_MARKER_BYTES)?)
            .map_err(|_| RestoreError::Invalid)?;
    if pending.version != 1 {
        return Err(RestoreError::Invalid);
    }
    validate_id(&pending.id)?;
    if !is_sha256(&pending.staged_sha256) || !is_sha256(&pending.rollback_sha256) {
        return Err(RestoreError::Invalid);
    }
    Ok(pending)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validates_as(path: &Path, expected_digest: &str) -> bool {
    path.is_file()
        && digest_file(path).is_ok_and(|digest| digest == expected_digest)
        && validate_snapshot_clean(path).is_ok()
}

fn validate_snapshot_clean(path: &Path) -> Result<(), RestoreError> {
    let result = Database::validate_backup_snapshot(path).map_err(Into::into);
    remove_database_sidecars(path);
    result
}

fn finalize(
    data_directory: &Path,
    marker: &Path,
    replaced: &Path,
    rollback_path: PathBuf,
) -> Result<ApplyOutcome, RestoreError> {
    fs::remove_file(marker)?;
    fs::remove_file(replaced)?;
    sync_directory(data_directory)?;
    Ok(ApplyOutcome::Applied { rollback_path })
}

fn restore_replaced(replaced: &Path, live: &Path) -> Result<(), RestoreError> {
    if live.exists() {
        fs::remove_file(live)?;
    }
    fs::rename(replaced, live)?;
    if validate_snapshot_clean(live).is_err() {
        return Err(RestoreError::Invalid);
    }
    if let Some(directory) = live.parent() {
        sync_directory(directory)?;
    }
    Ok(())
}

fn reject_known(data_directory: &Path, marker: &Path, staged: &Path) -> Result<(), RestoreError> {
    if marker.exists() {
        fs::remove_file(marker)?;
    }
    if staged.exists() {
        fs::remove_file(staged)?;
    }
    remove_database_sidecars(staged);
    sync_directory(data_directory)
}

fn remove_sqlite_sidecars(data_directory: &Path) {
    let _ = fs::remove_file(data_directory.join(format!("{LIVE_DATABASE}-wal")));
    let _ = fs::remove_file(data_directory.join(format!("{LIVE_DATABASE}-shm")));
}

fn remove_database_sidecars(database_path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = database_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let _ = fs::remove_file(PathBuf::from(sidecar));
    }
}

fn validate_id(id: &str) -> Result<(), RestoreError> {
    if id.len() == 16
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(RestoreError::Invalid)
    }
}

fn staged_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!(".restore-{id}-staged.db"))
}
fn rollback_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!("Personal Assistant rollback {id}.db"))
}

fn digest_file(path: &Path) -> Result<String, RestoreError> {
    if path.symlink_metadata()?.file_type().is_symlink() {
        return Err(RestoreError::Invalid);
    }
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 513 * 1024 * 1024 {
        return Err(RestoreError::Invalid);
    }
    let mut digest = Sha256::new();
    let copied = std::io::copy(
        &mut std::io::Read::by_ref(&mut file).take(513 * 1024 * 1024 + 1),
        &mut digest,
    )?;
    if copied > 513 * 1024 * 1024 {
        return Err(RestoreError::Invalid);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn read_bounded_regular(path: &Path, max: u64) -> Result<Vec<u8>, RestoreError> {
    if path.symlink_metadata()?.file_type().is_symlink() {
        return Err(RestoreError::Invalid);
    }
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max {
        return Err(RestoreError::Invalid);
    }
    let mut value = Vec::with_capacity(metadata.len() as usize);
    file.take(max + 1).read_to_end(&mut value)?;
    if value.len() as u64 > max {
        return Err(RestoreError::Invalid);
    }
    Ok(value)
}

fn write_new_private(path: &Path, value: &[u8]) -> Result<(), RestoreError> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(value)?;
    file.sync_all()?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), RestoreError> {
    fs::File::open(path)?.sync_all().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_core::Settings;

    #[test]
    fn staged_restore_swaps_only_at_startup_and_keeps_validated_rollback() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join(LIVE_DATABASE);
        let current = Database::open(&live).unwrap();
        let old = current.settings().unwrap();
        let source = Database::open_in_memory().unwrap();
        source
            .save_settings(Settings {
                household_name: "Restored household".into(),
                ..source.settings().unwrap()
            })
            .unwrap();
        let source_path = directory.path().join("source.db");
        source
            .create_consistent_backup_snapshot(&source_path)
            .unwrap();
        let bytes = fs::read(&source_path).unwrap();
        stage(directory.path(), &current, &bytes, "0123456789abcdef").unwrap();
        assert_eq!(current.settings().unwrap(), old);
        drop(current);
        let applied = apply_pending(directory.path()).unwrap();
        let restored = Database::open(&live).unwrap();
        assert_eq!(
            restored.settings().unwrap().household_name,
            "Restored household"
        );
        let ApplyOutcome::Applied { rollback_path } = applied else {
            panic!("not applied")
        };
        Database::validate_backup_snapshot(rollback_path).unwrap();
        assert!(!directory.path().join(MARKER).exists());
        assert!(!directory
            .path()
            .join(".restore-0123456789abcdef-staged.db-wal")
            .exists());
        assert!(!directory
            .path()
            .join(".restore-0123456789abcdef-staged.db-shm")
            .exists());
    }

    #[test]
    fn tampered_staging_never_replaces_live_database() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join(LIVE_DATABASE);
        let current = Database::open(&live).unwrap();
        let source = Database::open_in_memory().unwrap();
        let source_path = directory.path().join("source.db");
        source
            .create_consistent_backup_snapshot(&source_path)
            .unwrap();
        stage(
            directory.path(),
            &current,
            &fs::read(source_path).unwrap(),
            "fedcba9876543210",
        )
        .unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(staged_path(directory.path(), "fedcba9876543210"))
            .unwrap()
            .write_all(b"tamper")
            .unwrap();
        drop(current);
        assert_eq!(
            apply_pending(directory.path()).unwrap(),
            ApplyOutcome::Rejected
        );
        assert!(Database::open(&live).is_ok());
    }

    #[test]
    fn resumes_after_original_database_was_moved_aside() {
        let fixture = fixture("Restored household", "0011223344556677");
        drop(fixture.current);
        fs::rename(&fixture.live, &fixture.replaced).unwrap();

        assert!(matches!(
            apply_pending(fixture.directory.path()).unwrap(),
            ApplyOutcome::Applied { .. }
        ));
        assert_eq!(
            Database::open(&fixture.live)
                .unwrap()
                .settings()
                .unwrap()
                .household_name,
            "Restored household"
        );
    }

    #[test]
    fn finalizes_after_staged_database_was_moved_into_place() {
        let fixture = fixture("Restored household", "1122334455667788");
        drop(fixture.current);
        fs::rename(&fixture.live, &fixture.replaced).unwrap();
        fs::rename(
            staged_path(fixture.directory.path(), fixture.id),
            &fixture.live,
        )
        .unwrap();

        assert!(matches!(
            apply_pending(fixture.directory.path()).unwrap(),
            ApplyOutcome::Applied { .. }
        ));
        assert_eq!(
            Database::open(&fixture.live)
                .unwrap()
                .settings()
                .unwrap()
                .household_name,
            "Restored household"
        );
    }

    #[test]
    fn restores_original_if_staging_is_corrupt_after_original_move() {
        let fixture = fixture("Restored household", "2233445566778899");
        let original = fixture.current.settings().unwrap();
        drop(fixture.current);
        fs::rename(&fixture.live, &fixture.replaced).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(staged_path(fixture.directory.path(), fixture.id))
            .unwrap()
            .write_all(b"tamper")
            .unwrap();

        assert_eq!(
            apply_pending(fixture.directory.path()).unwrap(),
            ApplyOutcome::Rejected
        );
        assert_eq!(
            Database::open(&fixture.live).unwrap().settings().unwrap(),
            original
        );
    }

    #[test]
    fn malformed_marker_does_not_block_valid_live_database() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join(LIVE_DATABASE);
        drop(Database::open(&live).unwrap());
        write_new_private(&directory.path().join(MARKER), b"not json").unwrap();

        assert_eq!(
            apply_pending(directory.path()).unwrap(),
            ApplyOutcome::Rejected
        );
        assert!(Database::open(live).is_ok());
    }

    struct Fixture {
        directory: tempfile::TempDir,
        live: PathBuf,
        replaced: PathBuf,
        current: Database,
        id: &'static str,
    }

    fn fixture(household_name: &str, id: &'static str) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join(LIVE_DATABASE);
        let current = Database::open(&live).unwrap();
        let source = Database::open_in_memory().unwrap();
        source
            .save_settings(Settings {
                household_name: household_name.into(),
                ..source.settings().unwrap()
            })
            .unwrap();
        let source_path = directory.path().join("source.db");
        source
            .create_consistent_backup_snapshot(&source_path)
            .unwrap();
        stage(
            directory.path(),
            &current,
            &fs::read(source_path).unwrap(),
            id,
        )
        .unwrap();
        let replaced = directory.path().join(format!(".restore-{id}-replaced.db"));
        Fixture {
            directory,
            live,
            replaced,
            current,
            id,
        }
    }
}
