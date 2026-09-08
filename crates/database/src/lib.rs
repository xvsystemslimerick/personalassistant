use assistant_core::{AutomationPolicy, Settings, Theme};
use email::graph::{CalendarEvent, MessageMetadata};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};
use thiserror::Error;

const CURRENT_SCHEMA_VERSION: i64 = 27;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedAccount {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub email_address: String,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FamilyDisplayRecord {
    pub id: String,
    pub display_name: String,
    pub revoked: bool,
    pub last_seen_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FamilyDisplayServiceConfig {
    pub enabled: bool,
    pub bind_address: Option<String>,
    pub certificate_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRunSummary {
    pub outcome: String,
    pub finished_at: Option<String>,
    pub inbox_count: i64,
    pub sent_count: i64,
    pub calendar_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMessage {
    pub provider_id: String,
    pub subject: Option<String>,
    pub occurred_at: Option<String>,
    pub analyzed: bool,
    pub has_reply_draft: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewSuggestion {
    Task {
        title: String,
        due_at: Option<String>,
    },
    Appointment {
        title: String,
        start_at: Option<String>,
        end_at: Option<String>,
        confirmed: bool,
    },
    WaitingFor {
        description: String,
        expected_from: Option<String>,
        follow_up_at: Option<String>,
    },
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub account_id: String,
    pub provider_id: String,
    pub subject: Option<String>,
    pub sender_name: Option<String>,
    pub occurred_at: Option<String>,
    pub summary: String,
    pub classification: String,
    pub classification_confidence: f64,
    pub urgency: String,
    pub suggestions: Vec<ReviewSuggestion>,
    pub review_reasons: Vec<String>,
    pub analyzed_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecision {
    pub account_id: String,
    pub provider_id: String,
    pub subject: Option<String>,
    pub decision: String,
    pub decided_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalItem {
    pub id: i64,
    pub account_id: String,
    pub provider_id: String,
    pub suggestion_index: i64,
    pub kind: String,
    pub title: String,
    pub due_at: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub expected_from: Option<String>,
    pub follow_up_at: Option<String>,
    pub category: String,
    pub status: String,
    pub lifecycle_state: String,
    pub scheduled_at: Option<String>,
    pub created_at: String,
    pub undone_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalItemEvent {
    pub id: i64,
    pub item_id: i64,
    pub event_type: String,
    pub previous_state: String,
    pub new_state: String,
    pub previous_scheduled_at: Option<String>,
    pub new_scheduled_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalItemProvenance {
    pub item_id: i64,
    pub provider: String,
    pub source_subject: Option<String>,
    pub source_sender: Option<String>,
    pub source_occurred_at: Option<String>,
    pub source_available: bool,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InertActionProposal {
    pub id: i64,
    pub proposal_key: String,
    pub audit_event_key: String,
    pub action_kind: String,
    pub sensitivity: String,
    pub policy: String,
    pub disposition: String,
    pub reason_code: String,
    pub display_label: String,
    pub target_ref: Option<String>,
    pub confidence: f64,
    pub expires_at: String,
    pub created_at: String,
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarExecutionPreparation {
    pub execution_key: String,
    pub transaction_id: String,
    pub proposal_key: String,
    pub account_id: String,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarUpdateCandidate {
    pub account_id: String,
    pub provider_id: String,
    pub subject: String,
    pub start_at: String,
    pub start_timezone: String,
    pub end_at: String,
    pub end_timezone: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarUpdatePreparation {
    pub execution_key: String,
    pub proposal_key: String,
    pub account_id: String,
    pub provider_event_id: String,
    pub provider_etag: String,
    pub previous_start_at: String,
    pub previous_end_at: String,
    pub proposed_start_at: String,
    pub proposed_end_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplyDraftMetadata {
    pub draft_key: String,
    pub revision_key: String,
    pub account_id: String,
    pub provider_message_id: String,
    pub content_sha256: String,
    pub content_bytes: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceExecutionPreparation {
    pub execution_key: String,
    pub proposal_key: String,
    pub account_id: String,
    pub provider_message_id: String,
    pub revision_key: String,
    pub content_sha256: String,
    pub content_bytes: i64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceExecutionHistory {
    pub display_label: String,
    pub outcome: String,
    pub reason_code: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationAuditEntry {
    pub policy: String,
    pub decision: String,
    pub action_kind: String,
    pub reason_code: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct QueueActionProposal<'a> {
    pub proposal_key: &'a str,
    pub audit_event_key: &'a str,
    pub display_label: &'a str,
    pub target_ref: Option<&'a str>,
    pub expires_at: &'a str,
    pub policy: AutomationPolicy,
    pub context: rules::AutomationContext,
}

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        Self::configure(&connection)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let connection = Connection::open_in_memory()?;
        Self::configure(&connection)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn create_consistent_backup_snapshot(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<(), DatabaseError> {
        let destination = destination.as_ref();
        if destination.exists() || destination.parent().is_none() {
            return Err(DatabaseError::InvalidBackupDestination);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.backup(rusqlite::DatabaseName::Main, destination, None)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Validates a decrypted backup without migrating or otherwise modifying it.
    pub fn validate_backup_snapshot(path: impl AsRef<Path>) -> Result<(), DatabaseError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let integrity: String =
            connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(DatabaseError::InvalidBackupDestination);
        }
        let version: i64 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        let settings_table: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'settings'",
            [],
            |row| row.get(0),
        )?;
        if version != CURRENT_SCHEMA_VERSION || settings_table != 1 {
            return Err(DatabaseError::InvalidBackupDestination);
        }
        Ok(())
    }

    fn configure(connection: &Connection) -> Result<(), rusqlite::Error> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        Ok(())
    }

    fn migrate(connection: &Connection) -> Result<(), rusqlite::Error> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )?;
        Self::validate_migration_ledger(connection, false)?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS settings (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               household_name TEXT NOT NULL CHECK(length(household_name) BETWEEN 1 AND 80),
               theme TEXT NOT NULL CHECK(theme IN ('system', 'light', 'dark')),
               launch_at_login INTEGER NOT NULL CHECK(launch_at_login IN (0, 1)),
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             INSERT OR IGNORE INTO settings(id, household_name, theme, launch_at_login)
               VALUES (1, 'My household', 'system', 0);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
             CREATE TABLE IF NOT EXISTS connected_accounts (
               id TEXT PRIMARY KEY,
               provider TEXT NOT NULL CHECK(provider IN ('microsoft')),
               display_name TEXT NOT NULL CHECK(length(display_name) BETWEEN 1 AND 200),
               email_address TEXT NOT NULL CHECK(length(email_address) BETWEEN 3 AND 320),
               tenant_id TEXT,
               enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS sync_cursors (
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               resource TEXT NOT NULL CHECK(resource IN ('mail_inbox', 'mail_sent', 'calendar')),
               cursor_url TEXT,
               last_success_at TEXT,
               last_error_code TEXT,
               PRIMARY KEY(account_id, resource)
             );
             CREATE TABLE IF NOT EXISTS message_metadata (
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               provider_id TEXT NOT NULL,
               conversation_id TEXT,
               sender_name TEXT,
               sender_address TEXT,
               subject TEXT,
               received_at TEXT,
               sent_at TEXT,
               provider_web_link TEXT,
               is_read INTEGER NOT NULL DEFAULT 0 CHECK(is_read IN (0, 1)),
               deleted_at TEXT,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY(account_id, provider_id)
             );
             CREATE INDEX IF NOT EXISTS idx_message_metadata_received ON message_metadata(account_id, received_at DESC);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
             CREATE TABLE IF NOT EXISTS calendar_events (
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               provider_id TEXT NOT NULL,
               subject TEXT,
               start_at TEXT NOT NULL,
               start_timezone TEXT NOT NULL,
               end_at TEXT NOT NULL,
               end_timezone TEXT NOT NULL,
               provider_web_link TEXT,
               is_cancelled INTEGER NOT NULL DEFAULT 0 CHECK(is_cancelled IN (0, 1)),
               last_modified_at TEXT,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY(account_id, provider_id)
             );
             CREATE INDEX IF NOT EXISTS idx_calendar_events_start ON calendar_events(account_id, start_at);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
             CREATE TABLE IF NOT EXISTS sync_runs (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               finished_at TEXT,
               outcome TEXT CHECK(outcome IN ('running','success','cancelled','error')),
               inbox_count INTEGER NOT NULL DEFAULT 0,
               sent_count INTEGER NOT NULL DEFAULT 0,
               calendar_count INTEGER NOT NULL DEFAULT 0,
               error_code TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_sync_runs_account ON sync_runs(account_id, started_at DESC);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (4);
             COMMIT;"
        )?;
        let has_privacy_setting: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('settings') WHERE name='store_complete_email_content')",
            [], |row| row.get(0)
        )?;
        if !has_privacy_setting {
            connection.execute_batch("BEGIN; ALTER TABLE settings ADD COLUMN store_complete_email_content INTEGER NOT NULL DEFAULT 0 CHECK(store_complete_email_content IN (0,1)); INSERT OR IGNORE INTO schema_migrations(version) VALUES (5); COMMIT;")?;
        }
        let has_analysis_setting: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('settings') WHERE name='local_email_analysis_enabled')",
            [],
            |row| row.get(0),
        )?;
        if !has_analysis_setting {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE settings ADD COLUMN local_email_analysis_enabled INTEGER NOT NULL DEFAULT 0 CHECK(local_email_analysis_enabled IN (0,1));
                 CREATE TABLE local_ai_qualification (
                   id INTEGER PRIMARY KEY CHECK(id=1),
                   model_sha256 TEXT NOT NULL CHECK(length(model_sha256)=64),
                   runtime_build TEXT NOT NULL,
                   corpus_version INTEGER NOT NULL,
                   qualified_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (6);
                 COMMIT;",
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS message_analysis (
               account_id TEXT NOT NULL,
               provider_id TEXT NOT NULL,
               summary TEXT NOT NULL CHECK(length(summary) BETWEEN 1 AND 500),
               classification TEXT NOT NULL,
               classification_confidence REAL NOT NULL CHECK(classification_confidence BETWEEN 0 AND 1),
               urgency TEXT NOT NULL,
               disposition TEXT NOT NULL,
               suggestions_json TEXT NOT NULL,
               review_reasons_json TEXT NOT NULL,
               model_sha256 TEXT NOT NULL CHECK(length(model_sha256)=64),
               analyzed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY(account_id,provider_id),
               FOREIGN KEY(account_id,provider_id) REFERENCES message_metadata(account_id,provider_id) ON DELETE CASCADE
             );
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (7);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS review_decisions (
               account_id TEXT NOT NULL,
               provider_id TEXT NOT NULL,
               decision TEXT NOT NULL CHECK(decision IN ('accept','ignore')),
               decided_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY(account_id,provider_id),
               FOREIGN KEY(account_id,provider_id) REFERENCES message_analysis(account_id,provider_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_review_decisions_time ON review_decisions(decided_at DESC);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (8);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS local_items (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               account_id TEXT NOT NULL,
               provider_id TEXT NOT NULL,
               suggestion_index INTEGER NOT NULL CHECK(suggestion_index >= 0),
               kind TEXT NOT NULL CHECK(kind IN ('task','appointment','waiting_for')),
               title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 500),
               due_at TEXT,
               start_at TEXT,
               end_at TEXT,
               expected_from TEXT,
               follow_up_at TEXT,
               category TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','undone')),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               undone_at TEXT,
               UNIQUE(account_id,provider_id,suggestion_index),
               FOREIGN KEY(account_id,provider_id) REFERENCES message_analysis(account_id,provider_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_local_items_kind_status ON local_items(kind,status,created_at DESC);
             INSERT OR IGNORE INTO local_items(account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,expected_from,follow_up_at,category)
             SELECT d.account_id,d.provider_id,CAST(j.key AS INTEGER),json_extract(j.value,'$.kind'),
                    COALESCE(json_extract(j.value,'$.title'),json_extract(j.value,'$.description')),
                    json_extract(j.value,'$.due_at'),json_extract(j.value,'$.start_at'),json_extract(j.value,'$.end_at'),
                    json_extract(j.value,'$.expected_from'),json_extract(j.value,'$.follow_up_at'),a.classification
             FROM review_decisions d JOIN message_analysis a ON a.account_id=d.account_id AND a.provider_id=d.provider_id,
                  json_each(a.suggestions_json) j
             WHERE d.decision='accept';
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (9);
             COMMIT;",
        )?;
        let has_lifecycle_state: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('local_items') WHERE name='lifecycle_state')",
            [],
            |row| row.get(0),
        )?;
        if !has_lifecycle_state {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE local_items ADD COLUMN lifecycle_state TEXT NOT NULL DEFAULT 'active' CHECK(lifecycle_state IN ('active','completed','snoozed'));
                 ALTER TABLE local_items ADD COLUMN scheduled_at TEXT;
                 COMMIT;",
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS local_item_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               item_id INTEGER NOT NULL REFERENCES local_items(id) ON DELETE CASCADE,
               event_type TEXT NOT NULL CHECK(event_type IN ('complete','snooze','reschedule')),
               previous_state TEXT NOT NULL,
               new_state TEXT NOT NULL,
               previous_scheduled_at TEXT,
               new_scheduled_at TEXT,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS idx_local_item_events_item ON local_item_events(item_id,created_at DESC);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (10);
             COMMIT;",
        )?;
        let has_automation_policy: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('settings') WHERE name='automation_policy')",
            [],
            |row| row.get(0),
        )?;
        if !has_automation_policy {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE settings ADD COLUMN automation_policy TEXT NOT NULL DEFAULT 'balanced' CHECK(automation_policy IN ('conservative','balanced','assistant'));
                 CREATE TABLE automation_audit (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
                   policy TEXT NOT NULL CHECK(policy IN ('conservative','balanced','assistant')),
                   decision TEXT NOT NULL CHECK(decision IN ('blocked','review','confirmation_required','eligible','confirmed','executed','failed','undone')),
                   action_kind TEXT NOT NULL CHECK(action_kind IN ('task_create','calendar_create','calendar_update','correspondence','notification')),
                   target_ref TEXT CHECK(target_ref IS NULL OR length(target_ref) BETWEEN 1 AND 200),
                   reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 1 AND 100),
                   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 CREATE TRIGGER automation_audit_no_update BEFORE UPDATE ON automation_audit BEGIN SELECT RAISE(ABORT, 'automation audit is append-only'); END;
                 CREATE TRIGGER automation_audit_no_delete BEFORE DELETE ON automation_audit BEGIN SELECT RAISE(ABORT, 'automation audit is append-only'); END;
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (11);
                 COMMIT;",
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS action_proposals (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               proposal_key TEXT NOT NULL UNIQUE CHECK(length(proposal_key) BETWEEN 16 AND 100),
               audit_event_key TEXT NOT NULL UNIQUE REFERENCES automation_audit(event_key),
               action_kind TEXT NOT NULL CHECK(action_kind IN ('task_create','calendar_create','calendar_update','correspondence','notification')),
               sensitivity TEXT NOT NULL CHECK(sensitivity IN ('routine','important','sensitive')),
               policy TEXT NOT NULL CHECK(policy IN ('conservative','balanced','assistant')),
               disposition TEXT NOT NULL CHECK(disposition IN ('review','confirmation_required','eligible')),
               reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 1 AND 100),
               display_label TEXT NOT NULL CHECK(length(display_label) BETWEEN 1 AND 200),
               target_ref TEXT CHECK(target_ref IS NULL OR length(target_ref) BETWEEN 1 AND 100),
               confidence REAL NOT NULL CHECK(confidence BETWEEN 0 AND 1),
               expires_at TEXT NOT NULL,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS idx_action_proposals_expiry ON action_proposals(expires_at,created_at);
             CREATE TRIGGER IF NOT EXISTS action_proposals_no_update BEFORE UPDATE ON action_proposals BEGIN SELECT RAISE(ABORT, 'action proposals are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS action_proposals_no_delete BEFORE DELETE ON action_proposals BEGIN SELECT RAISE(ABORT, 'action proposals are immutable'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (12);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS action_proposal_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100) REFERENCES automation_audit(event_key),
               proposal_id INTEGER NOT NULL UNIQUE REFERENCES action_proposals(id),
               event_type TEXT NOT NULL CHECK(event_type IN ('confirm','cancel')),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS action_proposal_events_no_update BEFORE UPDATE ON action_proposal_events BEGIN SELECT RAISE(ABORT, 'action proposal events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS action_proposal_events_no_delete BEFORE DELETE ON action_proposal_events BEGIN SELECT RAISE(ABORT, 'action proposal events are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (13);
             COMMIT;",
        )?;
        let has_notification_settings: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('settings') WHERE name='urgent_alerts_enabled')",
            [],
            |row| row.get(0),
        )?;
        if !has_notification_settings {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE settings ADD COLUMN urgent_alerts_enabled INTEGER NOT NULL DEFAULT 0 CHECK(urgent_alerts_enabled IN (0,1));
                 ALTER TABLE settings ADD COLUMN appointment_reminders_enabled INTEGER NOT NULL DEFAULT 0 CHECK(appointment_reminders_enabled IN (0,1));
                 ALTER TABLE settings ADD COLUMN morning_summary_enabled INTEGER NOT NULL DEFAULT 0 CHECK(morning_summary_enabled IN (0,1));
                 ALTER TABLE settings ADD COLUMN evening_summary_enabled INTEGER NOT NULL DEFAULT 0 CHECK(evening_summary_enabled IN (0,1));
                 ALTER TABLE settings ADD COLUMN quiet_hours_start_minute INTEGER NOT NULL DEFAULT 1320 CHECK(quiet_hours_start_minute BETWEEN 0 AND 1439);
                 ALTER TABLE settings ADD COLUMN quiet_hours_end_minute INTEGER NOT NULL DEFAULT 420 CHECK(quiet_hours_end_minute BETWEEN 0 AND 1439);
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (14);
                 COMMIT;",
            )?;
        }
        let has_delivery_consent: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('settings') WHERE name='notification_delivery_enabled')",
            [],
            |row| row.get(0),
        )?;
        if !has_delivery_consent {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE settings ADD COLUMN notification_delivery_enabled INTEGER NOT NULL DEFAULT 0 CHECK(notification_delivery_enabled IN (0,1));
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (15);
                 COMMIT;",
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS notification_schedule_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               delivery_key TEXT NOT NULL CHECK(length(delivery_key) BETWEEN 16 AND 100),
               event_type TEXT NOT NULL CHECK(event_type IN ('scheduled','cancelled','delivered','failed')),
               notification_kind TEXT NOT NULL CHECK(notification_kind IN ('reminder','urgent_alert','morning_summary','evening_summary')),
               deliver_at TEXT NOT NULL CHECK(length(deliver_at) BETWEEN 20 AND 40),
               reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 2 AND 80),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS idx_notification_schedule_delivery ON notification_schedule_events(delivery_key,id);
             CREATE TRIGGER IF NOT EXISTS notification_schedule_events_no_update BEFORE UPDATE ON notification_schedule_events BEGIN SELECT RAISE(ABORT, 'notification schedule events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS notification_schedule_events_no_delete BEFORE DELETE ON notification_schedule_events BEGIN SELECT RAISE(ABORT, 'notification schedule events are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (16);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS calendar_executions (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               execution_key TEXT NOT NULL UNIQUE CHECK(length(execution_key) BETWEEN 16 AND 100),
               proposal_id INTEGER NOT NULL UNIQUE REFERENCES action_proposals(id),
               transaction_id TEXT NOT NULL UNIQUE CHECK(length(transaction_id) BETWEEN 16 AND 100),
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS calendar_execution_results (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               execution_id INTEGER NOT NULL UNIQUE REFERENCES calendar_executions(id),
               outcome TEXT NOT NULL CHECK(outcome IN ('succeeded','failed','unknown')),
               provider_event_id TEXT CHECK(provider_event_id IS NULL OR length(provider_event_id) BETWEEN 1 AND 512),
               reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 2 AND 80),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS calendar_executions_no_update BEFORE UPDATE ON calendar_executions BEGIN SELECT RAISE(ABORT, 'calendar executions are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS calendar_executions_no_delete BEFORE DELETE ON calendar_executions BEGIN SELECT RAISE(ABORT, 'calendar executions are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS calendar_execution_results_no_update BEFORE UPDATE ON calendar_execution_results BEGIN SELECT RAISE(ABORT, 'calendar execution results are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS calendar_execution_results_no_delete BEFORE DELETE ON calendar_execution_results BEGIN SELECT RAISE(ABORT, 'calendar execution results are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (17);
             COMMIT;",
        )?;
        let has_calendar_etag: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('calendar_events') WHERE name='provider_etag')",
            [],
            |row| row.get(0),
        )?;
        if !has_calendar_etag {
            connection.execute_batch(
                "BEGIN;
                 ALTER TABLE calendar_events ADD COLUMN provider_etag TEXT CHECK(provider_etag IS NULL OR length(provider_etag) BETWEEN 1 AND 512);
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (18);
                 COMMIT;",
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS calendar_update_targets (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               proposal_id INTEGER NOT NULL UNIQUE REFERENCES action_proposals(id),
               account_id TEXT NOT NULL REFERENCES connected_accounts(id),
               provider_event_id TEXT NOT NULL CHECK(length(provider_event_id) BETWEEN 1 AND 512),
               provider_etag TEXT NOT NULL CHECK(length(provider_etag) BETWEEN 1 AND 512),
               previous_start_at TEXT NOT NULL CHECK(length(previous_start_at) BETWEEN 10 AND 50),
               previous_end_at TEXT NOT NULL CHECK(length(previous_end_at) BETWEEN 10 AND 50),
               proposed_start_at TEXT NOT NULL CHECK(length(proposed_start_at) BETWEEN 20 AND 40),
               proposed_end_at TEXT NOT NULL CHECK(length(proposed_end_at) BETWEEN 20 AND 40),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS calendar_update_targets_no_update BEFORE UPDATE ON calendar_update_targets BEGIN SELECT RAISE(ABORT, 'calendar update targets are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS calendar_update_targets_no_delete BEFORE DELETE ON calendar_update_targets BEGIN SELECT RAISE(ABORT, 'calendar update targets are immutable'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (19);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS reply_drafts (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               draft_key TEXT NOT NULL UNIQUE CHECK(length(draft_key) BETWEEN 16 AND 100),
               account_id TEXT NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
               provider_message_id TEXT NOT NULL CHECK(length(provider_message_id) BETWEEN 1 AND 512),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               UNIQUE(account_id,provider_message_id)
             );
             CREATE TABLE IF NOT EXISTS reply_draft_revisions (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               revision_key TEXT NOT NULL UNIQUE CHECK(length(revision_key) BETWEEN 16 AND 100),
               draft_id INTEGER NOT NULL REFERENCES reply_drafts(id) ON DELETE CASCADE,
               content_sha256 TEXT NOT NULL CHECK(length(content_sha256)=64),
               content_bytes INTEGER NOT NULL CHECK(content_bytes BETWEEN 1 AND 16384),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS reply_drafts_no_update BEFORE UPDATE ON reply_drafts BEGIN SELECT RAISE(ABORT, 'reply draft targets are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS reply_draft_revisions_no_update BEFORE UPDATE ON reply_draft_revisions BEGIN SELECT RAISE(ABORT, 'reply draft revisions are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (20);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS correspondence_targets (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               proposal_id INTEGER NOT NULL UNIQUE REFERENCES action_proposals(id),
               revision_key TEXT NOT NULL UNIQUE CHECK(length(revision_key) BETWEEN 16 AND 100),
               content_sha256 TEXT NOT NULL CHECK(length(content_sha256)=64),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS correspondence_targets_no_update BEFORE UPDATE ON correspondence_targets BEGIN SELECT RAISE(ABORT, 'correspondence targets are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS correspondence_targets_no_delete BEFORE DELETE ON correspondence_targets BEGIN SELECT RAISE(ABORT, 'correspondence targets are immutable'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (21);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS correspondence_executions (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               execution_key TEXT NOT NULL UNIQUE CHECK(length(execution_key) BETWEEN 16 AND 100),
               proposal_id INTEGER NOT NULL UNIQUE REFERENCES action_proposals(id),
               account_id TEXT NOT NULL CHECK(length(account_id) BETWEEN 1 AND 100),
               revision_key TEXT NOT NULL UNIQUE CHECK(length(revision_key) BETWEEN 16 AND 100),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS correspondence_execution_results (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               execution_id INTEGER NOT NULL UNIQUE REFERENCES correspondence_executions(id),
               outcome TEXT NOT NULL CHECK(outcome IN ('succeeded','failed','unknown')),
               reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 2 AND 80),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS correspondence_executions_no_update BEFORE UPDATE ON correspondence_executions BEGIN SELECT RAISE(ABORT, 'correspondence executions are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS correspondence_executions_no_delete BEFORE DELETE ON correspondence_executions BEGIN SELECT RAISE(ABORT, 'correspondence executions are immutable'); END;
             CREATE TRIGGER IF NOT EXISTS correspondence_execution_results_no_update BEFORE UPDATE ON correspondence_execution_results BEGIN SELECT RAISE(ABORT, 'correspondence execution results are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS correspondence_execution_results_no_delete BEFORE DELETE ON correspondence_execution_results BEGIN SELECT RAISE(ABORT, 'correspondence execution results are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (22);
             COMMIT;",
        )?;
        let local_items_require_review_decision: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_list('local_items') WHERE \"table\"='review_decisions')",
            [],
            |row| row.get(0),
        )?;
        if local_items_require_review_decision {
            connection.execute_batch(
                "PRAGMA foreign_keys=OFF;
                 BEGIN;
                 CREATE TABLE local_items_v23 (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   account_id TEXT NOT NULL,
                   provider_id TEXT NOT NULL,
                   suggestion_index INTEGER NOT NULL CHECK(suggestion_index >= 0),
                   kind TEXT NOT NULL CHECK(kind IN ('task','appointment','waiting_for')),
                   title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 500),
                   due_at TEXT,
                   start_at TEXT,
                   end_at TEXT,
                   expected_from TEXT,
                   follow_up_at TEXT,
                   category TEXT NOT NULL,
                   status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','undone')),
                   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                   undone_at TEXT,
                   lifecycle_state TEXT NOT NULL DEFAULT 'active' CHECK(lifecycle_state IN ('active','completed','snoozed')),
                   scheduled_at TEXT,
                   UNIQUE(account_id,provider_id,suggestion_index),
                   FOREIGN KEY(account_id,provider_id) REFERENCES message_analysis(account_id,provider_id) ON DELETE CASCADE
                 );
                 INSERT INTO local_items_v23(id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,expected_from,follow_up_at,category,status,created_at,undone_at,lifecycle_state,scheduled_at)
                 SELECT id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,expected_from,follow_up_at,category,status,created_at,undone_at,lifecycle_state,scheduled_at FROM local_items;
                 DROP TABLE local_items;
                 ALTER TABLE local_items_v23 RENAME TO local_items;
                 CREATE INDEX idx_local_items_kind_status ON local_items(kind,status,created_at DESC);
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (23);
                 COMMIT;
                 PRAGMA foreign_keys=ON;",
            )?;
        } else {
            connection.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (23)",
                [],
            )?;
        }
        let audit_supports_policy_dispositions: bool = connection.query_row(
            "SELECT instr(sql, '''confirmation_required''') > 0 AND instr(sql, '''eligible''') > 0 FROM sqlite_master WHERE type='table' AND name='automation_audit'",
            [],
            |row| row.get(0),
        )?;
        if !audit_supports_policy_dispositions {
            connection.execute_batch(
                "PRAGMA foreign_keys=OFF;
                 BEGIN;
                 DROP TRIGGER automation_audit_no_update;
                 DROP TRIGGER automation_audit_no_delete;
                 CREATE TABLE automation_audit_v24 (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
                   policy TEXT NOT NULL CHECK(policy IN ('conservative','balanced','assistant')),
                   decision TEXT NOT NULL CHECK(decision IN ('blocked','review','confirmation_required','eligible','confirmed','executed','failed','undone')),
                   action_kind TEXT NOT NULL CHECK(action_kind IN ('task_create','calendar_create','calendar_update','correspondence','notification')),
                   target_ref TEXT CHECK(target_ref IS NULL OR length(target_ref) BETWEEN 1 AND 200),
                   reason_code TEXT NOT NULL CHECK(length(reason_code) BETWEEN 1 AND 100),
                   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO automation_audit_v24(id,event_key,policy,decision,action_kind,target_ref,reason_code,created_at)
                 SELECT id,event_key,policy,decision,action_kind,target_ref,reason_code,created_at FROM automation_audit;
                 DROP TABLE automation_audit;
                 ALTER TABLE automation_audit_v24 RENAME TO automation_audit;
                 CREATE TRIGGER automation_audit_no_update BEFORE UPDATE ON automation_audit BEGIN SELECT RAISE(ABORT, 'automation audit is append-only'); END;
                 CREATE TRIGGER automation_audit_no_delete BEFORE DELETE ON automation_audit BEGIN SELECT RAISE(ABORT, 'automation audit is append-only'); END;
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (24);
                 COMMIT;
                 PRAGMA foreign_keys=ON;",
            )?;
        } else {
            connection.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (24)",
                [],
            )?;
        }
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS family_displays (
               id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 16 AND 100),
               display_name TEXT NOT NULL CHECK(length(display_name) BETWEEN 1 AND 80),
               token_sha256 BLOB NOT NULL CHECK(length(token_sha256)=32),
               policy_json TEXT NOT NULL CHECK(length(policy_json) BETWEEN 2 AND 8192),
               last_seen_at TEXT,
               revoked_at TEXT,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS family_display_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               display_id TEXT NOT NULL REFERENCES family_displays(id),
               event_type TEXT NOT NULL CHECK(event_type IN ('paired','revoked')),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS family_display_events_no_update BEFORE UPDATE ON family_display_events BEGIN SELECT RAISE(ABORT, 'family display events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS family_display_events_no_delete BEFORE DELETE ON family_display_events BEGIN SELECT RAISE(ABORT, 'family display events are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (25);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS local_item_display_privacy (
               item_id INTEGER PRIMARY KEY REFERENCES local_items(id) ON DELETE CASCADE,
               privacy_profile TEXT NOT NULL CHECK(privacy_profile IN ('PUBLIC_FAMILY','PRIVATE','WORK_PRIVATE','SENSITIVE'))
             );
             INSERT OR IGNORE INTO local_item_display_privacy(item_id,privacy_profile)
             SELECT id,CASE
               WHEN category IN ('school','creche','kids','family','home') THEN 'PUBLIC_FAMILY'
               WHEN category IN ('work','work_administration') THEN 'WORK_PRIVATE'
               ELSE 'PRIVATE'
             END FROM local_items;
             CREATE TRIGGER IF NOT EXISTS local_items_default_display_privacy AFTER INSERT ON local_items
             BEGIN
               INSERT OR IGNORE INTO local_item_display_privacy(item_id,privacy_profile)
               VALUES (NEW.id,CASE
                 WHEN NEW.category IN ('school','creche','kids','family','home') THEN 'PUBLIC_FAMILY'
                 WHEN NEW.category IN ('work','work_administration') THEN 'WORK_PRIVATE'
                 ELSE 'PRIVATE'
               END);
             END;
             CREATE TABLE IF NOT EXISTS local_item_display_privacy_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               item_id INTEGER NOT NULL REFERENCES local_items(id) ON DELETE CASCADE,
               privacy_profile TEXT NOT NULL CHECK(privacy_profile IN ('PUBLIC_FAMILY','PRIVATE','WORK_PRIVATE','SENSITIVE')),
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS local_item_display_privacy_events_no_update BEFORE UPDATE ON local_item_display_privacy_events BEGIN SELECT RAISE(ABORT, 'display privacy events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS local_item_display_privacy_events_no_delete BEFORE DELETE ON local_item_display_privacy_events BEGIN SELECT RAISE(ABORT, 'display privacy events are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (26);
             COMMIT;",
        )?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS family_display_service_config (
               id INTEGER PRIMARY KEY CHECK(id=1),
               enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
               bind_address TEXT CHECK(bind_address IS NULL OR length(bind_address) BETWEEN 9 AND 80),
               certificate_sha256 BLOB CHECK(certificate_sha256 IS NULL OR length(certificate_sha256)=32),
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               CHECK(enabled=0 OR (bind_address IS NOT NULL AND certificate_sha256 IS NOT NULL))
             );
             INSERT OR IGNORE INTO family_display_service_config(id,enabled) VALUES (1,0);
             CREATE TABLE IF NOT EXISTS family_display_service_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               event_key TEXT NOT NULL UNIQUE CHECK(length(event_key) BETWEEN 16 AND 100),
               enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
               bind_address TEXT,
               certificate_sha256 BLOB,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TRIGGER IF NOT EXISTS family_display_service_events_no_update BEFORE UPDATE ON family_display_service_events BEGIN SELECT RAISE(ABORT, 'family display service events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS family_display_service_events_no_delete BEFORE DELETE ON family_display_service_events BEGIN SELECT RAISE(ABORT, 'family display service events are append-only'); END;
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (27);
             COMMIT;",
        )?;
        Self::validate_migration_ledger(connection, true)?;
        Ok(())
    }

    fn validate_migration_ledger(
        connection: &Connection,
        require_current: bool,
    ) -> Result<(), rusqlite::Error> {
        let (count, minimum, maximum): (i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(MIN(version), 0), COALESCE(MAX(version), 0)
             FROM schema_migrations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let contiguous = count == 0 || (minimum == 1 && count == maximum);
        if !contiguous
            || maximum > CURRENT_SCHEMA_VERSION
            || (require_current && maximum != CURRENT_SCHEMA_VERSION)
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(())
    }

    pub fn family_display_service_config(
        &self,
    ) -> Result<FamilyDisplayServiceConfig, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.query_row(
            "SELECT enabled,bind_address,certificate_sha256 FROM family_display_service_config WHERE id=1",
            [],
            |row| {
                let digest: Option<Vec<u8>> = row.get(2)?;
                Ok(FamilyDisplayServiceConfig {
                    enabled: row.get(0)?,
                    bind_address: row.get(1)?,
                    certificate_sha256: digest.map(|value| encode_digest(&value)),
                })
            },
        ).map_err(DatabaseError::from)
    }

    pub fn configure_family_display_service(
        &self,
        enabled: bool,
        bind_address: Option<&str>,
        certificate_sha256: Option<&[u8; 32]>,
        event_key: &str,
    ) -> Result<FamilyDisplayServiceConfig, DatabaseError> {
        if !valid_display_identifier(event_key)
            || !valid_family_display_service_values(enabled, bind_address, certificate_sha256)
        {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE family_display_service_config SET enabled=?1,bind_address=?2,certificate_sha256=?3,updated_at=CURRENT_TIMESTAMP WHERE id=1",
            params![enabled, bind_address, certificate_sha256.map(|value| value.as_slice())],
        )?;
        transaction.execute(
            "INSERT INTO family_display_service_events(event_key,enabled,bind_address,certificate_sha256) VALUES (?1,?2,?3,?4)",
            params![event_key, enabled, bind_address, certificate_sha256.map(|value| value.as_slice())],
        )?;
        transaction.commit()?;
        drop(connection);
        self.family_display_service_config()
    }

    pub fn register_family_display(
        &self,
        display_id: &str,
        display_name: &str,
        token_sha256: &[u8; 32],
        policy: &display_api::DisplayPolicy,
        event_key: &str,
    ) -> Result<FamilyDisplayRecord, DatabaseError> {
        if !valid_display_identifier(display_id)
            || !valid_display_identifier(event_key)
            || display_name.trim().is_empty()
            || display_name.trim().chars().count() > 80
            || display_name.chars().any(char::is_control)
        {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let policy_json =
            serde_json::to_string(policy).map_err(|_| DatabaseError::InvalidFamilyDisplay)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO family_displays(id,display_name,token_sha256,policy_json) VALUES (?1,?2,?3,?4)",
            params![display_id, display_name.trim(), token_sha256.as_slice(), policy_json],
        )?;
        transaction.execute(
            "INSERT INTO family_display_events(event_key,display_id,event_type) VALUES (?1,?2,'paired')",
            params![event_key, display_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.family_display(display_id)
    }

    pub fn family_displays(&self) -> Result<Vec<FamilyDisplayRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id,display_name,revoked_at IS NOT NULL,last_seen_at,created_at FROM family_displays ORDER BY created_at,id",
        )?;
        let records = statement
            .query_map([], family_display_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::from)?;
        Ok(records)
    }

    fn family_display(&self, display_id: &str) -> Result<FamilyDisplayRecord, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT id,display_name,revoked_at IS NOT NULL,last_seen_at,created_at FROM family_displays WHERE id=?1",
                [display_id],
                family_display_from_row,
            )
            .optional()?
            .ok_or(DatabaseError::FamilyDisplayMissing)
    }

    pub fn authenticate_family_display(
        &self,
        display_id: &str,
        presented_sha256: &[u8; 32],
        seen_at: &str,
    ) -> Result<bool, DatabaseError> {
        if !valid_display_identifier(display_id)
            || chrono::DateTime::parse_from_rfc3339(seen_at).is_err()
        {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let stored: Option<Vec<u8>> = connection
            .query_row(
                "SELECT token_sha256 FROM family_displays WHERE id=?1 AND revoked_at IS NULL",
                [display_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let matches = stored.len() == 32
            && stored
                .iter()
                .zip(presented_sha256.iter())
                .fold(0_u8, |difference, (left, right)| {
                    difference | (left ^ right)
                })
                == 0;
        if matches {
            connection.execute(
                "UPDATE family_displays SET last_seen_at=?2 WHERE id=?1 AND revoked_at IS NULL",
                params![display_id, seen_at],
            )?;
        }
        Ok(matches)
    }

    pub fn revoke_family_display(
        &self,
        display_id: &str,
        event_key: &str,
    ) -> Result<(), DatabaseError> {
        if !valid_display_identifier(display_id) || !valid_display_identifier(event_key) {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE family_displays SET revoked_at=CURRENT_TIMESTAMP WHERE id=?1 AND revoked_at IS NULL",
            [display_id],
        )?;
        if changed == 0 {
            return Err(DatabaseError::FamilyDisplayMissing);
        }
        transaction.execute(
            "INSERT INTO family_display_events(event_key,display_id,event_type) VALUES (?1,?2,'revoked')",
            params![event_key, display_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_local_item_display_privacy(
        &self,
        item_id: i64,
        privacy: display_api::PrivacyProfile,
        event_key: &str,
    ) -> Result<(), DatabaseError> {
        if item_id <= 0 || !valid_display_identifier(event_key) {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let profile = display_privacy_name(privacy);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_items WHERE id=?1)",
            [item_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(DatabaseError::LocalItemMissing);
        }
        transaction.execute(
            "INSERT INTO local_item_display_privacy_events(event_key,item_id,privacy_profile) VALUES (?1,?2,?3)",
            params![event_key, item_id, profile],
        )?;
        transaction.execute(
            "INSERT INTO local_item_display_privacy(item_id,privacy_profile) VALUES (?1,?2)
             ON CONFLICT(item_id) DO UPDATE SET privacy_profile=excluded.privacy_profile",
            params![item_id, profile],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn family_display_snapshot(
        &self,
        display_id: &str,
        generated_at: &str,
        mode: display_api::DisplayMode,
    ) -> Result<display_api::DisplaySnapshot, DatabaseError> {
        if !valid_display_identifier(display_id)
            || chrono::DateTime::parse_from_rfc3339(generated_at).is_err()
        {
            return Err(DatabaseError::InvalidFamilyDisplay);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let policy_json: String = connection
            .query_row(
                "SELECT policy_json FROM family_displays WHERE id=?1 AND revoked_at IS NULL",
                [display_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::FamilyDisplayMissing)?;
        let policy: display_api::DisplayPolicy =
            serde_json::from_str(&policy_json).map_err(|_| DatabaseError::InvalidFamilyDisplay)?;
        let mut statement = connection.prepare(
            "SELECT i.id,i.kind,i.title,i.category,p.privacy_profile,
                    COALESCE(i.scheduled_at,i.due_at,i.start_at,i.follow_up_at),i.end_at
             FROM local_items i
             JOIN local_item_display_privacy p ON p.item_id=i.id
             WHERE i.status='active' AND i.lifecycle_state!='completed'
             ORDER BY COALESCE(i.scheduled_at,i.due_at,i.start_at,i.follow_up_at,i.created_at),i.id
             LIMIT 100",
        )?;
        let source_rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);

        let mut items = Vec::new();
        for (id, kind, title, category, privacy, starts_at, ends_at) in source_rows {
            let digest = Sha256::digest(format!("{display_id}:{id}").as_bytes());
            let starts_at = starts_at.as_deref().and_then(normalize_display_timestamp);
            let ends_at = ends_at.as_deref().and_then(normalize_display_timestamp);
            let projection = display_api::project_item(
                &policy,
                display_api::ProjectionInput {
                    display_id: &format!("item-{digest:x}"),
                    title: &title,
                    kind: display_kind(&kind),
                    category: display_category(&category),
                    privacy: parse_display_privacy(&privacy)?,
                    starts_at: starts_at.as_deref(),
                    ends_at: ends_at.as_deref(),
                },
            )
            .map_err(|_| DatabaseError::InvalidFamilyDisplay)?;
            if let Some(item) = projection.1 {
                items.push(item);
            }
        }
        display_api::build_snapshot(generated_at, mode, items)
            .map_err(|_| DatabaseError::InvalidFamilyDisplay)
    }

    pub fn settings(&self) -> Result<Settings, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT household_name,theme,launch_at_login,store_complete_email_content,local_email_analysis_enabled,automation_policy,notification_delivery_enabled,urgent_alerts_enabled,appointment_reminders_enabled,morning_summary_enabled,evening_summary_enabled,quiet_hours_start_minute,quiet_hours_end_minute FROM settings WHERE id=1",
                [],
                |row| {
                    let theme: String = row.get(1)?;
                    let parsed = Theme::try_from(theme.as_str())
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let policy: String = row.get(5)?;
                    let automation_policy = AutomationPolicy::try_from(policy.as_str())
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(Settings {
                        household_name: row.get(0)?,
                        theme: parsed,
                        launch_at_login: row.get(2)?,
                        store_complete_email_content: row.get(3)?,
                        local_email_analysis_enabled: row.get(4)?,
                        automation_policy,
                        notification_delivery_enabled: row.get(6)?,
                        urgent_alerts_enabled: row.get(7)?,
                        appointment_reminders_enabled: row.get(8)?,
                        morning_summary_enabled: row.get(9)?,
                        evening_summary_enabled: row.get(10)?,
                        quiet_hours_start_minute: row.get(11)?,
                        quiet_hours_end_minute: row.get(12)?,
                    })
                },
            )
            .optional()?
            .ok_or(DatabaseError::SettingsMissing)
    }

    pub fn save_settings(&self, settings: Settings) -> Result<Settings, DatabaseError> {
        let settings = settings.validate()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        if settings.local_email_analysis_enabled {
            let qualified: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM local_ai_qualification WHERE id=1)",
                [],
                |row| row.get(0),
            )?;
            if !qualified {
                return Err(DatabaseError::AiNotQualified);
            }
        }
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE settings SET household_name=?1,theme=?2,launch_at_login=?3,store_complete_email_content=?4,local_email_analysis_enabled=?5,automation_policy=?6,notification_delivery_enabled=?7,urgent_alerts_enabled=?8,appointment_reminders_enabled=?9,morning_summary_enabled=?10,evening_summary_enabled=?11,quiet_hours_start_minute=?12,quiet_hours_end_minute=?13,updated_at=CURRENT_TIMESTAMP WHERE id=1",
            params![settings.household_name,settings.theme.as_str(),settings.launch_at_login,settings.store_complete_email_content,settings.local_email_analysis_enabled,settings.automation_policy.as_str(),settings.notification_delivery_enabled,settings.urgent_alerts_enabled,settings.appointment_reminders_enabled,settings.morning_summary_enabled,settings.evening_summary_enabled,settings.quiet_hours_start_minute,settings.quiet_hours_end_minute],
        )?;
        transaction.commit()?;
        Ok(settings)
    }

    pub fn record_ai_qualification(
        &self,
        model_sha256: &str,
        runtime_build: &str,
        corpus_version: u16,
    ) -> Result<(), DatabaseError> {
        if model_sha256.len() != 64
            || !model_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || runtime_build.is_empty()
        {
            return Err(DatabaseError::InvalidQualification);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO local_ai_qualification(id,model_sha256,runtime_build,corpus_version) VALUES(1,?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET model_sha256=excluded.model_sha256,runtime_build=excluded.runtime_build,corpus_version=excluded.corpus_version,qualified_at=CURRENT_TIMESTAMP",
            params![model_sha256.to_ascii_lowercase(), runtime_build, corpus_version],
        )?;
        Ok(())
    }

    pub fn has_ai_qualification(
        &self,
        model_sha256: &str,
        runtime_build: &str,
        corpus_version: u16,
    ) -> Result<bool, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_ai_qualification WHERE id=1 AND model_sha256=?1 AND runtime_build=?2 AND corpus_version=?3)",
                params![model_sha256.to_ascii_lowercase(), runtime_build, corpus_version],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn clear_ai_qualification(&self) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute("DELETE FROM local_ai_qualification", [])?;
        connection.execute(
            "UPDATE settings SET local_email_analysis_enabled=0,updated_at=CURRENT_TIMESTAMP WHERE id=1",
            [],
        )?;
        Ok(())
    }

    /// Persists only validated derived fields. Source evidence and provider body
    /// content are structurally absent from the stored representation.
    pub fn save_message_analysis(
        &self,
        account_id: &str,
        provider_id: &str,
        extraction: &ai::Extraction,
        plan: &rules::RulePlan,
        model_sha256: &str,
    ) -> Result<(), DatabaseError> {
        if model_sha256.len() != 64 || !model_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DatabaseError::InvalidQualification);
        }
        let classification = enum_name(&extraction.classification)?;
        let urgency = enum_name(&extraction.urgency)?;
        let disposition = enum_name(&plan.disposition)?;
        let suggestions = plan
            .proposals
            .iter()
            .map(|proposal| sanitized_suggestion(extraction, proposal))
            .collect::<Result<Vec<_>, _>>()?;
        let suggestions_json =
            serde_json::to_string(&suggestions).map_err(|_| DatabaseError::InvalidAnalysis)?;
        let review_reasons_json = serde_json::to_string(&plan.review_reasons)
            .map_err(|_| DatabaseError::InvalidAnalysis)?;
        if suggestions_json.len() > 32 * 1024 || review_reasons_json.len() > 8 * 1024 {
            return Err(DatabaseError::InvalidAnalysis);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM message_metadata WHERE account_id=?1 AND provider_id=?2 AND deleted_at IS NULL)",
            params![account_id, provider_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(DatabaseError::MessageMissing);
        }
        connection.execute(
            "INSERT INTO message_analysis(account_id,provider_id,summary,classification,classification_confidence,urgency,disposition,suggestions_json,review_reasons_json,model_sha256)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(account_id,provider_id) DO UPDATE SET summary=excluded.summary,classification=excluded.classification,classification_confidence=excluded.classification_confidence,urgency=excluded.urgency,disposition=excluded.disposition,suggestions_json=excluded.suggestions_json,review_reasons_json=excluded.review_reasons_json,model_sha256=excluded.model_sha256,analyzed_at=CURRENT_TIMESTAMP",
            params![account_id,provider_id,extraction.summary,classification,extraction.classification_confidence,urgency,disposition,suggestions_json,review_reasons_json,model_sha256.to_ascii_lowercase()],
        )?;
        Ok(())
    }

    /// Applies only reversible local projections when every suggestion in the
    /// validated plan independently passes the deterministic automation policy.
    /// Mixed plans remain wholly in Review; this method performs no provider I/O.
    pub fn apply_policy_local_projection(
        &self,
        account_id: &str,
        provider_id: &str,
        extraction: &ai::Extraction,
        plan: &rules::RulePlan,
    ) -> Result<usize, DatabaseError> {
        if plan.disposition != rules::PlanDisposition::Suggestions || plan.proposals.is_empty() {
            return Ok(0);
        }
        let policy = self.settings()?.automation_policy;
        let important = matches!(
            extraction.urgency,
            ai::Urgency::High | ai::Urgency::Critical
        ) || extraction.classification == ai::Classification::WorkAdministration;
        let sensitivity = if extraction.urgency == ai::Urgency::Critical {
            rules::ActionSensitivity::Sensitive
        } else if important {
            rules::ActionSensitivity::Important
        } else {
            rules::ActionSensitivity::Routine
        };
        let decisions = plan
            .proposals
            .iter()
            .map(|proposal| {
                let (action, confidence, fact_confirmed) = match proposal {
                    rules::Proposal::Task { candidate_index } => (
                        rules::AutomationAction::TaskCreate,
                        extraction
                            .tasks
                            .get(*candidate_index)
                            .ok_or(DatabaseError::InvalidAnalysis)?
                            .confidence,
                        true,
                    ),
                    rules::Proposal::Appointment { candidate_index } => {
                        let candidate = extraction
                            .appointments
                            .get(*candidate_index)
                            .ok_or(DatabaseError::InvalidAnalysis)?;
                        (
                            rules::AutomationAction::CalendarCreate,
                            candidate.confidence,
                            candidate.confirmed,
                        )
                    }
                    rules::Proposal::WaitingFor { candidate_index } => (
                        rules::AutomationAction::TaskCreate,
                        extraction
                            .waiting_for
                            .get(*candidate_index)
                            .ok_or(DatabaseError::InvalidAnalysis)?
                            .confidence,
                        true,
                    ),
                };
                Ok((
                    action,
                    rules::decide_automation(
                        policy,
                        rules::AutomationContext {
                            action,
                            sensitivity,
                            confidence,
                            source_verified: true,
                            fact_confirmed,
                            reversible: true,
                            user_confirmed: false,
                        },
                    ),
                ))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        let all_eligible = decisions
            .iter()
            .all(|(_, decision)| decision.disposition == rules::AutomationDisposition::Eligible);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let source_available: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM message_metadata WHERE account_id=?1 AND provider_id=?2 AND deleted_at IS NULL)",
            params![account_id, provider_id],
            |row| row.get(0),
        )?;
        if !source_available {
            return Err(DatabaseError::MessageMissing);
        }
        for (index, (action, decision)) in decisions.iter().enumerate() {
            let digest = Sha256::digest(format!("{account_id}:{provider_id}:{index}").as_bytes());
            let event_key = format!("auto-local-{digest:x}");
            transaction.execute(
                "INSERT OR IGNORE INTO automation_audit(event_key,policy,decision,action_kind,reason_code) VALUES (?1,?2,?3,?4,?5)",
                params![
                    event_key,
                    enum_name(&policy)?,
                    enum_name(&decision.disposition)?,
                    enum_name(action)?,
                    enum_name(&decision.reason)?,
                ],
            )?;
        }
        let projected = if all_eligible {
            transaction.execute(
                "INSERT OR IGNORE INTO local_items(account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,expected_from,follow_up_at,category)
                 SELECT a.account_id,a.provider_id,CAST(j.key AS INTEGER),json_extract(j.value,'$.kind'),
                        COALESCE(json_extract(j.value,'$.title'),json_extract(j.value,'$.description')),
                        json_extract(j.value,'$.due_at'),json_extract(j.value,'$.start_at'),json_extract(j.value,'$.end_at'),
                        json_extract(j.value,'$.expected_from'),json_extract(j.value,'$.follow_up_at'),a.classification
                 FROM message_analysis a,json_each(a.suggestions_json) j
                 WHERE a.account_id=?1 AND a.provider_id=?2",
                params![account_id, provider_id],
            )?
        } else {
            0
        };
        transaction.commit()?;
        Ok(projected)
    }

    pub fn recent_messages(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<RecentMessage>, DatabaseError> {
        let limit = limit.clamp(1, 20) as i64;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT m.provider_id,m.subject,COALESCE(m.received_at,m.sent_at),a.provider_id IS NOT NULL,EXISTS(SELECT 1 FROM reply_drafts d WHERE d.account_id=m.account_id AND d.provider_message_id=m.provider_id)
             FROM message_metadata m LEFT JOIN message_analysis a ON a.account_id=m.account_id AND a.provider_id=m.provider_id
             WHERE m.account_id=?1 AND m.deleted_at IS NULL
             ORDER BY COALESCE(m.received_at,m.sent_at,m.updated_at) DESC LIMIT ?2",
        )?;
        let messages = statement
            .query_map(params![account_id, limit], |row| {
                Ok(RecentMessage {
                    provider_id: row.get(0)?,
                    subject: row.get(1)?,
                    occurred_at: row.get(2)?,
                    analyzed: row.get(3)?,
                    has_reply_draft: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::from)?;
        Ok(messages)
    }

    pub fn review_items(&self, limit: usize) -> Result<Vec<ReviewItem>, DatabaseError> {
        let limit = limit.clamp(1, 100) as i64;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT a.account_id,a.provider_id,m.subject,m.sender_name,
                    COALESCE(m.received_at,m.sent_at),a.summary,a.classification,
                    a.classification_confidence,a.urgency,a.suggestions_json,
                    a.review_reasons_json,a.analyzed_at
             FROM message_analysis a
             JOIN message_metadata m ON m.account_id=a.account_id AND m.provider_id=a.provider_id
             LEFT JOIN review_decisions d ON d.account_id=a.account_id AND d.provider_id=a.provider_id
             WHERE a.disposition IN ('review','suggestions') AND m.deleted_at IS NULL AND d.provider_id IS NULL
                   AND NOT EXISTS(SELECT 1 FROM local_items i WHERE i.account_id=a.account_id AND i.provider_id=a.provider_id)
             ORDER BY a.analyzed_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, f64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        })?;
        rows.map(|row| {
            let (
                account_id,
                provider_id,
                subject,
                sender_name,
                occurred_at,
                summary,
                classification,
                classification_confidence,
                urgency,
                suggestions_json,
                review_reasons_json,
                analyzed_at,
            ) = row?;
            let mut review_reasons: Vec<String> = serde_json::from_str(&review_reasons_json)
                .map_err(|_| DatabaseError::InvalidAnalysis)?;
            if review_reasons.is_empty() {
                review_reasons.push("automationPolicyRequiresDecision".to_owned());
            }
            Ok(ReviewItem {
                account_id,
                provider_id,
                subject,
                sender_name,
                occurred_at,
                summary,
                classification,
                classification_confidence,
                urgency,
                suggestions: serde_json::from_str(&suggestions_json)
                    .map_err(|_| DatabaseError::InvalidAnalysis)?,
                review_reasons,
                analyzed_at,
            })
        })
        .collect()
    }

    pub fn pending_review_count(&self) -> Result<i64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM message_analysis a
                 JOIN message_metadata m ON m.account_id=a.account_id AND m.provider_id=a.provider_id
                 LEFT JOIN review_decisions d ON d.account_id=a.account_id AND d.provider_id=a.provider_id
                 WHERE a.disposition IN ('review','suggestions') AND m.deleted_at IS NULL AND d.provider_id IS NULL
                       AND NOT EXISTS(SELECT 1 FROM local_items i WHERE i.account_id=a.account_id AND i.provider_id=a.provider_id)",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn decide_review(
        &self,
        account_id: &str,
        provider_id: &str,
        decision: &str,
    ) -> Result<ReviewDecision, DatabaseError> {
        if !matches!(decision, "accept" | "ignore") {
            return Err(DatabaseError::InvalidReviewDecision);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT decision,decided_at FROM review_decisions WHERE account_id=?1 AND provider_id=?2",
                params![account_id, provider_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_decision, decided_at)) = existing {
            if existing_decision != decision {
                return Err(DatabaseError::ReviewAlreadyDecided);
            }
            let subject = transaction.query_row(
                "SELECT subject FROM message_metadata WHERE account_id=?1 AND provider_id=?2",
                params![account_id, provider_id],
                |row| row.get(0),
            )?;
            return Ok(ReviewDecision {
                account_id: account_id.to_owned(),
                provider_id: provider_id.to_owned(),
                subject,
                decision: existing_decision,
                decided_at,
            });
        }
        let subject: Option<String> = transaction
            .query_row(
                "SELECT m.subject FROM message_analysis a JOIN message_metadata m ON m.account_id=a.account_id AND m.provider_id=a.provider_id WHERE a.account_id=?1 AND a.provider_id=?2 AND a.disposition IN ('review','suggestions') AND m.deleted_at IS NULL",
                params![account_id, provider_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::MessageMissing)?;
        transaction.execute(
            "INSERT INTO review_decisions(account_id,provider_id,decision) VALUES(?1,?2,?3)",
            params![account_id, provider_id, decision],
        )?;
        if decision == "accept" {
            transaction.execute(
                "INSERT OR IGNORE INTO local_items(account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,expected_from,follow_up_at,category)
                 SELECT a.account_id,a.provider_id,CAST(j.key AS INTEGER),json_extract(j.value,'$.kind'),
                        COALESCE(json_extract(j.value,'$.title'),json_extract(j.value,'$.description')),
                        json_extract(j.value,'$.due_at'),json_extract(j.value,'$.start_at'),json_extract(j.value,'$.end_at'),
                        json_extract(j.value,'$.expected_from'),json_extract(j.value,'$.follow_up_at'),a.classification
                 FROM message_analysis a,json_each(a.suggestions_json) j
                 WHERE a.account_id=?1 AND a.provider_id=?2",
                params![account_id, provider_id],
            )?;
        }
        let decided_at = transaction.query_row(
            "SELECT decided_at FROM review_decisions WHERE account_id=?1 AND provider_id=?2",
            params![account_id, provider_id],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(ReviewDecision {
            account_id: account_id.to_owned(),
            provider_id: provider_id.to_owned(),
            subject,
            decision: decision.to_owned(),
            decided_at,
        })
    }

    pub fn review_history(&self, limit: usize) -> Result<Vec<ReviewDecision>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT d.account_id,d.provider_id,m.subject,d.decision,d.decided_at
             FROM review_decisions d JOIN message_metadata m ON m.account_id=d.account_id AND m.provider_id=d.provider_id
             ORDER BY d.decided_at DESC LIMIT ?1",
        )?;
        let decisions = statement
            .query_map([limit.clamp(1, 100) as i64], |row| {
                Ok(ReviewDecision {
                    account_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    subject: row.get(2)?,
                    decision: row.get(3)?,
                    decided_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::from)?;
        Ok(decisions)
    }

    pub fn queue_review_action_proposals(
        &self,
        account_id: &str,
        provider_id: &str,
    ) -> Result<Vec<InertActionProposal>, DatabaseError> {
        let policy = self.settings()?.automation_policy;
        let candidates = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            let mut statement = connection.prepare(
                "SELECT i.id,i.kind,i.title,i.category,i.created_at,a.classification_confidence,a.urgency,
                        COALESCE(json_extract(a.suggestions_json,'$[' || i.suggestion_index || '].confirmed'),0),
                        COALESCE(i.scheduled_at,i.due_at,i.start_at,i.follow_up_at),m.deleted_at IS NULL
                 FROM local_items i
                 JOIN review_decisions d ON d.account_id=i.account_id AND d.provider_id=i.provider_id
                 JOIN message_analysis a ON a.account_id=i.account_id AND a.provider_id=i.provider_id
                 JOIN message_metadata m ON m.account_id=i.account_id AND m.provider_id=i.provider_id
                 WHERE i.account_id=?1 AND i.provider_id=?2 AND d.decision='accept' AND i.status='active'
                 ORDER BY i.id",
            )?;
            let values = statement
                .query_map(params![account_id, provider_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, bool>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, bool>(9)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            values
        };
        let mut proposals = Vec::new();
        for (
            id,
            kind,
            title,
            category,
            created_at,
            confidence,
            urgency,
            fact_confirmed,
            effective_at,
            source_verified,
        ) in candidates
        {
            let action = match kind.as_str() {
                "appointment" => rules::AutomationAction::CalendarCreate,
                "task" | "waiting_for" if effective_at.is_some() => {
                    rules::AutomationAction::Notification
                }
                _ => continue,
            };
            let sensitivity = if urgency == "critical" {
                rules::ActionSensitivity::Sensitive
            } else if urgency == "high" || category == "work" {
                rules::ActionSensitivity::Important
            } else {
                rules::ActionSensitivity::Routine
            };
            let created_at =
                chrono::NaiveDateTime::parse_from_str(&created_at, "%Y-%m-%d %H:%M:%S")
                    .map_err(|_| DatabaseError::InvalidActionProposal)?
                    .and_utc();
            let expires_at = created_at + chrono::Duration::days(30);
            if expires_at <= chrono::Utc::now() {
                continue;
            }
            let proposal_key = format!("local-proposal-{id:016}");
            let audit_key = format!("local-proposal-audit-{id:016}");
            let target_ref = format!("local-item:{id}");
            let prefix = if action == rules::AutomationAction::CalendarCreate {
                "Calendar: "
            } else {
                "Reminder: "
            };
            let display_label = bounded_display_label(prefix, &title);
            if let Some(proposal) = self.queue_action_proposal(QueueActionProposal {
                proposal_key: &proposal_key,
                audit_event_key: &audit_key,
                display_label: &display_label,
                target_ref: Some(&target_ref),
                expires_at: &expires_at.to_rfc3339(),
                policy,
                context: rules::AutomationContext {
                    action,
                    sensitivity,
                    confidence: confidence as f32,
                    source_verified,
                    fact_confirmed,
                    reversible: true,
                    user_confirmed: true,
                },
            })? {
                proposals.push(proposal);
            }
        }
        Ok(proposals)
    }

    pub fn local_items(&self, include_undone: bool) -> Result<Vec<LocalItem>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,
                    expected_from,follow_up_at,category,status,lifecycle_state,scheduled_at,created_at,undone_at
             FROM local_items WHERE status='active' OR ?1 ORDER BY created_at DESC,id DESC",
        )?;
        let items = statement
            .query_map([include_undone], local_item_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn focused_local_items(
        &self,
        view: &str,
        day_start: &str,
        day_end: &str,
    ) -> Result<Vec<LocalItem>, DatabaseError> {
        if !matches!(view, "today" | "work" | "kids" | "personal") {
            return Err(DatabaseError::InvalidFocusView);
        }
        let start = chrono::DateTime::parse_from_rfc3339(day_start)
            .map_err(|_| DatabaseError::InvalidFocusView)?;
        let end = chrono::DateTime::parse_from_rfc3339(day_end)
            .map_err(|_| DatabaseError::InvalidFocusView)?;
        let duration = end.signed_duration_since(start);
        if duration <= chrono::Duration::zero() || duration > chrono::Duration::hours(26) {
            return Err(DatabaseError::InvalidFocusView);
        }
        Ok(self
            .local_items(false)?
            .into_iter()
            .filter(|item| item.lifecycle_state != "completed")
            .filter(|item| match view {
                "work" => item.category == "work",
                "kids" => matches!(item.category.as_str(), "school" | "creche" | "kids"),
                "personal" => matches!(item.category.as_str(), "personal" | "home"),
                "today" => effective_item_timestamp(item)
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .is_some_and(|value| value >= start && value < end),
                _ => false,
            })
            .collect())
    }

    pub fn local_item_provenance(&self, id: i64) -> Result<LocalItemProvenance, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT i.id,c.provider,m.subject,COALESCE(m.sender_name,m.sender_address),
                        COALESCE(m.received_at,m.sent_at),m.deleted_at IS NULL AND m.provider_web_link IS NOT NULL
                 FROM local_items i
                 JOIN connected_accounts c ON c.id=i.account_id
                 JOIN message_metadata m ON m.account_id=i.account_id AND m.provider_id=i.provider_id
                 WHERE i.id=?1",
                [id],
                |row| {
                    Ok(LocalItemProvenance {
                        item_id: row.get(0)?,
                        provider: row.get(1)?,
                        source_subject: row.get(2)?,
                        source_sender: row.get(3)?,
                        source_occurred_at: row.get(4)?,
                        source_available: row.get(5)?,
                    })
                },
            )
            .optional()?
            .ok_or(DatabaseError::LocalItemMissing)
    }

    pub fn local_item_source_link(&self, id: i64) -> Result<String, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT m.provider_web_link FROM local_items i
                 JOIN message_metadata m ON m.account_id=i.account_id AND m.provider_id=i.provider_id
                 WHERE i.id=?1 AND m.deleted_at IS NULL AND m.provider_web_link IS NOT NULL",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::SourceLinkUnavailable)
    }

    pub fn undo_local_item(&self, id: i64) -> Result<LocalItem, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let changed = connection.execute(
            "UPDATE local_items SET status='undone',undone_at=COALESCE(undone_at,CURRENT_TIMESTAMP) WHERE id=?1 AND status='active'",
            [id],
        )?;
        if changed == 0 {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM local_items WHERE id=?1)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(DatabaseError::LocalItemMissing);
            }
        }
        connection
            .query_row(
                "SELECT id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,
                        expected_from,follow_up_at,category,status,lifecycle_state,scheduled_at,created_at,undone_at
                 FROM local_items WHERE id=?1",
                [id],
                local_item_from_row,
            )
            .map_err(Into::into)
    }

    pub fn transition_local_item(
        &self,
        id: i64,
        event_key: &str,
        event_type: &str,
        scheduled_at: Option<&str>,
    ) -> Result<LocalItem, DatabaseError> {
        if !(16..=100).contains(&event_key.len())
            || !event_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !matches!(event_type, "complete" | "snooze" | "reschedule")
        {
            return Err(DatabaseError::InvalidLocalTransition);
        }
        let normalized_schedule = match event_type {
            "complete" => None,
            _ => Some(
                chrono::DateTime::parse_from_rfc3339(
                    scheduled_at.ok_or(DatabaseError::InvalidLocalTransition)?,
                )
                .map_err(|_| DatabaseError::InvalidLocalTransition)?
                .to_rfc3339(),
            ),
        };
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let existing_item: Option<i64> = transaction
            .query_row(
                "SELECT item_id FROM local_item_events WHERE event_key=?1",
                [event_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_item) = existing_item {
            if existing_item != id {
                return Err(DatabaseError::InvalidLocalTransition);
            }
            return transaction
                .query_row(
                    "SELECT id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,
                            expected_from,follow_up_at,category,status,lifecycle_state,scheduled_at,created_at,undone_at
                     FROM local_items WHERE id=?1",
                    [id],
                    local_item_from_row,
                )
                .map_err(Into::into);
        }
        let (projection_status, previous_state, previous_schedule): (
            String,
            String,
            Option<String>,
        ) = transaction
            .query_row(
                "SELECT status,lifecycle_state,scheduled_at FROM local_items WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(DatabaseError::LocalItemMissing)?;
        if projection_status != "active" {
            return Err(DatabaseError::InvalidLocalTransition);
        }
        let new_state = match event_type {
            "complete" => "completed",
            "snooze" => "snoozed",
            "reschedule" => "active",
            _ => unreachable!(),
        };
        transaction.execute(
            "INSERT INTO local_item_events(event_key,item_id,event_type,previous_state,new_state,previous_scheduled_at,new_scheduled_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![event_key,id,event_type,previous_state,new_state,previous_schedule,normalized_schedule],
        )?;
        transaction.execute(
            "UPDATE local_items SET lifecycle_state=?2,scheduled_at=?3 WHERE id=?1",
            params![id, new_state, normalized_schedule],
        )?;
        transaction.commit()?;
        drop(connection);
        self.local_item(id)
    }

    pub fn local_item_events(&self, limit: usize) -> Result<Vec<LocalItemEvent>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id,item_id,event_type,previous_state,new_state,previous_scheduled_at,new_scheduled_at,created_at
             FROM local_item_events ORDER BY id DESC LIMIT ?1",
        )?;
        let events = statement
            .query_map([limit.clamp(1, 100) as i64], |row| {
                Ok(LocalItemEvent {
                    id: row.get(0)?,
                    item_id: row.get(1)?,
                    event_type: row.get(2)?,
                    previous_state: row.get(3)?,
                    new_state: row.get(4)?,
                    previous_scheduled_at: row.get(5)?,
                    new_scheduled_at: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }

    fn local_item(&self, id: i64) -> Result<LocalItem, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT id,account_id,provider_id,suggestion_index,kind,title,due_at,start_at,end_at,
                        expected_from,follow_up_at,category,status,lifecycle_state,scheduled_at,created_at,undone_at
                 FROM local_items WHERE id=?1",
                [id],
                local_item_from_row,
            )
            .optional()?
            .ok_or(DatabaseError::LocalItemMissing)
    }

    pub fn message_available(
        &self,
        account_id: &str,
        provider_id: &str,
    ) -> Result<bool, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM message_metadata WHERE account_id=?1 AND provider_id=?2 AND deleted_at IS NULL)",
                params![account_id, provider_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn message_analyzed(
        &self,
        account_id: &str,
        provider_id: &str,
    ) -> Result<bool, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM message_analysis WHERE account_id=?1 AND provider_id=?2)",
                params![account_id, provider_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn upsert_account(&self, account: &ConnectedAccount) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO connected_accounts(id, provider, display_name, email_address, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, email_address=excluded.email_address,
               tenant_id=excluded.tenant_id, enabled=1, updated_at=CURRENT_TIMESTAMP",
            params![account.id, account.provider, account.display_name, account.email_address, account.tenant_id],
        )?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<ConnectedAccount>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare("SELECT id, provider, display_name, email_address, tenant_id FROM connected_accounts WHERE enabled=1 ORDER BY created_at")?;
        let values = statement
            .query_map([], |row| {
                Ok(ConnectedAccount {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    display_name: row.get(2)?,
                    email_address: row.get(3)?,
                    tenant_id: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn save_sync_cursor(
        &self,
        account_id: &str,
        resource: &str,
        cursor_url: &str,
    ) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO sync_cursors(account_id, resource, cursor_url, last_success_at) VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(account_id, resource) DO UPDATE SET cursor_url=excluded.cursor_url, last_success_at=CURRENT_TIMESTAMP, last_error_code=NULL",
            params![account_id, resource, cursor_url],
        )?;
        Ok(())
    }

    pub fn sync_cursor(
        &self,
        account_id: &str,
        resource: &str,
    ) -> Result<Option<String>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT cursor_url FROM sync_cursors WHERE account_id=?1 AND resource=?2",
                params![account_id, resource],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn apply_message_page(
        &self,
        account_id: &str,
        resource: &str,
        messages: &[MessageMetadata],
        final_cursor: Option<&str>,
    ) -> Result<(), DatabaseError> {
        if !matches!(resource, "mail_inbox" | "mail_sent") {
            return Err(DatabaseError::InvalidResource);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        for message in messages {
            if message.removed.is_some() {
                transaction.execute("UPDATE message_metadata SET deleted_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP WHERE account_id=?1 AND provider_id=?2", params![account_id, message.id])?;
                continue;
            }
            let (sender_name, sender_address) = message
                .sender
                .as_ref()
                .map(|sender| {
                    (
                        sender.email_address.name.as_deref(),
                        sender.email_address.address.as_deref(),
                    )
                })
                .unwrap_or((None, None));
            transaction.execute(
                "INSERT INTO message_metadata(account_id, provider_id, conversation_id, sender_name, sender_address, subject, received_at, sent_at, provider_web_link, is_read, deleted_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL)
                 ON CONFLICT(account_id,provider_id) DO UPDATE SET conversation_id=excluded.conversation_id,sender_name=excluded.sender_name,sender_address=excluded.sender_address,subject=excluded.subject,received_at=excluded.received_at,sent_at=excluded.sent_at,provider_web_link=excluded.provider_web_link,is_read=excluded.is_read,deleted_at=NULL,updated_at=CURRENT_TIMESTAMP",
                params![account_id,message.id,message.conversation_id,sender_name,sender_address,message.subject,message.received_date_time,message.sent_date_time,message.web_link,message.is_read],
            )?;
        }
        if let Some(cursor) = final_cursor {
            transaction.execute("INSERT INTO sync_cursors(account_id,resource,cursor_url,last_success_at) VALUES (?1,?2,?3,CURRENT_TIMESTAMP) ON CONFLICT(account_id,resource) DO UPDATE SET cursor_url=excluded.cursor_url,last_success_at=CURRENT_TIMESTAMP,last_error_code=NULL", params![account_id,resource,cursor])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn apply_calendar_page(
        &self,
        account_id: &str,
        events: &[CalendarEvent],
    ) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        for event in events {
            transaction.execute(
                "INSERT INTO calendar_events(account_id,provider_id,subject,start_at,start_timezone,end_at,end_timezone,provider_web_link,is_cancelled,last_modified_at,provider_etag)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(account_id,provider_id) DO UPDATE SET subject=excluded.subject,start_at=excluded.start_at,start_timezone=excluded.start_timezone,end_at=excluded.end_at,end_timezone=excluded.end_timezone,provider_web_link=excluded.provider_web_link,is_cancelled=excluded.is_cancelled,last_modified_at=excluded.last_modified_at,provider_etag=excluded.provider_etag,updated_at=CURRENT_TIMESTAMP",
                params![account_id,event.id,event.subject,event.start.date_time,event.start.time_zone,event.end.date_time,event.end.time_zone,event.web_link,event.is_cancelled,event.last_modified_date_time,event.etag],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn calendar_update_candidates(
        &self,
        account_id: &str,
    ) -> Result<Vec<CalendarUpdateCandidate>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT account_id,provider_id,COALESCE(subject,'Untitled event'),start_at,start_timezone,end_at,end_timezone
             FROM calendar_events
             WHERE account_id=?1 AND is_cancelled=0 AND provider_etag IS NOT NULL
             ORDER BY start_at LIMIT 100",
        )?;
        let candidates = statement
            .query_map([account_id], |row| {
                Ok(CalendarUpdateCandidate {
                    account_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    subject: row.get(2)?,
                    start_at: row.get(3)?,
                    start_timezone: row.get(4)?,
                    end_at: row.get(5)?,
                    end_timezone: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::from)?;
        Ok(candidates)
    }

    pub fn queue_calendar_update_proposal(
        &self,
        proposal_key: &str,
        audit_event_key: &str,
        account_id: &str,
        provider_event_id: &str,
        proposed_start_at: &str,
        proposed_end_at: &str,
    ) -> Result<InertActionProposal, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(audit_event_key)?;
        let (subject, previous_start, previous_end, etag) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            connection
                .query_row(
                    "SELECT COALESCE(subject,'Untitled event'),start_at,end_at,provider_etag FROM calendar_events WHERE account_id=?1 AND provider_id=?2 AND is_cancelled=0 AND provider_etag IS NOT NULL",
                    params![account_id, provider_event_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
                )
                .optional()?
                .ok_or(DatabaseError::InvalidCalendarExecution)?
        };
        let proposed_start = chrono::DateTime::parse_from_rfc3339(proposed_start_at)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?;
        let proposed_end = chrono::DateTime::parse_from_rfc3339(proposed_end_at)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?;
        if proposed_end <= proposed_start
            || proposed_end - proposed_start > chrono::Duration::days(7)
            || provider_event_id.is_empty()
            || provider_event_id.len() > 512
            || etag.is_empty()
            || etag.len() > 512
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let target_ref = format!("calendar-update:{proposal_key}");
        if target_ref.len() > 100 {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let existing_expiry = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            connection
                .query_row(
                    "SELECT expires_at FROM action_proposals WHERE proposal_key=?1",
                    [proposal_key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        };
        let expires = existing_expiry
            .unwrap_or_else(|| (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339());
        let display = bounded_display_label("Update calendar: ", &subject);
        let proposal = self
            .queue_action_proposal(QueueActionProposal {
                proposal_key,
                audit_event_key,
                display_label: &display,
                target_ref: Some(&target_ref),
                expires_at: &expires,
                policy: self.settings()?.automation_policy,
                context: rules::AutomationContext {
                    action: rules::AutomationAction::CalendarUpdate,
                    sensitivity: rules::ActionSensitivity::Important,
                    confidence: 1.0,
                    source_verified: true,
                    fact_confirmed: true,
                    reversible: true,
                    user_confirmed: false,
                },
            })?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT OR IGNORE INTO calendar_update_targets(proposal_id,account_id,provider_event_id,provider_etag,previous_start_at,previous_end_at,proposed_start_at,proposed_end_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![proposal.id,account_id,provider_event_id,etag,previous_start,previous_end,proposed_start.to_rfc3339(),proposed_end.to_rfc3339()],
        )?;
        let stored: (String, String, String, String, String, String, String) = connection.query_row(
            "SELECT account_id,provider_event_id,provider_etag,previous_start_at,previous_end_at,proposed_start_at,proposed_end_at FROM calendar_update_targets WHERE proposal_id=?1",
            [proposal.id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
        )?;
        if stored
            != (
                account_id.to_owned(),
                provider_event_id.to_owned(),
                etag,
                previous_start,
                previous_end,
                proposed_start.to_rfc3339(),
                proposed_end.to_rfc3339(),
            )
        {
            return Err(DatabaseError::CalendarExecutionConflict);
        }
        Ok(proposal)
    }

    pub fn record_reply_draft_metadata(
        &self,
        draft_key: &str,
        revision_key: &str,
        account_id: &str,
        provider_message_id: &str,
        content_sha256: &str,
        content_bytes: i64,
    ) -> Result<ReplyDraftMetadata, DatabaseError> {
        validate_proposal_identifier(draft_key)?;
        validate_proposal_identifier(revision_key)?;
        if provider_message_id.is_empty()
            || provider_message_id.len() > 512
            || provider_message_id.chars().any(char::is_control)
            || content_bytes <= 0
            || content_bytes > 16 * 1024
            || content_sha256.len() != 64
            || !content_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(DatabaseError::InvalidReplyDraft);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let source_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM message_metadata WHERE account_id=?1 AND provider_id=?2 AND deleted_at IS NULL)",
            params![account_id, provider_message_id],
            |row| row.get(0),
        )?;
        if !source_exists {
            return Err(DatabaseError::InvalidReplyDraft);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO reply_drafts(draft_key,account_id,provider_message_id) VALUES (?1,?2,?3)",
            params![draft_key, account_id, provider_message_id],
        )?;
        let (draft_id, stored_draft_key, stored_account, stored_provider):
            (i64, String, String, String) = transaction
            .query_row(
                "SELECT id,draft_key,account_id,provider_message_id FROM reply_drafts WHERE draft_key=?1 OR (account_id=?2 AND provider_message_id=?3) ORDER BY CASE WHEN draft_key=?1 THEN 0 ELSE 1 END LIMIT 1",
                params![draft_key, account_id, provider_message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidReplyDraft)?;
        if stored_account != account_id || stored_provider != provider_message_id {
            return Err(DatabaseError::ReplyDraftConflict);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO reply_draft_revisions(revision_key,draft_id,content_sha256,content_bytes) VALUES (?1,?2,?3,?4)",
            params![revision_key, draft_id, content_sha256.to_ascii_lowercase(), content_bytes],
        )?;
        let metadata = transaction.query_row(
            "SELECT d.draft_key,r.revision_key,d.account_id,d.provider_message_id,r.content_sha256,r.content_bytes,r.created_at FROM reply_draft_revisions r JOIN reply_drafts d ON d.id=r.draft_id WHERE r.revision_key=?1",
            [revision_key],
            |row| Ok(ReplyDraftMetadata { draft_key: row.get(0)?, revision_key: row.get(1)?, account_id: row.get(2)?, provider_message_id: row.get(3)?, content_sha256: row.get(4)?, content_bytes: row.get(5)?, created_at: row.get(6)? }),
        )?;
        if metadata.draft_key != stored_draft_key
            || metadata.account_id != account_id
            || metadata.provider_message_id != provider_message_id
            || metadata.content_sha256 != content_sha256.to_ascii_lowercase()
            || metadata.content_bytes != content_bytes
        {
            return Err(DatabaseError::ReplyDraftConflict);
        }
        transaction.commit()?;
        Ok(metadata)
    }

    pub fn reply_draft_revision_keys(
        &self,
        account_id: &str,
    ) -> Result<Vec<String>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT r.revision_key FROM reply_draft_revisions r JOIN reply_drafts d ON d.id=r.draft_id WHERE d.account_id=?1 ORDER BY r.id",
        )?;
        let keys = statement
            .query_map([account_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::from)?;
        Ok(keys)
    }

    pub fn reply_draft_metadata(
        &self,
        account_id: &str,
    ) -> Result<Vec<ReplyDraftMetadata>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT d.draft_key,r.revision_key,d.account_id,d.provider_message_id,r.content_sha256,r.content_bytes,r.created_at FROM reply_draft_revisions r JOIN reply_drafts d ON d.id=r.draft_id WHERE d.account_id=?1 AND r.id=(SELECT MAX(latest.id) FROM reply_draft_revisions latest WHERE latest.draft_id=d.id) ORDER BY r.id",
        )?;
        let values = statement
            .query_map([account_id], |row| {
                Ok(ReplyDraftMetadata {
                    draft_key: row.get(0)?,
                    revision_key: row.get(1)?,
                    account_id: row.get(2)?,
                    provider_message_id: row.get(3)?,
                    content_sha256: row.get(4)?,
                    content_bytes: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn queue_reply_draft_proposal(
        &self,
        proposal_key: &str,
        audit_event_key: &str,
        draft_key: &str,
        revision_key: &str,
    ) -> Result<InertActionProposal, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(audit_event_key)?;
        validate_proposal_identifier(draft_key)?;
        validate_proposal_identifier(revision_key)?;
        let (content_sha256, bytes) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            connection
                .query_row(
                    "SELECT r.content_sha256,r.content_bytes FROM reply_draft_revisions r JOIN reply_drafts d ON d.id=r.draft_id WHERE d.draft_key=?1 AND r.revision_key=?2 AND r.id=(SELECT MAX(latest.id) FROM reply_draft_revisions latest WHERE latest.draft_id=d.id)",
                    params![draft_key, revision_key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?
                .ok_or(DatabaseError::InvalidReplyDraft)?
        };
        let target_ref = format!("reply-draft:{draft_key}");
        if target_ref.len() > 100 {
            return Err(DatabaseError::InvalidReplyDraft);
        }
        let existing_expiry = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            connection
                .query_row(
                    "SELECT expires_at FROM action_proposals WHERE proposal_key=?1",
                    [proposal_key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        };
        let expires = existing_expiry
            .unwrap_or_else(|| (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339());
        let display = format!("Private reply draft · {bytes} bytes");
        let proposal = self
            .queue_action_proposal(QueueActionProposal {
                proposal_key,
                audit_event_key,
                display_label: &display,
                target_ref: Some(&target_ref),
                expires_at: &expires,
                policy: self.settings()?.automation_policy,
                context: rules::AutomationContext {
                    action: rules::AutomationAction::Correspondence,
                    sensitivity: rules::ActionSensitivity::Sensitive,
                    confidence: 1.0,
                    source_verified: true,
                    fact_confirmed: true,
                    reversible: false,
                    user_confirmed: false,
                },
            })?
            .ok_or(DatabaseError::InvalidReplyDraft)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT OR IGNORE INTO correspondence_targets(proposal_id,revision_key,content_sha256) VALUES (?1,?2,?3)",
            params![proposal.id, revision_key, content_sha256],
        )?;
        let stored: (String, String) = connection.query_row(
            "SELECT revision_key,content_sha256 FROM correspondence_targets WHERE proposal_id=?1",
            [proposal.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored != (revision_key.to_owned(), content_sha256) {
            return Err(DatabaseError::ReplyDraftConflict);
        }
        Ok(proposal)
    }

    pub fn prepare_correspondence_execution(
        &self,
        proposal_key: &str,
        execution_key: &str,
    ) -> Result<CorrespondenceExecutionPreparation, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(execution_key)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let target: (i64, String, String, String, String, i64, String) = transaction
            .query_row(
                "SELECT p.id,d.account_id,d.provider_message_id,r.revision_key,r.content_sha256,r.content_bytes,p.expires_at FROM action_proposals p JOIN action_proposal_events e ON e.proposal_id=p.id AND e.event_type='confirm' JOIN correspondence_targets c ON c.proposal_id=p.id JOIN reply_draft_revisions r ON r.revision_key=c.revision_key AND r.content_sha256=c.content_sha256 JOIN reply_drafts d ON d.id=r.draft_id JOIN message_metadata m ON m.account_id=d.account_id AND m.provider_id=d.provider_message_id AND m.deleted_at IS NULL WHERE p.proposal_key=?1 AND p.action_kind='correspondence' AND r.id=(SELECT MAX(latest.id) FROM reply_draft_revisions latest WHERE latest.draft_id=d.id)",
                [proposal_key],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCorrespondenceExecution)?;
        if chrono::DateTime::parse_from_rfc3339(&target.6)
            .map_err(|_| DatabaseError::InvalidCorrespondenceExecution)?
            <= chrono::Utc::now()
        {
            return Err(DatabaseError::InvalidCorrespondenceExecution);
        }
        let existing: Option<(String, String, String, Option<i64>)> = transaction
            .query_row(
                "SELECT x.execution_key,x.account_id,x.revision_key,result.id FROM correspondence_executions x LEFT JOIN correspondence_execution_results result ON result.execution_id=x.id WHERE x.proposal_id=?1",
                [target.0],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            )
            .optional()?;
        let effective_key = if let Some(existing) = existing {
            if existing.1 != target.1 || existing.2 != target.3 || existing.3.is_some() {
                return Err(DatabaseError::CorrespondenceExecutionConflict);
            }
            existing.0
        } else {
            let key_in_use: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM correspondence_executions WHERE execution_key=?1)",
                [execution_key],
                |row| row.get(0),
            )?;
            if key_in_use {
                return Err(DatabaseError::CorrespondenceExecutionConflict);
            }
            transaction.execute(
                "INSERT INTO correspondence_executions(execution_key,proposal_id,account_id,revision_key) VALUES (?1,?2,?3,?4)",
                params![execution_key,target.0,target.1,target.3],
            )?;
            execution_key.to_owned()
        };
        transaction.commit()?;
        Ok(CorrespondenceExecutionPreparation {
            execution_key: effective_key,
            proposal_key: proposal_key.to_owned(),
            account_id: target.1,
            provider_message_id: target.2,
            revision_key: target.3,
            content_sha256: target.4,
            content_bytes: target.5,
        })
    }

    pub fn record_correspondence_execution_result(
        &self,
        execution_key: &str,
        event_key: &str,
        outcome: &str,
        reason_code: &str,
    ) -> Result<(), DatabaseError> {
        validate_proposal_identifier(execution_key)?;
        validate_proposal_identifier(event_key)?;
        if !matches!(outcome, "succeeded" | "failed" | "unknown")
            || !(2..=80).contains(&reason_code.len())
            || !reason_code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(DatabaseError::InvalidCorrespondenceExecution);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let execution_id: i64 = connection
            .query_row(
                "SELECT id FROM correspondence_executions WHERE execution_key=?1",
                [execution_key],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCorrespondenceExecution)?;
        let existing: Option<(String, String, String)> = connection
            .query_row(
                "SELECT event_key,outcome,reason_code FROM correspondence_execution_results WHERE execution_id=?1 OR event_key=?2",
                params![execution_id,event_key],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
            )
            .optional()?;
        let expected = (
            event_key.to_owned(),
            outcome.to_owned(),
            reason_code.to_owned(),
        );
        if let Some(existing) = existing {
            return if existing == expected {
                Ok(())
            } else {
                Err(DatabaseError::CorrespondenceExecutionConflict)
            };
        }
        connection.execute(
            "INSERT INTO correspondence_execution_results(event_key,execution_id,outcome,reason_code) VALUES (?1,?2,?3,?4)",
            params![event_key,execution_id,outcome,reason_code],
        )?;
        Ok(())
    }

    pub fn correspondence_execution_history(
        &self,
        limit: usize,
    ) -> Result<Vec<CorrespondenceExecutionHistory>, DatabaseError> {
        let limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT p.display_label,r.outcome,r.reason_code,r.created_at FROM correspondence_execution_results r JOIN correspondence_executions x ON x.id=r.execution_id JOIN action_proposals p ON p.id=x.proposal_id ORDER BY r.id DESC LIMIT ?1",
        )?;
        let values = statement
            .query_map([limit], |row| {
                Ok(CorrespondenceExecutionHistory {
                    display_label: row.get(0)?,
                    outcome: row.get(1)?,
                    reason_code: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn automation_audit_history(
        &self,
        limit: usize,
    ) -> Result<Vec<AutomationAuditEntry>, DatabaseError> {
        let limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT policy,decision,action_kind,reason_code,created_at FROM automation_audit ORDER BY id DESC LIMIT ?1",
        )?;
        let values = statement
            .query_map([limit], |row| {
                Ok(AutomationAuditEntry {
                    policy: row.get(0)?,
                    decision: row.get(1)?,
                    action_kind: row.get(2)?,
                    reason_code: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn delete_account_data(&self, account_id: &str) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute("DELETE FROM connected_accounts WHERE id=?1", [account_id])?;
        Ok(())
    }

    pub fn disable_account(&self, account_id: &str) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "UPDATE connected_accounts SET enabled=0,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            [account_id],
        )?;
        Ok(())
    }

    pub fn begin_sync_run(&self, account_id: &str) -> Result<i64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO sync_runs(account_id,outcome) VALUES (?1,'running')",
            [account_id],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn finish_sync_run(
        &self,
        run_id: i64,
        outcome: &str,
        counts: (usize, usize, usize),
        error_code: Option<&str>,
    ) -> Result<(), DatabaseError> {
        if !matches!(outcome, "success" | "cancelled" | "error") {
            return Err(DatabaseError::InvalidResource);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute("UPDATE sync_runs SET finished_at=CURRENT_TIMESTAMP,outcome=?2,inbox_count=?3,sent_count=?4,calendar_count=?5,error_code=?6 WHERE id=?1", params![run_id,outcome,counts.0 as i64,counts.1 as i64,counts.2 as i64,error_code])?;
        Ok(())
    }

    pub fn last_sync_run(&self, account_id: &str) -> Result<Option<SyncRunSummary>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.query_row(
            "SELECT outcome,finished_at,inbox_count,sent_count,calendar_count FROM sync_runs WHERE account_id=?1 AND outcome!='running' ORDER BY id DESC LIMIT 1",
            [account_id],
            |row| Ok(SyncRunSummary { outcome: row.get(0)?, finished_at: row.get(1)?, inbox_count: row.get(2)?, sent_count: row.get(3)?, calendar_count: row.get(4)? })
        ).optional().map_err(Into::into)
    }

    pub fn clear_synced_data(&self, account_id: &str) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM reply_draft_revisions WHERE draft_id IN (SELECT id FROM reply_drafts WHERE account_id=?1)",
            [account_id],
        )?;
        transaction.execute("DELETE FROM reply_drafts WHERE account_id=?1", [account_id])?;
        transaction.execute(
            "DELETE FROM message_metadata WHERE account_id=?1",
            [account_id],
        )?;
        transaction.execute(
            "DELETE FROM calendar_events WHERE account_id=?1",
            [account_id],
        )?;
        transaction.execute("DELETE FROM sync_cursors WHERE account_id=?1", [account_id])?;
        transaction.execute("DELETE FROM sync_runs WHERE account_id=?1", [account_id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn apply_retention(&self) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM message_metadata WHERE deleted_at IS NOT NULL AND deleted_at < datetime('now','-30 days')", [])?;
        transaction.execute("DELETE FROM sync_runs WHERE finished_at IS NOT NULL AND finished_at < datetime('now','-90 days')", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn queue_action_proposal(
        &self,
        input: QueueActionProposal<'_>,
    ) -> Result<Option<InertActionProposal>, DatabaseError> {
        validate_proposal_identifier(input.proposal_key)?;
        validate_proposal_identifier(input.audit_event_key)?;
        validate_display_label(input.display_label)?;
        if let Some(target_ref) = input.target_ref {
            if target_ref.is_empty()
                || target_ref.len() > 100
                || !target_ref.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
                })
            {
                return Err(DatabaseError::InvalidActionProposal);
            }
        }
        let expires_at = chrono::DateTime::parse_from_rfc3339(input.expires_at)
            .map_err(|_| DatabaseError::InvalidActionProposal)?;
        let now = chrono::Utc::now();
        if expires_at <= now || expires_at > now + chrono::Duration::days(30) {
            return Err(DatabaseError::InvalidActionProposal);
        }
        let decision = rules::decide_automation(input.policy, input.context);
        let action_kind = enum_name(&input.context.action)?;
        let sensitivity = enum_name(&input.context.sensitivity)?;
        let policy = input.policy.as_str();
        let disposition = enum_name(&decision.disposition)?;
        let reason = enum_name(&decision.reason)?;
        let audit_decision = if decision.disposition == rules::AutomationDisposition::Blocked {
            "blocked"
        } else {
            "review"
        };
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let existing_audit: Option<(String, String, String, Option<String>, String)> = transaction
            .query_row(
                "SELECT policy,decision,action_kind,target_ref,reason_code FROM automation_audit WHERE event_key=?1",
                [input.audit_event_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        if let Some(existing) = existing_audit {
            if existing
                != (
                    policy.to_owned(),
                    audit_decision.to_owned(),
                    action_kind.clone(),
                    input.target_ref.map(str::to_owned),
                    reason.clone(),
                )
            {
                return Err(DatabaseError::ActionProposalConflict);
            }
        }
        let audited_proposal_key: Option<String> = transaction
            .query_row(
                "SELECT proposal_key FROM action_proposals WHERE audit_event_key=?1",
                [input.audit_event_key],
                |row| row.get(0),
            )
            .optional()?;
        if audited_proposal_key
            .as_deref()
            .is_some_and(|key| key != input.proposal_key)
        {
            return Err(DatabaseError::ActionProposalConflict);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO automation_audit(event_key,policy,decision,action_kind,target_ref,reason_code) VALUES (?1,?2,?3,?4,?5,?6)",
            params![input.audit_event_key, policy, audit_decision, action_kind, input.target_ref, reason],
        )?;
        if decision.disposition == rules::AutomationDisposition::Blocked {
            transaction.commit()?;
            return Ok(None);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO action_proposals(proposal_key,audit_event_key,action_kind,sensitivity,policy,disposition,reason_code,display_label,target_ref,confidence,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![input.proposal_key, input.audit_event_key, action_kind, sensitivity, policy, disposition, reason, input.display_label, input.target_ref, input.context.confidence as f64, expires_at.to_rfc3339()],
        )?;
        let proposal = transaction.query_row(
            "SELECT id,proposal_key,audit_event_key,action_kind,sensitivity,policy,disposition,reason_code,display_label,target_ref,confidence,expires_at,created_at,'pending' FROM action_proposals WHERE proposal_key=?1",
            [input.proposal_key],
            action_proposal_from_row,
        )?;
        if proposal.audit_event_key != input.audit_event_key
            || proposal.action_kind != action_kind
            || proposal.sensitivity != sensitivity
            || proposal.policy != policy
            || proposal.disposition != disposition
            || proposal.reason_code != reason
            || proposal.display_label != input.display_label
            || proposal.target_ref.as_deref() != input.target_ref
            || (proposal.confidence - input.context.confidence as f64).abs() > f64::EPSILON
            || proposal.expires_at != expires_at.to_rfc3339()
        {
            return Err(DatabaseError::ActionProposalConflict);
        }
        transaction.commit()?;
        Ok(Some(proposal))
    }

    pub fn active_action_proposals(
        &self,
        now: &str,
    ) -> Result<Vec<InertActionProposal>, DatabaseError> {
        let now = chrono::DateTime::parse_from_rfc3339(now)
            .map_err(|_| DatabaseError::InvalidActionProposal)?
            .to_rfc3339();
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT p.id,p.proposal_key,p.audit_event_key,p.action_kind,p.sensitivity,p.policy,p.disposition,p.reason_code,p.display_label,p.target_ref,p.confidence,p.expires_at,p.created_at,'pending' FROM action_proposals p LEFT JOIN action_proposal_events e ON e.proposal_id=p.id WHERE e.id IS NULL AND julianday(p.expires_at)>julianday(?1) ORDER BY p.created_at,p.id",
        )?;
        let values = statement
            .query_map([now], action_proposal_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn action_proposal_queue(
        &self,
        now: &str,
    ) -> Result<Vec<InertActionProposal>, DatabaseError> {
        let now = chrono::DateTime::parse_from_rfc3339(now)
            .map_err(|_| DatabaseError::InvalidActionProposal)?
            .to_rfc3339();
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT p.id,p.proposal_key,p.audit_event_key,p.action_kind,p.sensitivity,p.policy,p.disposition,p.reason_code,p.display_label,p.target_ref,p.confidence,p.expires_at,p.created_at,CASE WHEN p.action_kind='correspondence' AND EXISTS(SELECT 1 FROM correspondence_executions x JOIN correspondence_execution_results r ON r.execution_id=x.id WHERE x.proposal_id=p.id) THEN 'closed' WHEN e.event_type='confirm' THEN 'confirmed' WHEN e.event_type='cancel' THEN 'cancelled' WHEN julianday(p.expires_at)<=julianday(?1) THEN 'expired' ELSE 'pending' END FROM action_proposals p LEFT JOIN action_proposal_events e ON e.proposal_id=p.id ORDER BY p.created_at DESC,p.id DESC LIMIT 100",
        )?;
        let values = statement
            .query_map([now], action_proposal_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn notification_previews(
        &self,
        now: &str,
    ) -> Result<Vec<notifications::NotificationPlan>, DatabaseError> {
        use chrono::TimeZone;
        use notifications::{
            ContentMode, NotificationKind, NotificationRequest, PrivacyClass, QuietHours,
        };

        let now = chrono::DateTime::parse_from_rfc3339(now)
            .map_err(|_| DatabaseError::InvalidNotificationPreview)?;
        let settings = self.settings()?;
        let items = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            let mut statement = connection.prepare(
                "SELECT i.id,i.kind,i.title,i.category,COALESCE(i.scheduled_at,i.due_at,i.start_at,i.follow_up_at),a.urgency
                 FROM local_items i JOIN message_analysis a ON a.account_id=i.account_id AND a.provider_id=i.provider_id
                 WHERE i.status='active' AND i.lifecycle_state!='completed' ORDER BY i.id",
            )?;
            let values = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            values
        };
        let quiet_hours = Some(QuietHours {
            start_minute: settings.quiet_hours_start_minute,
            end_minute: settings.quiet_hours_end_minute,
        });
        let mut plans = Vec::new();
        for (id, kind, title, category, effective_at, urgency) in &items {
            let effective_at = effective_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
            let privacy = if urgency == "critical" {
                PrivacyClass::Sensitive
            } else if category == "work" {
                PrivacyClass::WorkPrivate
            } else {
                PrivacyClass::Private
            };
            let content_mode =
                if matches!(privacy, PrivacyClass::WorkPrivate | PrivacyClass::Sensitive) {
                    ContentMode::Generic
                } else {
                    ContentMode::Full
                };
            if settings.appointment_reminders_enabled && kind == "appointment" {
                if let Some(start) = effective_at
                    .filter(|start| *start > now && *start <= now + chrono::Duration::days(7))
                {
                    let requested = std::cmp::max(
                        start - chrono::Duration::minutes(30),
                        now + chrono::Duration::minutes(1),
                    );
                    let body = if content_mode == ContentMode::Generic {
                        "You have a private appointment soon.".to_owned()
                    } else {
                        bounded_display_label("Upcoming: ", title)
                    };
                    plans.push(
                        notifications::plan_notification(
                            NotificationRequest {
                                idempotency_key: format!("appointment-reminder-{id:016}"),
                                kind: NotificationKind::Reminder,
                                privacy,
                                content_mode,
                                title: if content_mode == ContentMode::Generic {
                                    "Private reminder".into()
                                } else {
                                    "Appointment reminder".into()
                                },
                                body,
                                deliver_at: requested,
                            },
                            now,
                            quiet_hours,
                        )
                        .map_err(|_| DatabaseError::InvalidNotificationPreview)?,
                    );
                }
            }
            if settings.urgent_alerts_enabled && matches!(urgency.as_str(), "high" | "critical") {
                plans.push(
                    notifications::plan_notification(
                        NotificationRequest {
                            idempotency_key: format!("urgent-alert-{id:016}"),
                            kind: NotificationKind::UrgentAlert,
                            privacy,
                            content_mode,
                            title: "Urgent item".into(),
                            body: if content_mode == ContentMode::Generic {
                                "A private item needs attention.".into()
                            } else {
                                bounded_display_label("Needs attention: ", title)
                            },
                            deliver_at: now + chrono::Duration::minutes(1),
                        },
                        now,
                        quiet_hours,
                    )
                    .map_err(|_| DatabaseError::InvalidNotificationPreview)?,
                );
            }
        }
        for (enabled, kind, hour, label) in [
            (
                settings.morning_summary_enabled,
                NotificationKind::MorningSummary,
                7,
                "Morning summary",
            ),
            (
                settings.evening_summary_enabled,
                NotificationKind::EveningSummary,
                19,
                "Evening summary",
            ),
        ] {
            if !enabled {
                continue;
            }
            let mut date = now.date_naive();
            let local = date
                .and_hms_opt(hour, 30, 0)
                .ok_or(DatabaseError::InvalidNotificationPreview)?;
            let mut deliver_at = now
                .offset()
                .from_local_datetime(&local)
                .single()
                .ok_or(DatabaseError::InvalidNotificationPreview)?;
            if deliver_at <= now {
                date = date
                    .succ_opt()
                    .ok_or(DatabaseError::InvalidNotificationPreview)?;
                let local = date
                    .and_hms_opt(hour, 30, 0)
                    .ok_or(DatabaseError::InvalidNotificationPreview)?;
                deliver_at = now
                    .offset()
                    .from_local_datetime(&local)
                    .single()
                    .ok_or(DatabaseError::InvalidNotificationPreview)?;
            }
            let summary_date = if kind == NotificationKind::EveningSummary {
                deliver_at
                    .date_naive()
                    .succ_opt()
                    .ok_or(DatabaseError::InvalidNotificationPreview)?
            } else {
                deliver_at.date_naive()
            };
            let summary_count = items
                .iter()
                .filter_map(|item| item.4.as_deref())
                .filter_map(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .filter(|value| value.with_timezone(now.offset()).date_naive() == summary_date)
                .count();
            let summary_day = if kind == NotificationKind::EveningSummary {
                "tomorrow"
            } else {
                "today"
            };
            plans.push(
                notifications::plan_notification(
                    NotificationRequest {
                        idempotency_key: format!(
                            "{}-{}",
                            label.to_ascii_lowercase().replace(' ', "-"),
                            deliver_at.format("%Y%m%d")
                        ),
                        kind,
                        privacy: PrivacyClass::Private,
                        content_mode: ContentMode::Generic,
                        title: label.into(),
                        body: format!(
                            "{summary_count} local item{} scheduled {summary_day}.",
                            if summary_count == 1 { "" } else { "s" }
                        ),
                        deliver_at,
                    },
                    now,
                    quiet_hours,
                )
                .map_err(|_| DatabaseError::InvalidNotificationPreview)?,
            );
        }
        plans.sort_by_key(|plan| plan.deliver_at);
        Ok(plans)
    }

    pub fn record_notification_test_request(&self, event_key: &str) -> Result<(), DatabaseError> {
        validate_proposal_identifier(event_key)?;
        let policy = self.settings()?.automation_policy;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let existing: Option<(String, String, String)> = connection
            .query_row(
                "SELECT policy,decision,reason_code FROM automation_audit WHERE event_key=?1",
                [event_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let expected = (
            policy.as_str().to_owned(),
            "confirmed".to_owned(),
            "generic_test_requested".to_owned(),
        );
        if let Some(existing) = existing {
            return if existing == expected {
                Ok(())
            } else {
                Err(DatabaseError::ActionProposalConflict)
            };
        }
        connection.execute(
            "INSERT INTO automation_audit(event_key,policy,decision,action_kind,reason_code) VALUES (?1,?2,'confirmed','notification','generic_test_requested')",
            params![event_key, policy.as_str()],
        )?;
        Ok(())
    }

    pub fn record_notification_schedule_event(
        &self,
        event_key: &str,
        delivery_key: &str,
        event_type: &str,
        notification_kind: notifications::NotificationKind,
        deliver_at: &str,
        reason_code: &str,
    ) -> Result<(), DatabaseError> {
        validate_proposal_identifier(event_key)
            .map_err(|_| DatabaseError::InvalidNotificationScheduleEvent)?;
        validate_proposal_identifier(delivery_key)
            .map_err(|_| DatabaseError::InvalidNotificationScheduleEvent)?;
        if !matches!(
            event_type,
            "scheduled" | "cancelled" | "delivered" | "failed"
        ) || !(2..=80).contains(&reason_code.len())
            || !reason_code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            || chrono::DateTime::parse_from_rfc3339(deliver_at).is_err()
        {
            return Err(DatabaseError::InvalidNotificationScheduleEvent);
        }
        let kind = enum_name(&notification_kind)?;
        let expected = (
            delivery_key.to_owned(),
            event_type.to_owned(),
            kind.clone(),
            deliver_at.to_owned(),
            reason_code.to_owned(),
        );
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let existing: Option<(String, String, String, String, String)> = connection
            .query_row(
                "SELECT delivery_key,event_type,notification_kind,deliver_at,reason_code FROM notification_schedule_events WHERE event_key=?1",
                [event_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        if let Some(existing) = existing {
            return if existing == expected {
                Ok(())
            } else {
                Err(DatabaseError::NotificationScheduleConflict)
            };
        }
        connection.execute(
            "INSERT INTO notification_schedule_events(event_key,delivery_key,event_type,notification_kind,deliver_at,reason_code) VALUES (?1,?2,?3,?4,?5,?6)",
            params![event_key, delivery_key, event_type, kind, deliver_at, reason_code],
        )?;
        Ok(())
    }

    pub fn record_notification_schedule_cancellation(
        &self,
        event_key: &str,
        delivery_key: &str,
    ) -> Result<(), DatabaseError> {
        let (kind, deliver_at): (String, String) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            connection
                .query_row(
                    "SELECT notification_kind,deliver_at FROM notification_schedule_events WHERE delivery_key=?1 AND event_type='scheduled' ORDER BY id DESC LIMIT 1",
                    [delivery_key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or(DatabaseError::InvalidNotificationScheduleEvent)?
        };
        let kind = match kind.as_str() {
            "reminder" => notifications::NotificationKind::Reminder,
            "urgent_alert" => notifications::NotificationKind::UrgentAlert,
            "morning_summary" => notifications::NotificationKind::MorningSummary,
            "evening_summary" => notifications::NotificationKind::EveningSummary,
            _ => return Err(DatabaseError::InvalidNotificationScheduleEvent),
        };
        self.record_notification_schedule_event(
            event_key,
            delivery_key,
            "cancelled",
            kind,
            &deliver_at,
            "no_longer_desired",
        )
    }

    pub fn satisfied_notification_delivery_keys(
        &self,
        now: &str,
    ) -> Result<Vec<String>, DatabaseError> {
        chrono::DateTime::parse_from_rfc3339(now)
            .map_err(|_| DatabaseError::InvalidNotificationScheduleEvent)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT delivery_key FROM notification_schedule_events WHERE event_type='scheduled' AND deliver_at<=?1",
        )?;
        let keys = statement
            .query_map([now], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(keys)
    }

    pub fn decide_action_proposal(
        &self,
        proposal_key: &str,
        event_key: &str,
        event_type: &str,
    ) -> Result<InertActionProposal, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(event_key)?;
        if !matches!(event_type, "confirm" | "cancel") {
            return Err(DatabaseError::InvalidActionProposalDecision);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let proposal = transaction
            .query_row(
                "SELECT id,proposal_key,audit_event_key,action_kind,sensitivity,policy,disposition,reason_code,display_label,target_ref,confidence,expires_at,created_at,'pending' FROM action_proposals WHERE proposal_key=?1",
                [proposal_key],
                action_proposal_from_row,
            )
            .optional()?
            .ok_or(DatabaseError::ActionProposalMissing)?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&proposal.expires_at)
            .map_err(|_| DatabaseError::InvalidActionProposal)?;
        if expires_at <= chrono::Utc::now() {
            return Err(DatabaseError::ActionProposalExpired);
        }
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT event_key,event_type FROM action_proposal_events WHERE proposal_id=?1",
                [proposal.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_key, existing_type)) = existing {
            if existing_key != event_key || existing_type != event_type {
                return Err(DatabaseError::ActionProposalAlreadyDecided);
            }
            let mut replay = proposal;
            replay.state = if event_type == "confirm" {
                "confirmed".into()
            } else {
                "cancelled".into()
            };
            return Ok(replay);
        }
        if event_type == "confirm" && proposal.action_kind == "correspondence" {
            let latest: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM correspondence_targets c JOIN reply_draft_revisions r ON r.revision_key=c.revision_key JOIN reply_drafts d ON d.id=r.draft_id WHERE c.proposal_id=?1 AND r.id=(SELECT MAX(latest.id) FROM reply_draft_revisions latest WHERE latest.draft_id=d.id))",
                [proposal.id],
                |row| row.get(0),
            )?;
            if !latest {
                return Err(DatabaseError::InvalidReplyDraft);
            }
        }
        let audit_decision = if event_type == "confirm" {
            "confirmed"
        } else {
            "undone"
        };
        let reason_code = if event_type == "confirm" {
            "user_confirmed_inert"
        } else {
            "user_cancelled_inert"
        };
        transaction.execute(
            "INSERT INTO automation_audit(event_key,policy,decision,action_kind,target_ref,reason_code) VALUES (?1,?2,?3,?4,?5,?6)",
            params![event_key, proposal.policy, audit_decision, proposal.action_kind, proposal.target_ref, reason_code],
        )?;
        transaction.execute(
            "INSERT INTO action_proposal_events(event_key,proposal_id,event_type) VALUES (?1,?2,?3)",
            params![event_key, proposal.id, event_type],
        )?;
        transaction.commit()?;
        let mut decided = proposal;
        decided.state = if event_type == "confirm" {
            "confirmed".into()
        } else {
            "cancelled".into()
        };
        Ok(decided)
    }

    pub fn prepare_calendar_execution(
        &self,
        proposal_key: &str,
        execution_key: &str,
    ) -> Result<CalendarExecutionPreparation, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(execution_key)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let (proposal_id, target_ref, expires_at): (i64, String, String) = transaction
            .query_row(
                "SELECT p.id,p.target_ref,p.expires_at FROM action_proposals p JOIN action_proposal_events e ON e.proposal_id=p.id AND e.event_type='confirm' WHERE p.proposal_key=?1 AND p.action_kind='calendar_create'",
                [proposal_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        if chrono::DateTime::parse_from_rfc3339(&expires_at)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?
            <= chrono::Utc::now()
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let item_id = target_ref
            .strip_prefix("local-item:")
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        let (account_id, title, start_at, end_at): (String, String, String, String) = transaction
            .query_row(
                "SELECT account_id,title,start_at,end_at FROM local_items WHERE id=?1 AND kind='appointment' AND status='active' AND lifecycle_state='active'",
                [item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        let start = parse_calendar_execution_datetime(&start_at)?;
        let end = parse_calendar_execution_datetime(&end_at)?;
        if title.trim().is_empty()
            || title.len() > 200
            || title.chars().any(char::is_control)
            || end <= start
            || end - start > chrono::Duration::days(7)
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let existing: Option<(String, String, String, Option<i64>)> = transaction
            .query_row(
                "SELECT e.execution_key,e.transaction_id,e.account_id,r.id FROM calendar_executions e LEFT JOIN calendar_execution_results r ON r.execution_id=e.id WHERE e.proposal_id=?1",
                [proposal_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (effective_execution_key, transaction_id) = if let Some(existing) = existing {
            if existing.2 != account_id || existing.3.is_some() {
                return Err(DatabaseError::CalendarExecutionConflict);
            }
            (existing.0, existing.1)
        } else {
            let key_in_use: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM calendar_executions WHERE execution_key=?1)",
                [execution_key],
                |row| row.get(0),
            )?;
            if key_in_use {
                return Err(DatabaseError::CalendarExecutionConflict);
            }
            transaction.execute(
                "INSERT INTO calendar_executions(execution_key,proposal_id,transaction_id,account_id) VALUES (?1,?2,?1,?3)",
                params![execution_key, proposal_id, account_id],
            )?;
            (execution_key.to_owned(), execution_key.to_owned())
        };
        transaction.commit()?;
        Ok(CalendarExecutionPreparation {
            execution_key: effective_execution_key,
            transaction_id,
            proposal_key: proposal_key.to_owned(),
            account_id,
            title: title.trim().to_owned(),
            start_at: start.to_rfc3339(),
            end_at: end.to_rfc3339(),
        })
    }

    pub fn prepare_calendar_update_execution(
        &self,
        proposal_key: &str,
        execution_key: &str,
    ) -> Result<CalendarUpdatePreparation, DatabaseError> {
        validate_proposal_identifier(proposal_key)?;
        validate_proposal_identifier(execution_key)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let (proposal_id, expires_at): (i64, String) = transaction
            .query_row(
                "SELECT p.id,p.expires_at FROM action_proposals p JOIN action_proposal_events e ON e.proposal_id=p.id AND e.event_type='confirm' WHERE p.proposal_key=?1 AND p.action_kind='calendar_update'",
                [proposal_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        if chrono::DateTime::parse_from_rfc3339(&expires_at)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?
            <= chrono::Utc::now()
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let target: (String, String, String, String, String, String, String) = transaction
            .query_row(
                "SELECT account_id,provider_event_id,provider_etag,previous_start_at,previous_end_at,proposed_start_at,proposed_end_at FROM calendar_update_targets WHERE proposal_id=?1",
                [proposal_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        let current_etag: String = transaction
            .query_row(
                "SELECT provider_etag FROM calendar_events WHERE account_id=?1 AND provider_id=?2 AND is_cancelled=0",
                params![target.0, target.1],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        if current_etag != target.2 {
            return Err(DatabaseError::CalendarExecutionConflict);
        }
        let proposed_start = chrono::DateTime::parse_from_rfc3339(&target.5)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?;
        let proposed_end = chrono::DateTime::parse_from_rfc3339(&target.6)
            .map_err(|_| DatabaseError::InvalidCalendarExecution)?;
        if proposed_end <= proposed_start
            || proposed_end - proposed_start > chrono::Duration::days(7)
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let existing: Option<(String, String, Option<i64>)> = transaction
            .query_row(
                "SELECT e.execution_key,e.account_id,r.id FROM calendar_executions e LEFT JOIN calendar_execution_results r ON r.execution_id=e.id WHERE e.proposal_id=?1",
                [proposal_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let effective_execution_key = if let Some(existing) = existing {
            if existing.1 != target.0 || existing.2.is_some() {
                return Err(DatabaseError::CalendarExecutionConflict);
            }
            existing.0
        } else {
            let key_in_use: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM calendar_executions WHERE execution_key=?1)",
                [execution_key],
                |row| row.get(0),
            )?;
            if key_in_use {
                return Err(DatabaseError::CalendarExecutionConflict);
            }
            transaction.execute(
                "INSERT INTO calendar_executions(execution_key,proposal_id,transaction_id,account_id) VALUES (?1,?2,?1,?3)",
                params![execution_key, proposal_id, target.0],
            )?;
            execution_key.to_owned()
        };
        transaction.commit()?;
        Ok(CalendarUpdatePreparation {
            execution_key: effective_execution_key,
            proposal_key: proposal_key.to_owned(),
            account_id: target.0,
            provider_event_id: target.1,
            provider_etag: target.2,
            previous_start_at: target.3,
            previous_end_at: target.4,
            proposed_start_at: proposed_start.to_rfc3339(),
            proposed_end_at: proposed_end.to_rfc3339(),
        })
    }

    pub fn record_calendar_execution_result(
        &self,
        execution_key: &str,
        event_key: &str,
        outcome: &str,
        provider_event_id: Option<&str>,
        reason_code: &str,
    ) -> Result<(), DatabaseError> {
        validate_proposal_identifier(execution_key)?;
        validate_proposal_identifier(event_key)?;
        if !matches!(outcome, "succeeded" | "failed" | "unknown")
            || !(2..=80).contains(&reason_code.len())
            || !reason_code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || provider_event_id.is_some_and(|value| {
                value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
            })
            || (outcome == "succeeded") != provider_event_id.is_some()
        {
            return Err(DatabaseError::InvalidCalendarExecution);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let execution_id: i64 = transaction
            .query_row(
                "SELECT id FROM calendar_executions WHERE execution_key=?1",
                [execution_key],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(DatabaseError::InvalidCalendarExecution)?;
        let existing: Option<(String, String, Option<String>, String)> = transaction
            .query_row(
                "SELECT event_key,outcome,provider_event_id,reason_code FROM calendar_execution_results WHERE execution_id=?1 OR event_key=?2",
                params![execution_id, event_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let expected = (
            event_key.to_owned(),
            outcome.to_owned(),
            provider_event_id.map(str::to_owned),
            reason_code.to_owned(),
        );
        if let Some(existing) = existing {
            if existing != expected {
                return Err(DatabaseError::CalendarExecutionConflict);
            }
            return Ok(());
        }
        transaction.execute(
            "INSERT INTO calendar_execution_results(event_key,execution_id,outcome,provider_event_id,reason_code) VALUES (?1,?2,?3,?4,?5)",
            params![event_key, execution_id, outcome, provider_event_id, reason_code],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn family_display_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FamilyDisplayRecord> {
    Ok(FamilyDisplayRecord {
        id: row.get(0)?,
        display_name: row.get(1)?,
        revoked: row.get(2)?,
        last_seen_at: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn valid_display_identifier(value: &str) -> bool {
    (16..=100).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_family_display_service_values(
    enabled: bool,
    bind_address: Option<&str>,
    certificate_sha256: Option<&[u8; 32]>,
) -> bool {
    let (Some(raw_address), Some(_)) = (bind_address, certificate_sha256) else {
        return !enabled && bind_address.is_none() && certificate_sha256.is_none();
    };
    let Ok(address) = raw_address.parse::<std::net::SocketAddr>() else {
        return false;
    };
    address.port() == 8765
        && match address.ip() {
            std::net::IpAddr::V4(value) => value.is_private() || value.is_loopback(),
            std::net::IpAddr::V6(value) => value.is_unique_local() || value.is_loopback(),
        }
}

fn encode_digest(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalize_display_timestamp(value: &str) -> Option<String> {
    use chrono::TimeZone;
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(parsed.to_rfc3339());
    }
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .and_then(|value| chrono::Local.from_local_datetime(&value).single())
        .map(|value| value.to_rfc3339())
}

fn display_privacy_name(value: display_api::PrivacyProfile) -> &'static str {
    match value {
        display_api::PrivacyProfile::PublicFamily => "PUBLIC_FAMILY",
        display_api::PrivacyProfile::Private => "PRIVATE",
        display_api::PrivacyProfile::WorkPrivate => "WORK_PRIVATE",
        display_api::PrivacyProfile::Sensitive => "SENSITIVE",
    }
}

fn parse_display_privacy(value: &str) -> Result<display_api::PrivacyProfile, DatabaseError> {
    match value {
        "PUBLIC_FAMILY" => Ok(display_api::PrivacyProfile::PublicFamily),
        "PRIVATE" => Ok(display_api::PrivacyProfile::Private),
        "WORK_PRIVATE" => Ok(display_api::PrivacyProfile::WorkPrivate),
        "SENSITIVE" => Ok(display_api::PrivacyProfile::Sensitive),
        _ => Err(DatabaseError::InvalidFamilyDisplay),
    }
}

fn display_kind(value: &str) -> display_api::DisplayKind {
    match value {
        "appointment" => display_api::DisplayKind::Appointment,
        "waiting_for" => display_api::DisplayKind::Reminder,
        _ => display_api::DisplayKind::Task,
    }
}

fn display_category(value: &str) -> display_api::DisplayCategory {
    match value {
        "school" => display_api::DisplayCategory::School,
        "creche" => display_api::DisplayCategory::Creche,
        "kids" => display_api::DisplayCategory::Kids,
        "personal" => display_api::DisplayCategory::Personal,
        "work" | "work_administration" => display_api::DisplayCategory::Work,
        "family" => display_api::DisplayCategory::Family,
        _ => display_api::DisplayCategory::Home,
    }
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("settings validation failed: {0}")]
    Validation(#[from] assistant_core::ValidationError),
    #[error("settings row is missing")]
    SettingsMissing,
    #[error("database lock was poisoned")]
    LockPoisoned,
    #[error("backup destination is invalid")]
    InvalidBackupDestination,
    #[error("backup file operation failed")]
    BackupIo(#[from] std::io::Error),
    #[error("invalid synchronization resource")]
    InvalidResource,
    #[error("local email analysis requires a qualified private AI build")]
    AiNotQualified,
    #[error("local AI qualification data is invalid")]
    InvalidQualification,
    #[error("analysis result is invalid")]
    InvalidAnalysis,
    #[error("source message is unavailable")]
    MessageMissing,
    #[error("review decision must be accept or ignore")]
    InvalidReviewDecision,
    #[error("this review item already has a different decision")]
    ReviewAlreadyDecided,
    #[error("local item is unavailable")]
    LocalItemMissing,
    #[error("local item transition is invalid")]
    InvalidLocalTransition,
    #[error("focus view request is invalid")]
    InvalidFocusView,
    #[error("source email link is unavailable")]
    SourceLinkUnavailable,
    #[error("action proposal is invalid")]
    InvalidActionProposal,
    #[error("an idempotency key already identifies a different action proposal")]
    ActionProposalConflict,
    #[error("action proposal is unavailable")]
    ActionProposalMissing,
    #[error("action proposal has expired")]
    ActionProposalExpired,
    #[error("action proposal decision must be confirm or cancel")]
    InvalidActionProposalDecision,
    #[error("this action proposal already has a different terminal decision")]
    ActionProposalAlreadyDecided,
    #[error("notification preview could not be generated safely")]
    InvalidNotificationPreview,
    #[error("notification schedule event is invalid")]
    InvalidNotificationScheduleEvent,
    #[error("an idempotency key already identifies a different notification schedule event")]
    NotificationScheduleConflict,
    #[error("calendar execution preparation is invalid")]
    InvalidCalendarExecution,
    #[error("calendar execution preparation conflicts with an existing attempt")]
    CalendarExecutionConflict,
    #[error("reply draft metadata is invalid")]
    InvalidReplyDraft,
    #[error("an idempotency key already identifies different reply draft metadata")]
    ReplyDraftConflict,
    #[error("correspondence execution preparation is invalid")]
    InvalidCorrespondenceExecution,
    #[error("correspondence execution preparation conflicts with an existing attempt")]
    CorrespondenceExecutionConflict,
    #[error("family display metadata is invalid")]
    InvalidFamilyDisplay,
    #[error("family display is unavailable")]
    FamilyDisplayMissing,
}

fn action_proposal_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InertActionProposal> {
    Ok(InertActionProposal {
        id: row.get(0)?,
        proposal_key: row.get(1)?,
        audit_event_key: row.get(2)?,
        action_kind: row.get(3)?,
        sensitivity: row.get(4)?,
        policy: row.get(5)?,
        disposition: row.get(6)?,
        reason_code: row.get(7)?,
        display_label: row.get(8)?,
        target_ref: row.get(9)?,
        confidence: row.get(10)?,
        expires_at: row.get(11)?,
        created_at: row.get(12)?,
        state: row.get(13)?,
    })
}

fn validate_proposal_identifier(value: &str) -> Result<(), DatabaseError> {
    if !(16..=100).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(DatabaseError::InvalidActionProposal);
    }
    Ok(())
}

fn validate_display_label(value: &str) -> Result<(), DatabaseError> {
    if value.is_empty()
        || value.chars().count() > 200
        || value.chars().any(|character| character.is_control())
    {
        return Err(DatabaseError::InvalidActionProposal);
    }
    Ok(())
}

fn parse_calendar_execution_datetime(
    value: &str,
) -> Result<chrono::DateTime<chrono::FixedOffset>, DatabaseError> {
    use chrono::TimeZone;
    if let Ok(value) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(value);
    }
    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| DatabaseError::InvalidCalendarExecution)?;
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|value| value.fixed_offset())
        .ok_or(DatabaseError::InvalidCalendarExecution)
}

fn bounded_display_label(prefix: &str, value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let available = 200usize.saturating_sub(prefix.chars().count());
    let value = cleaned.trim().chars().take(available).collect::<String>();
    format!("{prefix}{value}")
}

fn effective_item_timestamp(item: &LocalItem) -> Option<&str> {
    item.scheduled_at
        .as_deref()
        .or(item.due_at.as_deref())
        .or(item.start_at.as_deref())
        .or(item.follow_up_at.as_deref())
}

fn local_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalItem> {
    Ok(LocalItem {
        id: row.get(0)?,
        account_id: row.get(1)?,
        provider_id: row.get(2)?,
        suggestion_index: row.get(3)?,
        kind: row.get(4)?,
        title: row.get(5)?,
        due_at: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        expected_from: row.get(9)?,
        follow_up_at: row.get(10)?,
        category: row.get(11)?,
        status: row.get(12)?,
        lifecycle_state: row.get(13)?,
        scheduled_at: row.get(14)?,
        created_at: row.get(15)?,
        undone_at: row.get(16)?,
    })
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SanitizedSuggestion<'a> {
    Task {
        title: &'a str,
        due_at: Option<&'a str>,
    },
    Appointment {
        title: &'a str,
        start_at: Option<&'a str>,
        end_at: Option<&'a str>,
        confirmed: bool,
    },
    WaitingFor {
        description: &'a str,
        expected_from: Option<&'a str>,
        follow_up_at: Option<&'a str>,
    },
}

fn sanitized_suggestion<'a>(
    extraction: &'a ai::Extraction,
    proposal: &rules::Proposal,
) -> Result<SanitizedSuggestion<'a>, DatabaseError> {
    match proposal {
        rules::Proposal::Task { candidate_index } => {
            extraction
                .tasks
                .get(*candidate_index)
                .map(|candidate| SanitizedSuggestion::Task {
                    title: &candidate.title,
                    due_at: candidate.due_at.as_deref(),
                })
        }
        rules::Proposal::Appointment { candidate_index } => extraction
            .appointments
            .get(*candidate_index)
            .map(|candidate| SanitizedSuggestion::Appointment {
                title: &candidate.title,
                start_at: candidate.start_at.as_deref(),
                end_at: candidate.end_at.as_deref(),
                confirmed: candidate.confirmed,
            }),
        rules::Proposal::WaitingFor { candidate_index } => extraction
            .waiting_for
            .get(*candidate_index)
            .map(|candidate| SanitizedSuggestion::WaitingFor {
                description: &candidate.description,
                expected_from: candidate.expected_from.as_deref(),
                follow_up_at: candidate.follow_up_at.as_deref(),
            }),
    }
    .ok_or(DatabaseError::InvalidAnalysis)
}

fn enum_name(value: &impl serde::Serialize) -> Result<String, DatabaseError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(DatabaseError::InvalidAnalysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_defaults() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
        assert!(!db.settings().unwrap().store_complete_email_content);
        assert!(!db.settings().unwrap().local_email_analysis_enabled);
        assert!(!db.settings().unwrap().notification_delivery_enabled);
        let connection = db.connection.lock().unwrap();
        let versions = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
        let integrity: String = connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        drop(connection);
        assert_eq!(
            db.settings().unwrap().automation_policy,
            AutomationPolicy::Balanced
        );
    }

    #[test]
    fn migration_rejects_future_schema_without_creating_application_tables() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let connection = Connection::open(file.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO schema_migrations(version) VALUES (28);
                 CREATE TABLE preservation_marker(value TEXT NOT NULL);
                 INSERT INTO preservation_marker(value) VALUES ('unchanged');",
            )
            .unwrap();
        drop(connection);

        assert!(Database::open(file.path()).is_err());
        let connection = Connection::open(file.path()).unwrap();
        let marker: String = connection
            .query_row("SELECT value FROM preservation_marker", [], |row| {
                row.get(0)
            })
            .unwrap();
        let settings_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker, "unchanged");
        assert_eq!(settings_count, 0);
    }

    #[test]
    fn migration_rejects_a_non_contiguous_ledger_before_changes() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let connection = Connection::open(file.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO schema_migrations(version) VALUES (1), (3);",
            )
            .unwrap();
        drop(connection);

        assert!(Database::open(file.path()).is_err());
        let connection = Connection::open(file.path()).unwrap();
        let settings_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(settings_count, 0);
    }
    #[test]
    fn persists_valid_settings() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let expected = Settings {
            household_name: "Tobin family".into(),
            theme: Theme::Dark,
            launch_at_login: true,
            store_complete_email_content: false,
            local_email_analysis_enabled: false,
            automation_policy: AutomationPolicy::Conservative,
            notification_delivery_enabled: true,
            urgent_alerts_enabled: true,
            appointment_reminders_enabled: true,
            morning_summary_enabled: true,
            evening_summary_enabled: false,
            quiet_hours_start_minute: 1260,
            quiet_hours_end_minute: 390,
        };
        Database::open(file.path())
            .unwrap()
            .save_settings(expected.clone())
            .unwrap();
        assert_eq!(
            Database::open(file.path()).unwrap().settings().unwrap(),
            expected
        );
    }
    #[test]
    fn automation_audit_schema_is_append_only() {
        let db = Database::open_in_memory().unwrap();
        let connection = db.connection.lock().unwrap();
        connection.execute("INSERT INTO automation_audit(event_key,policy,decision,action_kind,reason_code) VALUES (?1,'balanced','blocked','calendar_create','foundation_only')", ["automation-event-0001"]).unwrap();
        drop(connection);
        let history = db.automation_audit_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].policy, "balanced");
        assert_eq!(history[0].decision, "blocked");
        assert_eq!(history[0].action_kind, "calendar_create");
        assert_eq!(history[0].reason_code, "foundation_only");
        let connection = db.connection.lock().unwrap();
        assert!(connection
            .execute(
                "UPDATE automation_audit SET decision='executed' WHERE event_key=?1",
                ["automation-event-0001"]
            )
            .is_err());
        assert!(connection
            .execute(
                "DELETE FROM automation_audit WHERE event_key=?1",
                ["automation-event-0001"]
            )
            .is_err());
    }
    #[test]
    fn generic_notification_test_request_is_audited_and_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.record_notification_test_request("notification-test-0001")
            .unwrap();
        db.record_notification_test_request("notification-test-0001")
            .unwrap();

        let connection = db.connection.lock().unwrap();
        let row: (String, String, String, String, i64) = connection
            .query_row(
                "SELECT policy,decision,action_kind,reason_code,COUNT(*) FROM automation_audit WHERE event_key=?1",
                ["notification-test-0001"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "balanced".into(),
                "confirmed".into(),
                "notification".into(),
                "generic_test_requested".into(),
                1,
            )
        );
    }
    #[test]
    fn notification_schedule_ledger_is_append_only_idempotent_and_content_free() {
        let db = Database::open_in_memory().unwrap();
        db.record_notification_schedule_event(
            "schedule-event-0001",
            "notification-key-0001",
            "scheduled",
            notifications::NotificationKind::Reminder,
            "2026-08-27T09:00:00+00:00",
            "desired_plan",
        )
        .unwrap();
        db.record_notification_schedule_event(
            "schedule-event-0001",
            "notification-key-0001",
            "scheduled",
            notifications::NotificationKind::Reminder,
            "2026-08-27T09:00:00+00:00",
            "desired_plan",
        )
        .unwrap();
        assert!(matches!(
            db.record_notification_schedule_event(
                "schedule-event-0001",
                "notification-key-0001",
                "cancelled",
                notifications::NotificationKind::Reminder,
                "2026-08-27T09:00:00+00:00",
                "consent_revoked",
            ),
            Err(DatabaseError::NotificationScheduleConflict)
        ));
        let connection = db.connection.lock().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM notification_schedule_events",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let columns = connection
            .prepare("PRAGMA table_info(notification_schedule_events)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!columns
            .iter()
            .any(|column| matches!(column.as_str(), "title" | "body" | "content" | "payload")));
        assert!(connection.execute("UPDATE notification_schedule_events SET reason_code='changed' WHERE event_key='schedule-event-0001'", []).is_err());
        assert!(connection
            .execute(
                "DELETE FROM notification_schedule_events WHERE event_key='schedule-event-0001'",
                []
            )
            .is_err());
    }
    fn queue_input<'a>(
        proposal_key: &'a str,
        audit_event_key: &'a str,
        expires_at: &'a str,
    ) -> QueueActionProposal<'a> {
        QueueActionProposal {
            proposal_key,
            audit_event_key,
            display_label: "Create local reminder",
            target_ref: Some("local-item:42"),
            expires_at,
            policy: AutomationPolicy::Balanced,
            context: rules::AutomationContext {
                action: rules::AutomationAction::TaskCreate,
                sensitivity: rules::ActionSensitivity::Routine,
                confidence: 0.98,
                source_verified: true,
                fact_confirmed: true,
                reversible: true,
                user_confirmed: false,
            },
        }
    }

    #[test]
    fn action_proposals_are_inert_idempotent_and_append_only() {
        let db = Database::open_in_memory().unwrap();
        let expiry = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        let first = db
            .queue_action_proposal(queue_input(
                "proposal-key-0001",
                "proposal-audit-0001",
                &expiry,
            ))
            .unwrap()
            .unwrap();
        let replay = db
            .queue_action_proposal(queue_input(
                "proposal-key-0001",
                "proposal-audit-0001",
                &expiry,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.disposition, "eligible");
        let connection = db.connection.lock().unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM action_proposals", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(connection
            .execute("DELETE FROM action_proposals", [])
            .is_err());
    }

    #[test]
    fn proposal_idempotency_conflicts_fail_closed() {
        let db = Database::open_in_memory().unwrap();
        let expiry = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        db.queue_action_proposal(queue_input(
            "proposal-key-0002",
            "proposal-audit-0002",
            &expiry,
        ))
        .unwrap();
        let mut conflict = queue_input("proposal-key-0002", "proposal-audit-0002", &expiry);
        conflict.display_label = "Different operation";
        assert!(matches!(
            db.queue_action_proposal(conflict),
            Err(DatabaseError::ActionProposalConflict)
        ));
        assert!(matches!(
            db.queue_action_proposal(queue_input(
                "proposal-key-other",
                "proposal-audit-0002",
                &expiry,
            )),
            Err(DatabaseError::ActionProposalConflict)
        ));
    }

    #[test]
    fn blocked_policy_decisions_are_audited_but_never_queued() {
        let db = Database::open_in_memory().unwrap();
        let expiry = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        let mut input = queue_input("proposal-key-0003", "proposal-audit-0003", &expiry);
        input.context.source_verified = false;
        assert!(db.queue_action_proposal(input).unwrap().is_none());
        assert!(db
            .active_action_proposals(&chrono::Utc::now().to_rfc3339())
            .unwrap()
            .is_empty());
        let connection = db.connection.lock().unwrap();
        let decision: String = connection
            .query_row(
                "SELECT decision FROM automation_audit WHERE event_key='proposal-audit-0003'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(decision, "blocked");
    }

    #[test]
    fn proposal_queue_validates_expiry_and_exposes_no_raw_payload_column() {
        let db = Database::open_in_memory().unwrap();
        let expired = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        assert!(matches!(
            db.queue_action_proposal(queue_input(
                "proposal-key-0004",
                "proposal-audit-0004",
                &expired,
            )),
            Err(DatabaseError::InvalidActionProposal)
        ));
        let connection = db.connection.lock().unwrap();
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('action_proposals')")
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for forbidden in ["body", "content", "payload", "evidence", "provider_url"] {
            assert!(!columns.iter().any(|column| column.contains(forbidden)));
        }
    }

    #[test]
    fn proposal_confirmation_is_inert_append_only_and_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let expiry = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        db.queue_action_proposal(queue_input(
            "proposal-key-0005",
            "proposal-audit-0005",
            &expiry,
        ))
        .unwrap();
        let first = db
            .decide_action_proposal("proposal-key-0005", "proposal-confirm-0005", "confirm")
            .unwrap();
        let replay = db
            .decide_action_proposal("proposal-key-0005", "proposal-confirm-0005", "confirm")
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.state, "confirmed");
        assert!(matches!(
            db.decide_action_proposal("proposal-key-0005", "proposal-cancel-0005", "cancel"),
            Err(DatabaseError::ActionProposalAlreadyDecided)
        ));
        assert!(db
            .active_action_proposals(&chrono::Utc::now().to_rfc3339())
            .unwrap()
            .is_empty());
        let connection = db.connection.lock().unwrap();
        assert!(connection
            .execute("DELETE FROM action_proposal_events", [])
            .is_err());
    }

    #[test]
    fn proposal_queue_derives_pending_cancelled_and_expired_states() {
        let db = Database::open_in_memory().unwrap();
        let expiry = chrono::Utc::now() + chrono::Duration::days(1);
        db.queue_action_proposal(queue_input(
            "proposal-key-0006",
            "proposal-audit-0006",
            &expiry.to_rfc3339(),
        ))
        .unwrap();
        assert_eq!(
            db.action_proposal_queue(&chrono::Utc::now().to_rfc3339())
                .unwrap()[0]
                .state,
            "pending"
        );
        db.decide_action_proposal("proposal-key-0006", "proposal-cancel-0006", "cancel")
            .unwrap();
        assert_eq!(
            db.action_proposal_queue(&chrono::Utc::now().to_rfc3339())
                .unwrap()[0]
                .state,
            "cancelled"
        );
        assert_eq!(
            db.action_proposal_queue(&(expiry + chrono::Duration::seconds(1)).to_rfc3339())
                .unwrap()[0]
                .state,
            "cancelled"
        );
        let second_expiry = chrono::Utc::now() + chrono::Duration::days(2);
        db.queue_action_proposal(queue_input(
            "proposal-key-0007",
            "proposal-audit-0007",
            &second_expiry.to_rfc3339(),
        ))
        .unwrap();
        let states = db
            .action_proposal_queue(&(second_expiry + chrono::Duration::seconds(1)).to_rfc3339())
            .unwrap();
        assert!(states
            .iter()
            .any(|proposal| proposal.proposal_key == "proposal-key-0007"
                && proposal.state == "expired"));
    }
    #[test]
    fn stores_account_metadata_without_secrets() {
        let db = Database::open_in_memory().unwrap();
        let account = ConnectedAccount {
            id: "account-1".into(),
            provider: "microsoft".into(),
            display_name: "Consultant".into(),
            email_address: "user@example.com".into(),
            tenant_id: Some("tenant".into()),
        };
        db.upsert_account(&account).unwrap();
        assert_eq!(db.accounts().unwrap(), vec![account]);
    }

    #[test]
    fn calendar_update_candidates_require_a_synchronized_etag() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account(&ConnectedAccount {
            id: "calendar-update-account".into(),
            provider: "microsoft".into(),
            display_name: "Calendar".into(),
            email_address: "calendar@example.com".into(),
            tenant_id: None,
        })
        .unwrap();
        let event = |id: &str, etag: Option<&str>| CalendarEvent {
            etag: etag.map(str::to_owned),
            id: id.into(),
            subject: Some("Test event".into()),
            start: email::graph::GraphDateTime {
                date_time: "2026-08-28T09:00:00.0000000".into(),
                time_zone: "UTC".into(),
            },
            end: email::graph::GraphDateTime {
                date_time: "2026-08-28T09:15:00.0000000".into(),
                time_zone: "UTC".into(),
            },
            is_cancelled: false,
            web_link: None,
            last_modified_date_time: None,
        };
        db.apply_calendar_page(
            "calendar-update-account",
            &[
                event("without-etag", None),
                event("with-etag", Some("etag-1")),
            ],
        )
        .unwrap();
        let candidates = db
            .calendar_update_candidates("calendar-update-account")
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider_id, "with-etag");
        let proposal = db
            .queue_calendar_update_proposal(
                "calendar-update-proposal-0001",
                "calendar-update-audit-000001",
                "calendar-update-account",
                "with-etag",
                "2026-08-28T11:00:00+01:00",
                "2026-08-28T11:15:00+01:00",
            )
            .unwrap();
        assert_eq!(proposal.action_kind, "calendar_update");
        assert_eq!(proposal.disposition, "confirmation_required");
        assert_eq!(proposal.state, "pending");
        let replay = db
            .queue_calendar_update_proposal(
                "calendar-update-proposal-0001",
                "calendar-update-audit-000001",
                "calendar-update-account",
                "with-etag",
                "2026-08-28T11:00:00+01:00",
                "2026-08-28T11:15:00+01:00",
            )
            .unwrap();
        assert_eq!(replay.id, proposal.id);
        assert!(matches!(
            db.queue_calendar_update_proposal(
                "calendar-update-proposal-0001",
                "calendar-update-audit-000001",
                "calendar-update-account",
                "with-etag",
                "2026-08-28T12:00:00+01:00",
                "2026-08-28T12:15:00+01:00",
            ),
            Err(DatabaseError::CalendarExecutionConflict)
        ));
        db.decide_action_proposal(
            "calendar-update-proposal-0001",
            "calendar-update-confirm-00001",
            "confirm",
        )
        .unwrap();
        let prepared = db
            .prepare_calendar_update_execution(
                "calendar-update-proposal-0001",
                "calendar-update-execution-001",
            )
            .unwrap();
        assert_eq!(prepared.provider_event_id, "with-etag");
        assert_eq!(prepared.provider_etag, "etag-1");
        assert_eq!(prepared.previous_start_at, "2026-08-28T09:00:00.0000000");
        assert_eq!(prepared.proposed_start_at, "2026-08-28T11:00:00+01:00");
        let recovered = db
            .prepare_calendar_update_execution(
                "calendar-update-proposal-0001",
                "calendar-update-execution-ignored",
            )
            .unwrap();
        assert_eq!(recovered.execution_key, prepared.execution_key);
        db.record_calendar_execution_result(
            &prepared.execution_key,
            "calendar-update-result-0001",
            "succeeded",
            Some("with-etag"),
            "provider_accepted",
        )
        .unwrap();
        assert!(matches!(
            db.prepare_calendar_update_execution(
                "calendar-update-proposal-0001",
                "calendar-update-execution-002",
            ),
            Err(DatabaseError::CalendarExecutionConflict)
        ));
        let connection = db.connection.lock().unwrap();
        let target: (String, String, String, String, String) = connection
            .query_row(
                "SELECT provider_etag,previous_start_at,previous_end_at,proposed_start_at,proposed_end_at FROM calendar_update_targets WHERE proposal_id=?1",
                [proposal.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(target.0, "etag-1");
        assert_eq!(target.1, "2026-08-28T09:00:00.0000000");
        assert_eq!(target.2, "2026-08-28T09:15:00.0000000");
        assert_eq!(target.3, "2026-08-28T11:00:00+01:00");
        assert_eq!(target.4, "2026-08-28T11:15:00+01:00");
        assert!(connection
            .execute(
                "UPDATE calendar_update_targets SET provider_etag='changed' WHERE proposal_id=?1",
                [proposal.id],
            )
            .is_err());
        assert!(connection
            .execute(
                "DELETE FROM calendar_update_targets WHERE proposal_id=?1",
                [proposal.id],
            )
            .is_err());
        let trigger_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('calendar_update_targets_no_update','calendar_update_targets_no_delete')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(trigger_count, 2);
    }

    #[test]
    fn calendar_execution_ledger_is_immutable_idempotent_and_content_free() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account(&ConnectedAccount {
            id: "calendar-account".into(),
            provider: "microsoft".into(),
            display_name: "Calendar account".into(),
            email_address: "calendar@example.com".into(),
            tenant_id: None,
        })
        .unwrap();
        let connection = db.connection.lock().unwrap();
        connection.execute("INSERT INTO automation_audit(event_key,policy,decision,action_kind,target_ref,reason_code) VALUES ('calendar-audit-000001','balanced','review','calendar_create','local-item:1','user_confirmed')", []).unwrap();
        connection.execute("INSERT INTO action_proposals(proposal_key,audit_event_key,action_kind,sensitivity,policy,disposition,reason_code,display_label,target_ref,confidence,expires_at) VALUES ('calendar-proposal-0001','calendar-audit-000001','calendar_create','routine','balanced','eligible','user_confirmed','Calendar item','local-item:1',1.0,'2099-01-01T00:00:00+00:00')", []).unwrap();
        connection.execute("INSERT INTO calendar_executions(execution_key,proposal_id,transaction_id,account_id) SELECT 'calendar-execution-0001',id,'calendar-transaction-0001','calendar-account' FROM action_proposals WHERE proposal_key='calendar-proposal-0001'", []).unwrap();
        assert_eq!(connection.execute("INSERT OR IGNORE INTO calendar_executions(execution_key,proposal_id,transaction_id,account_id) SELECT 'calendar-execution-0001',id,'calendar-transaction-0001','calendar-account' FROM action_proposals WHERE proposal_key='calendar-proposal-0001'", []).unwrap(), 0);
        connection.execute("INSERT INTO calendar_execution_results(event_key,execution_id,outcome,provider_event_id,reason_code) SELECT 'calendar-result-000001',id,'succeeded','provider-event-id','provider_accepted' FROM calendar_executions WHERE execution_key='calendar-execution-0001'", []).unwrap();
        assert!(connection
            .execute(
                "UPDATE calendar_executions SET transaction_id='changed-transaction-id'",
                []
            )
            .is_err());
        assert!(connection
            .execute("DELETE FROM calendar_execution_results", [])
            .is_err());
        let columns = ["calendar_executions", "calendar_execution_results"]
            .into_iter()
            .flat_map(|table| {
                let mut statement = connection
                    .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                    .unwrap();
                statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(!columns.iter().any(|column| matches!(
            column.as_str(),
            "title" | "body" | "content" | "payload" | "evidence"
        )));
        connection.execute_batch("PRAGMA foreign_keys=OFF;
            INSERT INTO local_items(id,account_id,provider_id,suggestion_index,kind,title,start_at,end_at,category,status,lifecycle_state) VALUES (2,'calendar-account','fixture-message',0,'appointment','Confirmed meeting','2098-01-01T10:00:00+00:00','2098-01-01T11:00:00+00:00','personal','active','active');
            INSERT INTO automation_audit(event_key,policy,decision,action_kind,target_ref,reason_code) VALUES ('calendar-audit-000002','balanced','review','calendar_create','local-item:2','user_confirmed');
            INSERT INTO action_proposals(proposal_key,audit_event_key,action_kind,sensitivity,policy,disposition,reason_code,display_label,target_ref,confidence,expires_at) VALUES ('calendar-proposal-0002','calendar-audit-000002','calendar_create','routine','balanced','eligible','user_confirmed','Calendar item','local-item:2',1.0,'2099-01-01T00:00:00+00:00');
            INSERT INTO automation_audit(event_key,policy,decision,action_kind,target_ref,reason_code) VALUES ('calendar-confirm-00002','balanced','confirmed','calendar_create','local-item:2','user_confirmed_inert');
            INSERT INTO action_proposal_events(event_key,proposal_id,event_type) SELECT 'calendar-confirm-00002',id,'confirm' FROM action_proposals WHERE proposal_key='calendar-proposal-0002';
            PRAGMA foreign_keys=ON;").unwrap();
        drop(connection);
        let prepared = db
            .prepare_calendar_execution("calendar-proposal-0002", "calendar-execution-0002")
            .unwrap();
        assert_eq!(prepared.title, "Confirmed meeting");
        assert_eq!(prepared.transaction_id, "calendar-execution-0002");
        assert_eq!(
            db.prepare_calendar_execution("calendar-proposal-0002", "calendar-execution-0002")
                .unwrap(),
            prepared
        );
        assert_eq!(
            db.prepare_calendar_execution("calendar-proposal-0002", "calendar-execution-other")
                .unwrap(),
            prepared
        );
        db.record_calendar_execution_result(
            "calendar-execution-0002",
            "calendar-result-000002",
            "unknown",
            None,
            "transport_ambiguous",
        )
        .unwrap();
        db.record_calendar_execution_result(
            "calendar-execution-0002",
            "calendar-result-000002",
            "unknown",
            None,
            "transport_ambiguous",
        )
        .unwrap();
        assert!(matches!(
            db.record_calendar_execution_result(
                "calendar-execution-0002",
                "calendar-result-conflict",
                "succeeded",
                Some("provider-event"),
                "provider_accepted",
            ),
            Err(DatabaseError::CalendarExecutionConflict)
        ));
        assert!(matches!(
            db.prepare_calendar_execution("calendar-proposal-0002", "calendar-execution-later"),
            Err(DatabaseError::CalendarExecutionConflict)
        ));
    }

    #[test]
    fn message_page_and_cursor_commit_together() {
        let db = Database::open_in_memory().unwrap();
        let account = ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        };
        db.upsert_account(&account).unwrap();
        let message = MessageMetadata {
            id: "m1".into(),
            conversation_id: None,
            sender: None,
            subject: Some("Metadata only".into()),
            received_date_time: None,
            sent_date_time: None,
            web_link: None,
            is_read: false,
            removed: None,
        };
        db.apply_message_page(
            "a",
            "mail_inbox",
            &[message],
            Some("https://graph.microsoft.com/v1.0/delta"),
        )
        .unwrap();
        assert_eq!(
            db.sync_cursor("a", "mail_inbox").unwrap().as_deref(),
            Some("https://graph.microsoft.com/v1.0/delta")
        );
        let digest = "a".repeat(64);
        let draft = db
            .record_reply_draft_metadata(
                "reply-draft-key-0001",
                "reply-revision-key-0001",
                "a",
                "m1",
                &digest,
                24,
            )
            .unwrap();
        assert_eq!(draft.content_sha256, digest);
        assert_eq!(draft.content_bytes, 24);
        assert_eq!(
            db.record_reply_draft_metadata(
                "reply-draft-key-0001",
                "reply-revision-key-0001",
                "a",
                "m1",
                &"a".repeat(64),
                24,
            )
            .unwrap(),
            draft
        );
        assert!(matches!(
            db.record_reply_draft_metadata(
                "reply-draft-key-0001",
                "reply-revision-key-0001",
                "a",
                "m1",
                &"b".repeat(64),
                24,
            ),
            Err(DatabaseError::ReplyDraftConflict)
        ));
        let replacement = db
            .record_reply_draft_metadata(
                "reply-draft-key-new1",
                "reply-revision-key-0002",
                "a",
                "m1",
                &"b".repeat(64),
                31,
            )
            .unwrap();
        assert_eq!(replacement.draft_key, "reply-draft-key-0001");
        assert_eq!(replacement.revision_key, "reply-revision-key-0002");
        assert_eq!(db.reply_draft_metadata("a").unwrap(), vec![replacement]);
        let proposal = db
            .queue_reply_draft_proposal(
                "reply-proposal-key-0001",
                "reply-proposal-audit-0001",
                "reply-draft-key-0001",
                "reply-revision-key-0002",
            )
            .unwrap();
        assert_eq!(proposal.action_kind, "correspondence");
        assert_eq!(proposal.sensitivity, "sensitive");
        assert_eq!(proposal.disposition, "confirmation_required");
        assert!(!proposal.display_label.contains("Metadata only"));
        assert_eq!(
            db.queue_reply_draft_proposal(
                "reply-proposal-key-0001",
                "reply-proposal-audit-0001",
                "reply-draft-key-0001",
                "reply-revision-key-0002",
            )
            .unwrap()
            .id,
            proposal.id
        );
        db.decide_action_proposal(
            "reply-proposal-key-0001",
            "reply-confirm-event-0001",
            "confirm",
        )
        .unwrap();
        let prepared = db
            .prepare_correspondence_execution("reply-proposal-key-0001", "reply-execution-key-0001")
            .unwrap();
        assert_eq!(prepared.revision_key, "reply-revision-key-0002");
        assert_eq!(prepared.content_bytes, 31);
        assert_eq!(
            db.prepare_correspondence_execution(
                "reply-proposal-key-0001",
                "reply-execution-key-ignored",
            )
            .unwrap()
            .execution_key,
            prepared.execution_key
        );
        db.record_correspondence_execution_result(
            &prepared.execution_key,
            "reply-result-event-0001",
            "failed",
            "provider_permission_absent",
        )
        .unwrap();
        let history = db.correspondence_execution_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].display_label, "Private reply draft · 31 bytes");
        assert_eq!(history[0].outcome, "failed");
        assert_eq!(history[0].reason_code, "provider_permission_absent");
        let closed = db
            .action_proposal_queue(&chrono::Utc::now().to_rfc3339())
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.proposal_key == "reply-proposal-key-0001")
            .unwrap();
        assert_eq!(closed.state, "closed");
        assert!(matches!(
            db.prepare_correspondence_execution(
                "reply-proposal-key-0001",
                "reply-execution-key-0002",
            ),
            Err(DatabaseError::CorrespondenceExecutionConflict)
        ));
        db.record_reply_draft_metadata(
            "reply-draft-key-new2",
            "reply-revision-key-0003",
            "a",
            "m1",
            &"c".repeat(64),
            35,
        )
        .unwrap();
        db.queue_reply_draft_proposal(
            "reply-proposal-key-0002",
            "reply-proposal-audit-0002",
            "reply-draft-key-0001",
            "reply-revision-key-0003",
        )
        .unwrap();
        db.record_reply_draft_metadata(
            "reply-draft-key-new3",
            "reply-revision-key-0004",
            "a",
            "m1",
            &"d".repeat(64),
            38,
        )
        .unwrap();
        assert!(matches!(
            db.decide_action_proposal(
                "reply-proposal-key-0002",
                "reply-confirm-event-0002",
                "confirm",
            ),
            Err(DatabaseError::InvalidReplyDraft)
        ));
        let connection = db.connection.lock().unwrap();
        let columns = ["reply_drafts", "reply_draft_revisions"]
            .into_iter()
            .flat_map(|table| {
                let mut statement = connection
                    .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                    .unwrap();
                statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(!columns.iter().any(|column| matches!(
            column.as_str(),
            "body" | "content" | "subject" | "recipient" | "sender"
        )));
        assert!(connection
            .execute("UPDATE reply_draft_revisions SET content_bytes=25", [],)
            .is_err());
        assert!(connection
            .execute(
                "UPDATE correspondence_targets SET revision_key='changed-revision-key'",
                [],
            )
            .is_err());
        drop(connection);
        db.clear_synced_data("a").unwrap();
        let connection = db.connection.lock().unwrap();
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM reply_drafts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn deleting_account_cascades_sync_state() {
        let db = Database::open_in_memory().unwrap();
        let account = ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        };
        db.upsert_account(&account).unwrap();
        db.save_sync_cursor("a", "mail_inbox", "https://graph.microsoft.com/v1.0/delta")
            .unwrap();
        db.delete_account_data("a").unwrap();
        assert!(db.accounts().unwrap().is_empty());
        assert!(db.sync_cursor("a", "mail_inbox").unwrap().is_none());
    }

    #[test]
    fn clearing_synced_data_keeps_account() {
        let db = Database::open_in_memory().unwrap();
        let account = ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        };
        db.upsert_account(&account).unwrap();
        db.save_sync_cursor("a", "mail_inbox", "https://graph.microsoft.com/v1.0/delta")
            .unwrap();
        let run = db.begin_sync_run("a").unwrap();
        db.finish_sync_run(run, "success", (1, 2, 3), None).unwrap();
        db.clear_synced_data("a").unwrap();
        assert_eq!(db.accounts().unwrap(), vec![account]);
        assert!(db.sync_cursor("a", "mail_inbox").unwrap().is_none());
        assert!(db.last_sync_run("a").unwrap().is_none());
    }

    #[test]
    fn local_email_analysis_requires_a_recorded_qualification() {
        let db = Database::open_in_memory().unwrap();
        let settings = Settings {
            local_email_analysis_enabled: true,
            ..Settings::default()
        };
        assert!(matches!(
            db.save_settings(settings.clone()),
            Err(DatabaseError::AiNotQualified)
        ));
        db.record_ai_qualification(&"a".repeat(64), "build 10434", 1)
            .unwrap();
        assert!(db
            .has_ai_qualification(&"a".repeat(64), "build 10434", 1)
            .unwrap());
        assert!(
            db.save_settings(settings)
                .unwrap()
                .local_email_analysis_enabled
        );
    }

    #[test]
    fn clearing_qualification_revokes_local_analysis_consent() {
        let db = Database::open_in_memory().unwrap();
        db.record_ai_qualification(&"b".repeat(64), "build 10434", 1)
            .unwrap();
        db.save_settings(Settings {
            local_email_analysis_enabled: true,
            ..Settings::default()
        })
        .unwrap();
        db.clear_ai_qualification().unwrap();
        assert!(!db.settings().unwrap().local_email_analysis_enabled);
        assert!(!db
            .has_ai_qualification(&"b".repeat(64), "build 10434", 1)
            .unwrap());
    }

    #[test]
    fn stored_analysis_omits_source_evidence_and_offsets() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account(&ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        })
        .unwrap();
        db.apply_message_page(
            "a",
            "mail_inbox",
            &[MessageMetadata {
                id: "m1".into(),
                conversation_id: None,
                sender: None,
                subject: Some("Metadata subject".into()),
                received_date_time: None,
                sent_date_time: None,
                web_link: None,
                is_read: false,
                removed: None,
            }],
            None,
        )
        .unwrap();
        let source = "Please secret raw excerpt today";
        let extraction = ai::parse_and_validate_extraction(
            source,
            br#"{"schemaVersion":1,"classification":"task","classificationConfidence":0.95,"urgency":"normal","summary":"An item needs attention.","tasks":[{"title":"Handle the item","dueAt":null,"confidence":0.96,"evidence":{"byteStart":7,"byteEnd":25,"quote":"secret raw excerpt"}}],"appointments":[],"waitingFor":[]}"#,
        )
        .unwrap();
        let plan = rules::evaluate(&extraction);
        db.save_message_analysis("a", "m1", &extraction, &plan, &"c".repeat(64))
            .unwrap();
        let recent = db.recent_messages("a", 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].analyzed);
        let connection = db.connection.lock().unwrap();
        let stored: String = connection
            .query_row(
                "SELECT summary || suggestions_json || review_reasons_json FROM message_analysis WHERE account_id='a' AND provider_id='m1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains("Handle the item"));
        assert!(!stored.contains("secret raw excerpt"));
        assert!(!stored.contains("byteStart"));
        assert!(!stored.contains("byteEnd"));
    }

    #[test]
    fn review_items_expose_only_sanitized_derived_data() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account(&ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        })
        .unwrap();
        db.apply_message_page(
            "a",
            "mail_inbox",
            &[MessageMetadata {
                id: "m1".into(),
                conversation_id: None,
                sender: None,
                subject: Some("School information".into()),
                received_date_time: Some("2026-08-22T08:00:00Z".into()),
                sent_date_time: None,
                web_link: Some("https://outlook.office.com/mail/deeplink/read/m1".into()),
                is_read: false,
                removed: None,
            }],
            None,
        )
        .unwrap();
        let source = "Please return the form";
        let extraction = ai::parse_and_validate_extraction(
            source,
            br#"{"schemaVersion":1,"classification":"school","classificationConfidence":1.0,"urgency":"low","summary":"A school form needs attention.","tasks":[{"title":"Return the form","dueAt":null,"confidence":0.96,"evidence":{"byteStart":7,"byteEnd":22,"quote":"return the form"}}],"appointments":[],"waitingFor":[]}"#,
        )
        .unwrap();
        let plan = rules::RulePlan {
            disposition: rules::PlanDisposition::Review,
            proposals: vec![rules::Proposal::Task { candidate_index: 0 }],
            review_reasons: vec![rules::ReviewReason::AmbiguousTemporalExpression],
        };
        db.save_message_analysis("a", "m1", &extraction, &plan, &"d".repeat(64))
            .unwrap();

        let items = db.review_items(20).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(db.pending_review_count().unwrap(), 1);
        assert_eq!(items[0].summary, "A school form needs attention.");
        assert_eq!(items[0].review_reasons, ["ambiguousTemporalExpression"]);
        assert_eq!(
            items[0].suggestions,
            [ReviewSuggestion::Task {
                title: "Return the form".into(),
                due_at: None,
            }]
        );
        let encoded = serde_json::to_string(&items).unwrap();
        assert!(!encoded.contains("return the form\"}}"));
        assert!(!encoded.contains("byteStart"));
        assert!(!encoded.contains("evidence"));

        let first = db.decide_review("a", "m1", "accept").unwrap();
        let repeated = db.decide_review("a", "m1", "accept").unwrap();
        assert_eq!(first, repeated);
        assert!(db.review_items(20).unwrap().is_empty());
        assert_eq!(db.pending_review_count().unwrap(), 0);
        assert_eq!(db.review_history(20).unwrap(), [first.clone()]);
        let projected = db.local_items(false).unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].kind, "task");
        assert_eq!(projected[0].title, "Return the form");
        assert_eq!(projected[0].category, "school");
        assert_eq!(projected[0].account_id, "a");
        assert_eq!(projected[0].provider_id, "m1");
        assert!(db
            .queue_review_action_proposals("a", "m1")
            .unwrap()
            .is_empty());
        let provenance = db.local_item_provenance(projected[0].id).unwrap();
        assert_eq!(provenance.provider, "microsoft");
        assert_eq!(
            provenance.source_subject.as_deref(),
            Some("School information")
        );
        assert!(provenance.source_available);
        assert_eq!(
            db.local_item_source_link(projected[0].id).unwrap(),
            "https://outlook.office.com/mail/deeplink/read/m1"
        );
        let rescheduled = db
            .transition_local_item(
                projected[0].id,
                "event-reschedule-0001",
                "reschedule",
                Some("2026-08-25T09:00:00+00:00"),
            )
            .unwrap();
        assert_eq!(rescheduled.lifecycle_state, "active");
        assert_eq!(
            rescheduled.scheduled_at.as_deref(),
            Some("2026-08-25T09:00:00+00:00")
        );
        assert!(db
            .notification_previews("2026-08-25T06:00:00+00:00")
            .unwrap()
            .is_empty());
        db.save_settings(Settings {
            morning_summary_enabled: true,
            quiet_hours_end_minute: 8 * 60,
            ..Settings::default()
        })
        .unwrap();
        let previews = db
            .notification_previews("2026-08-25T06:00:00+00:00")
            .unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(
            previews[0].kind,
            notifications::NotificationKind::MorningSummary
        );
        assert_eq!(
            previews[0].deliver_at.to_rfc3339(),
            "2026-08-25T08:00:00+00:00"
        );
        assert!(previews[0].quiet_hours_applied);
        assert_eq!(previews[0].body, "1 local item scheduled today.");
        let proposals = db.queue_review_action_proposals("a", "m1").unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].action_kind, "notification");
        assert_eq!(proposals[0].sensitivity, "routine");
        assert_eq!(proposals[0].state, "pending");
        assert_eq!(
            db.queue_review_action_proposals("a", "m1").unwrap(),
            proposals
        );
        assert_eq!(
            db.focused_local_items(
                "today",
                "2026-08-25T00:00:00+00:00",
                "2026-08-26T00:00:00+00:00"
            )
            .unwrap(),
            [rescheduled.clone()]
        );
        assert_eq!(
            db.focused_local_items(
                "kids",
                "2026-08-25T00:00:00+00:00",
                "2026-08-26T00:00:00+00:00"
            )
            .unwrap(),
            [rescheduled]
        );
        assert!(db
            .focused_local_items(
                "work",
                "2026-08-25T00:00:00+00:00",
                "2026-08-26T00:00:00+00:00"
            )
            .unwrap()
            .is_empty());
        let snoozed = db
            .transition_local_item(
                projected[0].id,
                "event-snooze-0000001",
                "snooze",
                Some("2026-08-26T10:00:00Z"),
            )
            .unwrap();
        assert_eq!(snoozed.lifecycle_state, "snoozed");
        assert_eq!(
            db.transition_local_item(
                projected[0].id,
                "event-snooze-0000001",
                "snooze",
                Some("2026-08-26T10:00:00Z"),
            )
            .unwrap(),
            snoozed
        );
        let completed = db
            .transition_local_item(projected[0].id, "event-complete-00001", "complete", None)
            .unwrap();
        assert_eq!(completed.lifecycle_state, "completed");
        assert!(db
            .focused_local_items(
                "kids",
                "2026-08-25T00:00:00+00:00",
                "2026-08-26T00:00:00+00:00"
            )
            .unwrap()
            .is_empty());
        assert_eq!(db.local_item_events(20).unwrap().len(), 3);
        let undone = db.undo_local_item(projected[0].id).unwrap();
        assert_eq!(undone.status, "undone");
        assert_eq!(db.undo_local_item(projected[0].id).unwrap(), undone);
        assert!(db.local_items(false).unwrap().is_empty());
        assert_eq!(db.local_items(true).unwrap(), [undone]);
        assert_eq!(db.review_history(20).unwrap(), [first]);
        assert!(matches!(
            db.decide_review("a", "m1", "ignore"),
            Err(DatabaseError::ReviewAlreadyDecided)
        ));
        assert!(matches!(
            db.decide_review("a", "m1", "execute"),
            Err(DatabaseError::InvalidReviewDecision)
        ));
    }

    #[test]
    fn policy_projection_is_reversible_all_or_nothing_and_audited() {
        fn seeded_database(policy: AutomationPolicy) -> Database {
            let db = Database::open_in_memory().unwrap();
            db.upsert_account(&ConnectedAccount {
                id: "a".into(),
                provider: "microsoft".into(),
                display_name: "A".into(),
                email_address: "a@example.com".into(),
                tenant_id: None,
            })
            .unwrap();
            db.apply_message_page(
                "a",
                "mail_inbox",
                &[MessageMetadata {
                    id: "m1".into(),
                    conversation_id: None,
                    sender: None,
                    subject: Some("Routine reminder".into()),
                    received_date_time: Some("2026-08-31T08:00:00Z".into()),
                    sent_date_time: None,
                    web_link: None,
                    is_read: false,
                    removed: None,
                }],
                None,
            )
            .unwrap();
            db.save_settings(Settings {
                automation_policy: policy,
                ..Settings::default()
            })
            .unwrap();
            db
        }

        let task = ai::parse_and_validate_extraction(
            "Please return the form",
            br#"{"schemaVersion":1,"classification":"task","classificationConfidence":0.99,"urgency":"normal","summary":"A routine form needs attention.","tasks":[{"title":"Return the form","dueAt":null,"confidence":0.99,"evidence":{"byteStart":7,"byteEnd":22,"quote":"return the form"}}],"appointments":[],"waitingFor":[]}"#,
        )
        .unwrap();
        let task_plan = rules::RulePlan {
            disposition: rules::PlanDisposition::Suggestions,
            proposals: vec![rules::Proposal::Task { candidate_index: 0 }],
            review_reasons: vec![],
        };

        let balanced = seeded_database(AutomationPolicy::Balanced);
        balanced
            .save_message_analysis("a", "m1", &task, &task_plan, &"e".repeat(64))
            .unwrap();
        assert_eq!(
            balanced
                .apply_policy_local_projection("a", "m1", &task, &task_plan)
                .unwrap(),
            1
        );
        assert_eq!(balanced.local_items(false).unwrap().len(), 1);
        assert!(balanced.review_items(20).unwrap().is_empty());
        let audit = balanced.automation_audit_history(10).unwrap();
        assert_eq!(audit[0].decision, "eligible");
        assert_eq!(audit[0].reason_code, "policy_allows_routine_action");

        let conservative = seeded_database(AutomationPolicy::Conservative);
        conservative
            .save_message_analysis("a", "m1", &task, &task_plan, &"f".repeat(64))
            .unwrap();
        assert_eq!(
            conservative
                .apply_policy_local_projection("a", "m1", &task, &task_plan)
                .unwrap(),
            0
        );
        assert!(conservative.local_items(false).unwrap().is_empty());
        let review = conservative.review_items(20).unwrap();
        assert_eq!(review.len(), 1);
        assert_eq!(
            review[0].review_reasons,
            ["automationPolicyRequiresDecision"]
        );
        let audit = conservative.automation_audit_history(10).unwrap();
        assert_eq!(audit[0].decision, "confirmation_required");
        assert_eq!(audit[0].reason_code, "conservative_policy");

        let mixed = ai::parse_and_validate_extraction(
            "Return form; meeting is proposed",
            br#"{"schemaVersion":1,"classification":"task","classificationConfidence":0.99,"urgency":"normal","summary":"A task and tentative meeting were found.","tasks":[{"title":"Return form","dueAt":null,"confidence":0.99,"evidence":{"byteStart":0,"byteEnd":11,"quote":"Return form"}}],"appointments":[{"title":"Proposed meeting","startAt":null,"endAt":null,"confirmed":false,"confidence":0.99,"evidence":{"byteStart":13,"byteEnd":32,"quote":"meeting is proposed"}}],"waitingFor":[]}"#,
        )
        .unwrap();
        let mixed_plan = rules::RulePlan {
            disposition: rules::PlanDisposition::Suggestions,
            proposals: vec![
                rules::Proposal::Task { candidate_index: 0 },
                rules::Proposal::Appointment { candidate_index: 0 },
            ],
            review_reasons: vec![],
        };
        let all_or_nothing = seeded_database(AutomationPolicy::Balanced);
        all_or_nothing
            .save_message_analysis("a", "m1", &mixed, &mixed_plan, &"1".repeat(64))
            .unwrap();
        assert_eq!(
            all_or_nothing
                .apply_policy_local_projection("a", "m1", &mixed, &mixed_plan)
                .unwrap(),
            0
        );
        assert!(all_or_nothing.local_items(false).unwrap().is_empty());
        let audit = all_or_nothing.automation_audit_history(10).unwrap();
        assert_eq!(audit.len(), 2);
        assert!(audit.iter().any(|entry| {
            entry.decision == "review" && entry.reason_code == "unconfirmed_calendar_fact"
        }));
    }

    #[test]
    fn family_display_credentials_are_digest_only_scoped_and_revocable() {
        let db = Database::open_in_memory().unwrap();
        let digest = [7_u8; 32];
        let record = db
            .register_family_display(
                "display-abcdefghijklmnop",
                "Kitchen",
                &digest,
                &display_api::DisplayPolicy::default(),
                "display-paired-event-0001",
            )
            .unwrap();
        assert_eq!(record.display_name, "Kitchen");
        assert!(!record.revoked);
        assert!(db
            .authenticate_family_display(&record.id, &digest, "2026-09-01T08:00:00+01:00")
            .unwrap());
        assert!(!db
            .authenticate_family_display(&record.id, &[8_u8; 32], "2026-09-01T08:01:00+01:00")
            .unwrap());
        let listed = db.family_displays().unwrap();
        assert_eq!(
            listed[0].last_seen_at.as_deref(),
            Some("2026-09-01T08:00:00+01:00")
        );

        db.revoke_family_display(&record.id, "display-revoked-event-0001")
            .unwrap();
        assert!(!db
            .authenticate_family_display(&record.id, &digest, "2026-09-01T08:02:00+01:00")
            .unwrap());
        assert!(db.family_displays().unwrap()[0].revoked);

        let connection = db.connection.lock().unwrap();
        let schema: String = connection
            .query_row(
                "SELECT group_concat(sql,' ') FROM sqlite_master WHERE name IN ('family_displays','family_display_events')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        for forbidden in ["token TEXT", "email", "provider", "body", "subject"] {
            assert!(!schema
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase()));
        }
        let stored: Vec<u8> = connection
            .query_row(
                "SELECT token_sha256 FROM family_displays WHERE id=?1",
                [&record.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, digest);
        assert!(connection
            .execute("DELETE FROM family_display_events", [])
            .is_err());
    }

    #[test]
    fn family_snapshot_projects_only_the_items_privacy_policy_allows() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account(&ConnectedAccount {
            id: "a".into(),
            provider: "microsoft".into(),
            display_name: "A".into(),
            email_address: "a@example.com".into(),
            tenant_id: None,
        })
        .unwrap();
        db.apply_message_page(
            "a",
            "mail_inbox",
            &[MessageMetadata {
                id: "m1".into(),
                conversation_id: None,
                sender: None,
                subject: Some("Private task".into()),
                received_date_time: Some("2026-09-01T08:00:00Z".into()),
                sent_date_time: None,
                web_link: None,
                is_read: false,
                removed: None,
            }],
            None,
        )
        .unwrap();
        let extraction = ai::parse_and_validate_extraction(
            "Please return the form",
            br#"{"schemaVersion":1,"classification":"task","classificationConfidence":0.99,"urgency":"normal","summary":"A routine task needs attention.","tasks":[{"title":"Return the private form","dueAt":null,"confidence":0.99,"evidence":{"byteStart":7,"byteEnd":22,"quote":"return the form"}}],"appointments":[],"waitingFor":[]}"#,
        )
        .unwrap();
        let plan = rules::RulePlan {
            disposition: rules::PlanDisposition::Suggestions,
            proposals: vec![rules::Proposal::Task { candidate_index: 0 }],
            review_reasons: vec![],
        };
        db.save_message_analysis("a", "m1", &extraction, &plan, &"2".repeat(64))
            .unwrap();
        assert_eq!(
            db.apply_policy_local_projection("a", "m1", &extraction, &plan)
                .unwrap(),
            1
        );
        let item_id = db.local_items(false).unwrap()[0].id;
        db.register_family_display(
            "display-abcdefghijklmnop",
            "Kitchen",
            &[3_u8; 32],
            &display_api::DisplayPolicy::default(),
            "display-paired-event-0002",
        )
        .unwrap();

        let private = db
            .family_display_snapshot(
                "display-abcdefghijklmnop",
                "2026-09-02T09:00:00+01:00",
                display_api::DisplayMode::Today,
            )
            .unwrap();
        assert_eq!(private.items.len(), 1);
        assert_eq!(private.items[0].title, "Reminder");
        assert_eq!(
            private.items[0].detail_level,
            display_api::DetailLevel::Generic
        );

        db.set_local_item_display_privacy(
            item_id,
            display_api::PrivacyProfile::PublicFamily,
            "display-privacy-event-0001",
        )
        .unwrap();
        let public = db
            .family_display_snapshot(
                "display-abcdefghijklmnop",
                "2026-09-02T09:00:00+01:00",
                display_api::DisplayMode::Today,
            )
            .unwrap();
        assert_eq!(public.items[0].title, "Return the private form");

        db.set_local_item_display_privacy(
            item_id,
            display_api::PrivacyProfile::Sensitive,
            "display-privacy-event-0002",
        )
        .unwrap();
        let sensitive = db
            .family_display_snapshot(
                "display-abcdefghijklmnop",
                "2026-09-02T09:00:00+01:00",
                display_api::DisplayMode::Today,
            )
            .unwrap();
        assert!(sensitive.items.is_empty());
    }

    #[test]
    fn family_display_service_is_default_off_validated_and_append_audited() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.family_display_service_config().unwrap(),
            FamilyDisplayServiceConfig {
                enabled: false,
                bind_address: None,
                certificate_sha256: None,
            }
        );
        let digest = [7_u8; 32];
        let enabled = db
            .configure_family_display_service(
                true,
                Some("192.168.1.20:8765"),
                Some(&digest),
                "display-service-event-0001",
            )
            .unwrap();
        assert!(enabled.enabled);
        assert_eq!(enabled.certificate_sha256, Some("07".repeat(32)));
        assert!(matches!(
            db.configure_family_display_service(
                true,
                Some("0.0.0.0:8765"),
                Some(&digest),
                "display-service-event-0002",
            ),
            Err(DatabaseError::InvalidFamilyDisplay)
        ));
        let disabled = db
            .configure_family_display_service(
                false,
                Some("192.168.1.20:8765"),
                Some(&digest),
                "display-service-event-0003",
            )
            .unwrap();
        assert!(!disabled.enabled);
        let connection = db.connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM family_display_service_events",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        assert!(connection
            .execute("DELETE FROM family_display_service_events", [])
            .is_err());
        let schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='family_display_service_config'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        for forbidden in ["private_key", "token", "credential", "certificate_der"] {
            assert!(!schema.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn backup_snapshot_is_consistent_private_and_never_overwrites() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let db = Database::open_in_memory().unwrap();
        let settings = db.settings().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("snapshot.db");
        db.create_consistent_backup_snapshot(&snapshot).unwrap();
        Database::validate_backup_snapshot(&snapshot).unwrap();
        let restored = Database::open(&snapshot).unwrap();
        assert_eq!(restored.settings().unwrap(), settings);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&snapshot).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(matches!(
            db.create_consistent_backup_snapshot(&snapshot),
            Err(DatabaseError::InvalidBackupDestination)
        ));
        let truncated = directory.path().join("truncated.db");
        std::fs::write(&truncated, b"SQLite format 3\0").unwrap();
        assert!(Database::validate_backup_snapshot(&truncated).is_err());
    }

    #[test]
    fn display_timestamps_normalize_unambiguous_legacy_wall_time_and_reject_junk() {
        let normalized = normalize_display_timestamp("2026-01-15T10:30:00").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&normalized).is_ok());
        assert_eq!(
            normalize_display_timestamp("2026-01-15T10:30:00Z").unwrap(),
            "2026-01-15T10:30:00+00:00"
        );
        assert!(normalize_display_timestamp("tomorrow morning").is_none());
    }
}
