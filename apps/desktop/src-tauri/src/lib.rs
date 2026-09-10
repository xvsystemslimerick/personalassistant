use ai::{AiCapabilities, DownloadProgress};
use assistant_core::Settings;
#[cfg(target_os = "macos")]
use base64::{engine::general_purpose::STANDARD, Engine as _};
use database::{ConnectedAccount, Database, SyncRunSummary};
use email::{
    graph::{
        initial_calendar_url, initial_mail_delta_url, CalendarCreateRequest, CalendarEvent,
        CalendarUpdateRequest, GraphClient, GraphError, MailFolder, MessageMetadata,
        ReplyToMessageRequest, UserProfile,
    },
    oauth,
    secrets::{MacKeychain, SecretStore},
    GRAPH_BASE_URL,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager, Runtime};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use zeroize::Zeroize;

#[cfg(target_os = "macos")]
mod macos_notifications;
mod restore;

#[derive(Clone)]
struct AppState {
    database: Arc<Database>,
    sync_cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    model_directory: std::path::PathBuf,
    model_download_active: Arc<AtomicBool>,
    worker_evaluation_active: Arc<AtomicBool>,
    message_analysis_active: Arc<AtomicBool>,
    family_display_listener: Arc<tokio::sync::Mutex<Option<display_server::ListenerHandle>>>,
    family_display_pairing: Arc<display_server::PairingCoordinator>,
    data_directory: std::path::PathBuf,
    startup_restore_status: Option<String>,
    updater_active: Arc<AtomicBool>,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Result<Settings, String> {
    state.database.settings().map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_encrypted_backup(
    destination: String,
    password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let destination = std::path::PathBuf::from(destination);
    if destination.extension().and_then(|value| value.to_str()) != Some("pabackup")
        || destination.exists()
        || !destination.parent().is_some_and(std::path::Path::is_dir)
    {
        return Err("Choose a new .pabackup file in an existing folder".into());
    }
    let database = state.database.clone();
    let data_directory = state.data_directory.clone();
    tokio::task::spawn_blocking(move || {
        let snapshot =
            data_directory.join(format!(".backup-snapshot-{}.db", rand::random::<u64>()));
        let result = (|| {
            database
                .create_consistent_backup_snapshot(&snapshot)
                .map_err(sanitized)?;
            let database = zeroize::Zeroizing::new(std::fs::read(&snapshot).map_err(sanitized)?);
            let mut password = password.into_bytes();
            let encrypted = backup::create_encrypted_backup(
                &database,
                &mut password,
                &chrono::Utc::now().to_rfc3339(),
            )
            .map_err(sanitized)?;
            write_new_private_file(&destination, &encrypted)?;
            Ok(format!(
                "Encrypted backup created · {} bytes",
                encrypted.len()
            ))
        })();
        let _ = std::fs::remove_file(snapshot);
        result
    })
    .await
    .map_err(|_| "The encrypted backup worker failed safely".to_string())?
}

#[tauri::command]
async fn verify_encrypted_backup(
    source: String,
    password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let source = std::path::PathBuf::from(source);
    if source.extension().and_then(|value| value.to_str()) != Some("pabackup") {
        return Err("Choose a Personal Assistant .pabackup file".into());
    }
    let data_directory = state.data_directory.clone();
    tokio::task::spawn_blocking(move || {
        let container = read_bounded_backup_file(&source)?;
        let mut password = password.into_bytes();
        let database = zeroize::Zeroizing::new(
            backup::decrypt_backup(&container, &mut password).map_err(sanitized)?,
        );
        let snapshot =
            data_directory.join(format!(".backup-verification-{}.db", rand::random::<u64>()));
        let result = (|| {
            write_new_private_file(&snapshot, &database)?;
            Database::validate_backup_snapshot(&snapshot).map_err(sanitized)?;
            Ok(format!(
                "Encrypted backup verified · {} database bytes · no data restored",
                database.len()
            ))
        })();
        let _ = std::fs::remove_file(snapshot);
        result
    })
    .await
    .map_err(|_| "The encrypted backup verification worker failed safely".to_string())?
}

#[tauri::command]
async fn prepare_encrypted_restore(
    source: String,
    password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let source = std::path::PathBuf::from(source);
    if source.extension().and_then(|value| value.to_str()) != Some("pabackup") {
        return Err("Choose a Personal Assistant .pabackup file".into());
    }
    let data_directory = state.data_directory.clone();
    let database = state.database.clone();
    tokio::task::spawn_blocking(move || {
        let container = read_bounded_backup_file(&source)?;
        let mut password = password.into_bytes();
        let restored_database = zeroize::Zeroizing::new(
            backup::decrypt_backup(&container, &mut password).map_err(sanitized)?,
        );
        let restore_id = format!("{:016x}", rand::random::<u64>());
        restore::stage(&data_directory, &database, &restored_database, &restore_id)
            .map_err(sanitized)?;
        Ok("Encrypted backup authenticated and staged. Restarting to restore it safely.".into())
    })
    .await
    .map_err(|_| "The encrypted restore worker failed safely".to_string())?
}

#[tauri::command]
fn restart_for_restore(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !state.data_directory.join(".restore-pending.json").is_file() {
        return Err("No authenticated restore is ready".into());
    }
    app.request_restart();
    Ok(())
}

#[tauri::command]
fn restore_status(state: tauri::State<'_, AppState>) -> Option<String> {
    state.startup_restore_status.clone()
}

const UPDATE_ENDPOINT: &str =
    "https://github.com/xvsystemslimerick/personalassistant/releases/latest/download/latest.json";

fn embedded_updater_public_key() -> Option<&'static str> {
    let value = include_str!("../updater.pub").trim();
    (!value.is_empty()).then_some(value)
}

fn updater_enabled(disable_flag: Option<&str>) -> bool {
    disable_flag != Some("1")
}

fn updater_public_key() -> Option<&'static str> {
    updater_enabled(option_env!("PA_DISABLE_UPDATER"))
        .then(embedded_updater_public_key)
        .flatten()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdaterStatus {
    configured: bool,
    current_version: String,
}

#[derive(serde::Serialize)]
struct AvailableUpdate {
    version: String,
}

#[tauri::command]
fn updater_status(app: tauri::AppHandle) -> UpdaterStatus {
    UpdaterStatus {
        configured: updater_public_key().is_some(),
        current_version: app.package_info().version.to_string(),
    }
}

fn update_builder(app: &tauri::AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    if updater_public_key().is_none() {
        return Err("Signed updates are not configured in this build".into());
    }
    let endpoint = UPDATE_ENDPOINT
        .parse()
        .map_err(|_| "The signed update endpoint is invalid".to_string())?;
    app.updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|_| "The signed update endpoint is invalid".to_string())?
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "The signed updater could not be initialized".to_string())
}

#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<AvailableUpdate>, String> {
    let update = update_builder(&app)?
        .check()
        .await
        .map_err(|_| "The signed update check failed safely".to_string())?;
    Ok(update.map(|value| AvailableUpdate {
        version: value.version,
    }))
}

#[tauri::command]
async fn install_update(
    expected_version: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !valid_expected_update_version(&expected_version)
        || state.data_directory.join(".restore-pending.json").exists()
    {
        return Err("The signed update request is invalid".into());
    }
    if state.updater_active.swap(true, Ordering::SeqCst) {
        return Err("A signed update is already being installed".into());
    }
    let result: Result<(), String> = async {
        let update = update_builder(&app)?
            .check()
            .await
            .map_err(|_| "The signed update check failed safely".to_string())?
            .ok_or_else(|| "The selected signed update is no longer available".to_string())?;
        if update.version != expected_version {
            return Err("The selected signed update changed; check again before installing".into());
        }
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|_| "The signed update was not installed".to_string())?;
        Ok(())
    }
    .await;
    state.updater_active.store(false, Ordering::SeqCst);
    result?;
    app.request_restart();
    Ok(())
}

fn valid_expected_update_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn read_bounded_backup_file(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    const MAX_CONTAINER_BYTES: u64 = 513 * 1024 * 1024;
    if path
        .symlink_metadata()
        .map_err(sanitized)?
        .file_type()
        .is_symlink()
    {
        return Err("The encrypted backup file is invalid".into());
    }
    let file = std::fs::File::open(path).map_err(sanitized)?;
    let metadata = file.metadata().map_err(sanitized)?;
    if !metadata.is_file() || metadata.len() > MAX_CONTAINER_BYTES {
        return Err("The encrypted backup file is invalid".into());
    }
    let mut value = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(MAX_CONTAINER_BYTES + 1)
        .read_to_end(&mut value)
        .map_err(sanitized)?;
    if value.len() as u64 > MAX_CONTAINER_BYTES {
        return Err("The encrypted backup file is invalid".into());
    }
    Ok(value)
}

#[tauri::command]
async fn save_settings(
    settings: Settings,
    state: tauri::State<'_, AppState>,
) -> Result<Settings, String> {
    #[cfg(target_os = "macos")]
    if settings.notification_delivery_enabled
        && macos_notifications::permission_status().await? != "granted"
    {
        return Err("macOS notification permission is required before delivery consent".into());
    }
    if settings.local_email_analysis_enabled {
        let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
        let qualified = capabilities.lifecycle == ai::ModelLifecycle::Installed
            && state
                .database
                .has_ai_qualification(
                    &capabilities.recommendation.artifact.sha256,
                    ai::QUALIFIED_RUNTIME_BUILD,
                    ai::PERSISTENT_CORPUS_VERSION,
                )
                .map_err(sanitized)?;
        if !qualified {
            return Err(
                "Run and pass the persistent worker evaluation before enabling local email analysis"
                    .into(),
            );
        }
    }
    let saved = state
        .database
        .save_settings(settings)
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    reconcile_notification_schedule(&state).await?;
    Ok(saved)
}

#[tauri::command]
fn ai_capabilities(state: tauri::State<'_, AppState>) -> Result<AiCapabilities, String> {
    ai::detect(&state.model_directory).map_err(sanitized)
}

#[tauri::command]
async fn download_ai_model<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<AiCapabilities, String> {
    if state.model_download_active.swap(true, Ordering::SeqCst) {
        return Err("A private model download is already running".into());
    }
    let result = async {
        let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
        if !capabilities.recommendation.eligible {
            return Err("Not enough free storage for the recommended private model".into());
        }
        let artifact = capabilities.recommendation.artifact;
        ai::download_model(
            &state.model_directory,
            &artifact,
            |progress: DownloadProgress| {
                let _ = app.emit("ai-model-download-progress", progress);
            },
        )
        .await
        .map_err(sanitized)?;
        ai::detect(&state.model_directory).map_err(sanitized)
    }
    .await;
    state.model_download_active.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
async fn remove_ai_model(state: tauri::State<'_, AppState>) -> Result<AiCapabilities, String> {
    if state.model_download_active.load(Ordering::SeqCst) {
        return Err("The private model cannot be removed while a download is running".into());
    }
    let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
    ai::remove_model(
        &state.model_directory,
        &capabilities.recommendation.artifact,
    )
    .await
    .map_err(sanitized)?;
    state.database.clear_ai_qualification().map_err(sanitized)?;
    ai::detect(&state.model_directory).map_err(sanitized)
}

#[tauri::command]
async fn test_ai_runtime<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<ai::RuntimeHealth, String> {
    let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
    if capabilities.lifecycle != ai::ModelLifecycle::Installed {
        return Err("Install and verify the private model before testing the runtime".into());
    }
    let runtime_directory = app
        .path()
        .resource_dir()
        .map_err(sanitized)?
        .join("resources/llama-runtime/macos-arm64");
    let model_directory = state.model_directory.clone();
    let artifact = capabilities.recommendation.artifact;
    tauri::async_runtime::spawn_blocking(move || {
        ai::health_check(&runtime_directory, &model_directory, &artifact).map_err(sanitized)
    })
    .await
    .map_err(sanitized)?
}

#[tauri::command]
async fn test_ai_evaluation<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<ai::EvaluationReport, String> {
    let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
    if capabilities.lifecycle != ai::ModelLifecycle::Installed {
        return Err("Install and verify the private model before running evaluation".into());
    }
    let runtime_directory = app
        .path()
        .resource_dir()
        .map_err(sanitized)?
        .join("resources/llama-runtime/macos-arm64");
    let model_directory = state.model_directory.clone();
    let artifact = capabilities.recommendation.artifact;
    tauri::async_runtime::spawn_blocking(move || {
        ai::run_evaluation_corpus(&runtime_directory, &model_directory, &artifact)
            .map_err(sanitized)
    })
    .await
    .map_err(sanitized)?
}

#[tauri::command]
async fn test_persistent_ai_evaluation<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<ai::EvaluationReport, String> {
    if state.worker_evaluation_active.swap(true, Ordering::SeqCst) {
        return Err("The persistent worker evaluation is already running".into());
    }
    let result = async {
        let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
        if capabilities.lifecycle != ai::ModelLifecycle::Installed {
            return Err("Install and verify the private model before running evaluation".into());
        }
        let resource_dir = app.path().resource_dir().map_err(sanitized)?;
        let runtime = resource_dir.join("resources/llama-runtime/macos-arm64");
        let worker = resource_dir
            .join("resources/inference-worker/macos-arm64")
            .join("personal-assistant-inference-worker");
        let model = ai::verify_model(
            &state.model_directory,
            &capabilities.recommendation.artifact,
        )
        .map_err(sanitized)?;
        let model_sha256 = capabilities.recommendation.artifact.sha256.clone();
        let report = tauri::async_runtime::spawn_blocking(move || {
            run_persistent_evaluation(&worker, &runtime, &model)
        })
        .await
        .map_err(sanitized)??;
        if report.all_passed {
            state
                .database
                .record_ai_qualification(
                    &model_sha256,
                    ai::QUALIFIED_RUNTIME_BUILD,
                    ai::PERSISTENT_CORPUS_VERSION,
                )
                .map_err(sanitized)?;
        }
        Ok(report)
    }
    .await;
    state
        .worker_evaluation_active
        .store(false, Ordering::SeqCst);
    result
}

fn run_persistent_evaluation(
    worker: &std::path::Path,
    runtime: &std::path::Path,
    model: &std::path::Path,
) -> Result<ai::EvaluationReport, String> {
    use inference_worker::{
        process_adapter::{AdapterEvent, ProcessAdapter, ProcessConfig},
        supervisor::{Job, SubmitOutcome},
        ErrorCode, Response,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CASE_DEADLINE_MS: u64 = 420_000;
    const CASE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(425);

    if !worker.is_file() {
        return Err("The packaged inference worker is unavailable".into());
    }
    let config = ProcessConfig::new(worker)
        .env("PA_INFERENCE_RUNTIME_DIR", runtime)
        .env("PA_INFERENCE_MODEL_PATH", model)
        .env("PA_INFERENCE_GPU_LAYERS", "99");
    let mut adapter = ProcessAdapter::new(config);
    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    adapter
        .start_and_health_check("persistent_health", Duration::from_secs(120), started_at)
        .map_err(|_| "Persistent worker health check timed out".to_string())?;

    let result = (|| {
        let mut results = Vec::new();
        for case in ai::evaluation_corpus() {
            let request_id = format!("case_{}", case.id.replace('-', "_"));
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let deadline = now + CASE_DEADLINE_MS;
            let outcome = adapter
                .submit(
                    Job {
                        request_id: request_id.clone(),
                        deadline_unix_ms: deadline,
                    },
                    case.source.into(),
                    now,
                )
                .map_err(|_| "Worker evaluation request failed".to_string())?;
            if outcome != SubmitOutcome::Started {
                return Err("Worker evaluation request was not started".into());
            }
            let wait_started = std::time::Instant::now();
            let response = loop {
                let poll_now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                match adapter.poll(poll_now) {
                    Ok(Some(AdapterEvent::Completed { response, .. })) => break response,
                    Ok(Some(AdapterEvent::DeadlineExceeded { .. })) => {
                        return Err("Persistent worker evaluation timed out".into())
                    }
                    Ok(Some(AdapterEvent::WorkerFailed { .. })) => {
                        return Err("Persistent worker stopped unexpectedly".into())
                    }
                    Ok(Some(AdapterEvent::Cancelled { .. })) => {
                        return Err("Persistent worker evaluation was cancelled".into())
                    }
                    Ok(None) if wait_started.elapsed() < CASE_RESPONSE_TIMEOUT => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(None) => {
                        let _ = adapter.cancel(&request_id, poll_now);
                        return Err("Persistent worker evaluation timed out".into());
                    }
                    Err(_) => return Err("Persistent worker protocol failed".into()),
                }
            };
            let result = match response {
                Response::Extracted { payload, .. } => {
                    let extraction = serde_json::from_value::<ai::Extraction>(payload).ok();
                    let passed = extraction.as_ref().is_some_and(|value| {
                        !case.expected_safety_rejection && ai::evaluate_case(case, value)
                    });
                    ai::EvaluationCaseResult {
                        id: case.id.into(),
                        passed,
                        observed_classification: extraction
                            .as_ref()
                            .map(|value| value.classification),
                        task_count: extraction.as_ref().map(|value| value.tasks.len()),
                        appointment_count: extraction
                            .as_ref()
                            .map(|value| value.appointments.len()),
                        waiting_count: extraction.as_ref().map(|value| value.waiting_for.len()),
                        safety_rejected: false,
                        failure_code: (!passed).then(|| "invalid_result".into()),
                    }
                }
                Response::Error {
                    code: ErrorCode::InvalidOutput,
                    ..
                } if case.expected_safety_rejection => ai::EvaluationCaseResult {
                    id: case.id.into(),
                    passed: true,
                    observed_classification: None,
                    task_count: None,
                    appointment_count: None,
                    waiting_count: None,
                    safety_rejected: true,
                    failure_code: None,
                },
                _ => ai::EvaluationCaseResult {
                    id: case.id.into(),
                    passed: false,
                    observed_classification: None,
                    task_count: None,
                    appointment_count: None,
                    waiting_count: None,
                    safety_rejected: false,
                    failure_code: Some("worker_error".into()),
                },
            };
            results.push(result);
        }
        let passed = results.iter().filter(|case| case.passed).count();
        let total = results.len();
        Ok(ai::EvaluationReport {
            passed,
            total,
            all_passed: passed == total,
            cases: results,
        })
    })();
    drop(adapter);
    result
}

fn run_single_message_inference(
    worker: &std::path::Path,
    runtime: &std::path::Path,
    model: &std::path::Path,
    source: String,
) -> Result<ai::Extraction, String> {
    use inference_worker::{
        process_adapter::{AdapterEvent, ProcessAdapter, ProcessConfig},
        supervisor::{Job, SubmitOutcome},
        Response,
    };
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use zeroize::Zeroizing;

    const DEADLINE_MS: u64 = 420_000;
    let validation_source = Zeroizing::new(source.clone());
    let config = ProcessConfig::new(worker)
        .env("PA_INFERENCE_RUNTIME_DIR", runtime)
        .env("PA_INFERENCE_MODEL_PATH", model)
        .env("PA_INFERENCE_GPU_LAYERS", "99");
    let mut adapter = ProcessAdapter::new(config);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    adapter
        .start_and_health_check("message_health", Duration::from_secs(120), now)
        .map_err(|_| "Private worker health check failed".to_string())?;
    let request_id = format!("message_{now}");
    let deadline = now.saturating_add(DEADLINE_MS);
    if adapter
        .submit(
            Job {
                request_id: request_id.clone(),
                deadline_unix_ms: deadline,
            },
            source,
            now,
        )
        .map_err(|_| "Private message analysis could not start".to_string())?
        != SubmitOutcome::Started
    {
        return Err("Private message analysis could not start".into());
    }
    let started = Instant::now();
    let response = loop {
        let poll_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match adapter.poll(poll_now) {
            Ok(Some(AdapterEvent::Completed { response, .. })) => break response,
            Ok(Some(AdapterEvent::DeadlineExceeded { .. })) => {
                return Err("Private message analysis timed out".into())
            }
            Ok(Some(AdapterEvent::WorkerFailed { .. })) | Err(_) => {
                return Err("Private message analysis failed safely".into())
            }
            Ok(Some(AdapterEvent::Cancelled { .. })) => {
                return Err("Private message analysis was cancelled".into())
            }
            Ok(None) if started.elapsed() < Duration::from_millis(DEADLINE_MS + 5_000) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = adapter.cancel(&request_id, poll_now);
                return Err("Private message analysis timed out".into());
            }
        }
    };
    let Response::Extracted { payload, .. } = response else {
        return Err("The selected message requires manual review".into());
    };
    let encoded = Zeroizing::new(
        serde_json::to_vec(&payload)
            .map_err(|_| "Private message analysis output was invalid".to_string())?,
    );
    ai::parse_and_validate_extraction(validation_source.as_str(), &encoded)
        .map_err(|_| "Private message analysis output was invalid".to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MicrosoftConnectionStatus {
    configured: bool,
    accounts: Vec<MicrosoftAccountView>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MicrosoftAccountView {
    id: String,
    display_name: String,
    email_address: String,
    last_sync: Option<SyncRunView>,
    recent_messages: Vec<RecentMessageView>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentMessageView {
    provider_id: String,
    subject: Option<String>,
    occurred_at: Option<String>,
    analyzed: bool,
    has_reply_draft: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageAnalysisResult {
    provider_id: String,
    summary: String,
    classification: ai::Classification,
    urgency: ai::Urgency,
    disposition: rules::PlanDisposition,
    suggestion_count: usize,
    local_projection_count: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncRunView {
    outcome: String,
    finished_at: Option<String>,
    item_count: i64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncResult {
    inbox: usize,
    sent: usize,
    calendar: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HomeDashboard {
    today: Vec<database::LocalItem>,
    todo_count: usize,
    waiting_count: usize,
    calendar_count: usize,
    review_count: i64,
    sync: HomeSyncHealth,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HomeSyncHealth {
    connected_accounts: usize,
    healthy_accounts: usize,
    attention_accounts: usize,
    last_sync_at: Option<String>,
}

#[tauri::command]
fn microsoft_status(
    state: tauri::State<'_, AppState>,
) -> Result<MicrosoftConnectionStatus, String> {
    let accounts = state
        .database
        .accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|a| {
            let last_sync = state
                .database
                .last_sync_run(&a.id)
                .map_err(sanitized)?
                .map(sync_run_view);
            let recent_messages = state
                .database
                .recent_messages(&a.id, 10)
                .map_err(sanitized)?
                .into_iter()
                .map(|message| RecentMessageView {
                    provider_id: message.provider_id,
                    subject: message.subject,
                    occurred_at: message.occurred_at,
                    analyzed: message.analyzed,
                    has_reply_draft: message.has_reply_draft,
                })
                .collect();
            Ok(MicrosoftAccountView {
                id: a.id,
                display_name: a.display_name,
                email_address: a.email_address,
                last_sync,
                recent_messages,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(MicrosoftConnectionStatus {
        configured: microsoft_client_id().is_some(),
        accounts,
    })
}

#[tauri::command]
fn review_items(state: tauri::State<'_, AppState>) -> Result<Vec<database::ReviewItem>, String> {
    state.database.review_items(100).map_err(sanitized)
}

#[tauri::command]
fn review_history(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::ReviewDecision>, String> {
    state.database.review_history(50).map_err(sanitized)
}

#[tauri::command]
fn decide_review(
    account_id: String,
    provider_id: String,
    decision: String,
    state: tauri::State<'_, AppState>,
) -> Result<database::ReviewDecision, String> {
    let result = state
        .database
        .decide_review(&account_id, &provider_id, &decision)
        .map_err(sanitized)?;
    if decision == "accept" {
        state
            .database
            .queue_review_action_proposals(&account_id, &provider_id)
            .map_err(sanitized)?;
    }
    Ok(result)
}

#[tauri::command]
fn local_items(state: tauri::State<'_, AppState>) -> Result<Vec<database::LocalItem>, String> {
    state.database.local_items(true).map_err(sanitized)
}

#[tauri::command]
fn undo_local_item(
    id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<database::LocalItem, String> {
    state.database.undo_local_item(id).map_err(sanitized)
}

#[tauri::command]
fn transition_local_item(
    id: i64,
    event_key: String,
    event_type: String,
    scheduled_at: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<database::LocalItem, String> {
    state
        .database
        .transition_local_item(id, &event_key, &event_type, scheduled_at.as_deref())
        .map_err(sanitized)
}

#[tauri::command]
fn local_item_events(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::LocalItemEvent>, String> {
    state.database.local_item_events(100).map_err(sanitized)
}

#[tauri::command]
fn action_proposals(
    now: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::InertActionProposal>, String> {
    state
        .database
        .action_proposal_queue(&now)
        .map_err(sanitized)
}

#[tauri::command]
fn correspondence_execution_history(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::CorrespondenceExecutionHistory>, String> {
    state
        .database
        .correspondence_execution_history(50)
        .map_err(sanitized)
}

#[tauri::command]
fn automation_audit_history(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::AutomationAuditEntry>, String> {
    state
        .database
        .automation_audit_history(50)
        .map_err(sanitized)
}

#[tauri::command]
fn family_displays(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::FamilyDisplayRecord>, String> {
    state.database.family_displays().map_err(sanitized)
}

#[tauri::command]
fn revoke_family_display(
    display_id: String,
    event_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::FamilyDisplayRecord>, String> {
    state
        .database
        .revoke_family_display(&display_id, &event_key)
        .map_err(sanitized)?;
    state.database.family_displays().map_err(sanitized)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FamilyDisplayServiceStatus {
    enabled: bool,
    running: bool,
    bind_address: Option<String>,
    certificate_sha256: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FamilyDisplayPairingChallenge {
    code: String,
    expires_at_unix: i64,
    host_url: String,
    certificate_sha256: String,
    certificate_der_base64: String,
}

#[tauri::command]
async fn begin_family_display_pairing(
    state: tauri::State<'_, AppState>,
) -> Result<FamilyDisplayPairingChallenge, String> {
    if state.family_display_listener.lock().await.is_none() {
        return Err("Start the encrypted Family Display service before pairing".into());
    }
    let challenge = state
        .family_display_pairing
        .begin(chrono::Utc::now().timestamp())
        .map_err(sanitized)?;
    #[cfg(target_os = "macos")]
    {
        let saved = state
            .database
            .family_display_service_config()
            .map_err(sanitized)?;
        let bind_address = saved
            .bind_address
            .ok_or_else(|| "The Family Display address is unavailable".to_string())?;
        let digest = saved
            .certificate_sha256
            .ok_or_else(|| "The Family Display identity is unavailable".to_string())?;
        let certificate = load_family_display_certificate(&state.data_directory, &digest)?;
        Ok(FamilyDisplayPairingChallenge {
            code: challenge.code,
            expires_at_unix: challenge.expires_at_unix,
            host_url: format!("https://{bind_address}"),
            certificate_sha256: digest,
            certificate_der_base64: STANDARD.encode(certificate),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = challenge;
        Err("Family Display hosting is not available on this platform yet".into())
    }
}

#[tauri::command]
async fn family_display_service_status(
    state: tauri::State<'_, AppState>,
) -> Result<FamilyDisplayServiceStatus, String> {
    let saved = state
        .database
        .family_display_service_config()
        .map_err(sanitized)?;
    let running = state.family_display_listener.lock().await.is_some();
    Ok(FamilyDisplayServiceStatus {
        enabled: saved.enabled,
        running,
        bind_address: saved.bind_address,
        certificate_sha256: saved.certificate_sha256,
    })
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn enable_family_display_service(
    bind_address: String,
    event_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<FamilyDisplayServiceStatus, String> {
    let address = bind_address
        .parse::<std::net::SocketAddr>()
        .map_err(|_| "The Family Display address is invalid".to_string())?;
    let mut listener = state.family_display_listener.lock().await;
    if listener.is_some() {
        return Err("The Family Display service is already running".into());
    }
    let saved = state
        .database
        .family_display_service_config()
        .map_err(sanitized)?;
    if saved
        .bind_address
        .as_deref()
        .is_some_and(|value| value != bind_address)
        && state
            .database
            .family_displays()
            .map_err(sanitized)?
            .iter()
            .any(|value| !value.revoked)
    {
        return Err("Revoke active Family Displays before changing the host address".into());
    }
    let keychain = MacKeychain::named("com.pattobin.personal-assistant", "family-display-tls");
    let identity = if saved.bind_address.as_deref() == Some(bind_address.as_str()) {
        saved
            .certificate_sha256
            .as_deref()
            .map(|digest| load_family_display_identity(&state.data_directory, &keychain, digest))
            .transpose()?
    } else {
        None
    };
    let identity = match identity {
        Some(value) => value,
        None => {
            let value = display_server::generate_tls_identity(address).map_err(sanitized)?;
            let digest = encode_sha256_hex(&value.certificate_sha256);
            write_private_atomic(
                &family_display_certificate_path(&state.data_directory, &digest),
                &value.certificate_der,
            )?;
            let mut encoded_key = zeroize::Zeroizing::new(STANDARD.encode(&value.private_key_der));
            keychain
                .put(&family_display_key_name(&digest), &encoded_key)
                .map_err(sanitized)?;
            encoded_key.zeroize();
            value
        }
    };
    let digest = identity.certificate_sha256;
    let started = display_server::start_listener_with_pairing(
        state.database.clone(),
        display_server::ListenerConfig {
            enabled: true,
            bind_address: address,
            certificate_der: identity.certificate_der,
            private_key_der: identity.private_key_der,
        },
        state.family_display_pairing.clone(),
    )
    .await
    .map_err(sanitized)?;
    if let Err(error) = state.database.configure_family_display_service(
        true,
        Some(&bind_address),
        Some(&digest),
        &event_key,
    ) {
        started.shutdown().await;
        return Err(sanitized(error));
    }
    *listener = Some(started);
    drop(listener);
    family_display_service_status(state).await
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn enable_family_display_service(
    _bind_address: String,
    _event_key: String,
    _state: tauri::State<'_, AppState>,
) -> Result<FamilyDisplayServiceStatus, String> {
    Err("Family Display hosting is not available on this platform yet".into())
}

#[tauri::command]
async fn disable_family_display_service(
    event_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<FamilyDisplayServiceStatus, String> {
    let mut listener = state.family_display_listener.lock().await;
    if let Some(active) = listener.take() {
        active.shutdown().await;
    }
    let saved = state
        .database
        .family_display_service_config()
        .map_err(sanitized)?;
    let digest = saved
        .certificate_sha256
        .as_deref()
        .map(decode_sha256_hex)
        .transpose()?;
    state
        .database
        .configure_family_display_service(
            false,
            saved.bind_address.as_deref(),
            digest.as_ref(),
            &event_key,
        )
        .map_err(sanitized)?;
    drop(listener);
    family_display_service_status(state).await
}

fn decode_sha256_hex(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Family Display identity is invalid".into());
    }
    let mut digest = [0_u8; 32];
    for (index, target) in digest.iter_mut().enumerate() {
        *target = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "Family Display identity is invalid".to_string())?;
    }
    Ok(digest)
}

#[cfg(target_os = "macos")]
fn encode_sha256_hex(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(target_os = "macos")]
fn family_display_certificate_path(
    data_directory: &std::path::Path,
    digest: &str,
) -> std::path::PathBuf {
    data_directory.join(format!("family-display-host-{digest}.der"))
}

#[cfg(target_os = "macos")]
fn family_display_key_name(digest: &str) -> String {
    format!("host-private-key-{digest}")
}

#[cfg(target_os = "macos")]
fn load_family_display_certificate(
    data_directory: &std::path::Path,
    digest_hex: &str,
) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::PermissionsExt;
    let expected = decode_sha256_hex(digest_hex)?;
    let path = family_display_certificate_path(data_directory, digest_hex);
    let metadata = path
        .symlink_metadata()
        .map_err(|_| "The saved Family Display certificate is unavailable".to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("Family Display identity storage is unsafe".into());
    }
    let certificate = std::fs::read(path)
        .map_err(|_| "The saved Family Display certificate is unavailable".to_string())?;
    if certificate.is_empty()
        || certificate.len() > 16 * 1024
        || <[u8; 32]>::from(Sha256::digest(&certificate)) != expected
    {
        return Err("The saved Family Display certificate failed integrity verification".into());
    }
    Ok(certificate)
}

#[cfg(target_os = "macos")]
fn load_family_display_identity(
    data_directory: &std::path::Path,
    keychain: &MacKeychain,
    digest_hex: &str,
) -> Result<display_server::TlsIdentity, String> {
    let expected = decode_sha256_hex(digest_hex)?;
    let certificate_der = load_family_display_certificate(data_directory, digest_hex)?;
    let mut encoded_key = zeroize::Zeroizing::new(
        keychain
            .get(&family_display_key_name(digest_hex))
            .map_err(sanitized)?
            .ok_or_else(|| {
                "The Family Display private key is missing from macOS Keychain".to_string()
            })?,
    );
    let private_key_der = zeroize::Zeroizing::new(
        STANDARD
            .decode(encoded_key.as_bytes())
            .map_err(|_| "The Family Display private key is invalid".to_string())?,
    );
    encoded_key.zeroize();
    if private_key_der.is_empty() || private_key_der.len() > 16 * 1024 {
        return Err("The Family Display private key is invalid".into());
    }
    Ok(display_server::TlsIdentity {
        certificate_der,
        certificate_sha256: expected,
        private_key_der,
    })
}

#[cfg(target_os = "macos")]
fn saved_family_display_listener_config(
    state: &AppState,
) -> Result<Option<display_server::ListenerConfig>, String> {
    let saved = state
        .database
        .family_display_service_config()
        .map_err(sanitized)?;
    if !saved.enabled {
        return Ok(None);
    }
    let address = saved
        .bind_address
        .as_deref()
        .ok_or_else(|| "The saved Family Display address is missing".to_string())?
        .parse::<std::net::SocketAddr>()
        .map_err(|_| "The saved Family Display address is invalid".to_string())?;
    let digest = saved
        .certificate_sha256
        .as_deref()
        .ok_or_else(|| "The saved Family Display identity is missing".to_string())?;
    let keychain = MacKeychain::named("com.pattobin.personal-assistant", "family-display-tls");
    let identity = load_family_display_identity(&state.data_directory, &keychain, digest)?;
    Ok(Some(display_server::ListenerConfig {
        enabled: true,
        bind_address: address,
        certificate_der: identity.certificate_der,
        private_key_der: identity.private_key_der,
    }))
}

#[cfg(target_os = "macos")]
fn write_private_atomic(path: &std::path::Path, value: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if value.is_empty() || value.len() > 16 * 1024 {
        return Err("Family Display identity is invalid".into());
    }
    if path
        .symlink_metadata()
        .is_ok_and(|value| value.file_type().is_symlink())
    {
        return Err("Family Display identity storage is unsafe".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Family Display identity storage is unsafe".to_string())?;
    let temporary = parent.join(format!(".family-display-{}.tmp", rand::random::<u64>()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "Family Display identity could not be stored".to_string())?;
        file.write_all(value)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Family Display identity could not be stored".to_string())?;
        std::fs::rename(&temporary, path)
            .map_err(|_| "Family Display identity could not be stored".to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn write_new_private_file(path: &std::path::Path, value: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|value| value.is_dir())
        .ok_or_else(|| "The backup destination is invalid".to_string())?;
    if value.len() < 64 || value.len() > 513 * 1024 * 1024 || path.exists() {
        return Err("The backup destination is invalid".into());
    }
    let temporary = parent.join(format!(
        ".personal-assistant-backup-{}.tmp",
        rand::random::<u64>()
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "The encrypted backup could not be stored".to_string())?;
        file.write_all(value)
            .and_then(|_| file.sync_all())
            .map_err(|_| "The encrypted backup could not be stored".to_string())?;
        std::fs::hard_link(&temporary, path)
            .map_err(|_| "The encrypted backup could not be stored".to_string())?;
        std::fs::remove_file(&temporary)
            .map_err(|_| "The encrypted backup could not be finalized".to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[tauri::command]
fn calendar_update_candidates(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::CalendarUpdateCandidate>, String> {
    state
        .database
        .calendar_update_candidates(&account_id)
        .map_err(sanitized)
}

#[tauri::command]
fn queue_calendar_update_proposal(
    proposal_key: String,
    audit_event_key: String,
    account_id: String,
    provider_event_id: String,
    proposed_start_at: String,
    proposed_end_at: String,
    state: tauri::State<'_, AppState>,
) -> Result<database::InertActionProposal, String> {
    state
        .database
        .queue_calendar_update_proposal(
            &proposal_key,
            &audit_event_key,
            &account_id,
            &provider_event_id,
            &proposed_start_at,
            &proposed_end_at,
        )
        .map_err(sanitized)
}

#[tauri::command]
fn save_reply_draft(
    draft_key: String,
    revision_key: String,
    account_id: String,
    provider_message_id: String,
    mut comment: String,
    state: tauri::State<'_, AppState>,
) -> Result<database::ReplyDraftMetadata, String> {
    let trimmed = comment.trim();
    if trimmed.is_empty()
        || trimmed.len() > 16 * 1024
        || trimmed
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        || !state
            .database
            .message_available(&account_id, &provider_message_id)
            .map_err(sanitized)?
    {
        comment.zeroize();
        return Err("Reply draft failed validation".into());
    }
    let mut normalized = trimmed.to_owned();
    comment.zeroize();
    let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
    let store = MacKeychain::named("com.pattobin.personal-assistant", "reply-drafts");
    if let Err(error) = store.put(&revision_key, &normalized) {
        normalized.zeroize();
        return Err(sanitized(error));
    }
    let mut read_back = match store.get(&revision_key) {
        Ok(Some(value)) => value,
        Ok(None) | Err(_) => {
            let _ = store.delete(&revision_key);
            normalized.zeroize();
            return Err("Reply draft could not be verified in macOS Keychain".into());
        }
    };
    let read_back_digest = format!("{:x}", Sha256::digest(read_back.as_bytes()));
    read_back.zeroize();
    if read_back_digest != digest {
        let _ = store.delete(&revision_key);
        normalized.zeroize();
        return Err("Reply draft could not be verified in macOS Keychain".into());
    }
    let result = state.database.record_reply_draft_metadata(
        &draft_key,
        &revision_key,
        &account_id,
        &provider_message_id,
        &digest,
        normalized.len() as i64,
    );
    normalized.zeroize();
    match result {
        Ok(metadata) => Ok(metadata),
        Err(error) => {
            let _ = store.delete(&revision_key);
            Err(sanitized(error))
        }
    }
}

#[tauri::command]
fn verify_reply_draft_storage(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    let metadata = state
        .database
        .reply_draft_metadata(&account_id)
        .map_err(sanitized)?;
    let store = MacKeychain::named("com.pattobin.personal-assistant", "reply-drafts");
    for revision in &metadata {
        let mut content = store
            .get(&revision.revision_key)
            .map_err(sanitized)?
            .ok_or("A private reply draft is missing from macOS Keychain")?;
        let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
        let bytes = content.len() as i64;
        content.zeroize();
        if digest != revision.content_sha256 || bytes != revision.content_bytes {
            return Err("A private reply draft failed its local integrity check".into());
        }
    }
    Ok(metadata.len())
}

#[tauri::command]
fn queue_reply_draft_proposal(
    proposal_key: String,
    audit_event_key: String,
    account_id: String,
    provider_message_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<database::InertActionProposal, String> {
    let revision = state
        .database
        .reply_draft_metadata(&account_id)
        .map_err(sanitized)?
        .into_iter()
        .find(|metadata| metadata.provider_message_id == provider_message_id)
        .ok_or("Private reply draft is unavailable")?;
    let store = MacKeychain::named("com.pattobin.personal-assistant", "reply-drafts");
    let mut content = store
        .get(&revision.revision_key)
        .map_err(sanitized)?
        .ok_or("Private reply draft is missing from macOS Keychain")?;
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let bytes = content.len() as i64;
    content.zeroize();
    if digest != revision.content_sha256 || bytes != revision.content_bytes {
        return Err("Private reply draft failed its local integrity check".into());
    }
    state
        .database
        .queue_reply_draft_proposal(
            &proposal_key,
            &audit_event_key,
            &revision.draft_key,
            &revision.revision_key,
        )
        .map_err(sanitized)
}

#[tauri::command]
fn notification_previews(
    now: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<notifications::NotificationPlan>, String> {
    state
        .database
        .notification_previews(&now)
        .map_err(sanitized)
}

#[tauri::command]
async fn notification_permission_status() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return macos_notifications::permission_status()
        .await
        .map(str::to_owned);
    #[cfg(not(target_os = "macos"))]
    Ok("granted".to_owned())
}

#[tauri::command]
async fn request_notification_permission() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return macos_notifications::request_permission()
        .await
        .map(str::to_owned);
    #[cfg(not(target_os = "macos"))]
    Ok("granted".to_owned())
}

#[tauri::command]
async fn notification_schedule_status() -> Result<usize, String> {
    #[cfg(target_os = "macos")]
    return macos_notifications::automatic_pending_count().await;
    #[cfg(not(target_os = "macos"))]
    Ok(0)
}

#[tauri::command]
async fn send_generic_test_notification<R: Runtime>(
    #[allow(unused_variables)] app: tauri::AppHandle<R>,
    event_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    state
        .database
        .record_notification_test_request(&event_key)
        .map_err(sanitized)?;
    #[cfg(target_os = "macos")]
    return macos_notifications::send_generic_test(&event_key)
        .await
        .map(str::to_owned);
    #[cfg(not(target_os = "macos"))]
    app.notification()
        .builder()
        .title("Personal Assistant")
        .body("Notifications are working. No email content is included.")
        .show()
        .map(|_| "accepted".to_owned())
        .map_err(sanitized)
}

#[tauri::command]
async fn schedule_generic_test_notification(
    event_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    state
        .database
        .record_notification_test_request(&event_key)
        .map_err(sanitized)?;
    #[cfg(target_os = "macos")]
    {
        let delivery_key = format!("scheduled-{event_key}");
        let plan = macos_notifications::schedule_generic_test(&delivery_key).await?;
        state
            .database
            .record_notification_schedule_event(
                &notification_schedule_event_key("scheduled", &delivery_key),
                &delivery_key,
                "scheduled",
                plan.kind,
                &plan.deliver_at.to_rfc3339(),
                "generic_test",
            )
            .map_err(sanitized)?;
        Ok(plan.deliver_at.to_rfc3339())
    }
    #[cfg(not(target_os = "macos"))]
    Err("scheduled notification testing is currently available on macOS only".into())
}

#[tauri::command]
fn decide_action_proposal(
    proposal_key: String,
    event_key: String,
    event_type: String,
    state: tauri::State<'_, AppState>,
) -> Result<database::InertActionProposal, String> {
    state
        .database
        .decide_action_proposal(&proposal_key, &event_key, &event_type)
        .map_err(sanitized)
}

#[tauri::command]
async fn execute_calendar_proposal(
    proposal_key: String,
    execution_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let prepared = state
        .database
        .prepare_calendar_execution(&proposal_key, &execution_key)
        .map_err(sanitized)?;
    let result_key = format!("{}-result", prepared.execution_key);
    let client_id = microsoft_client_id().ok_or("Microsoft connection is unavailable")?;
    let keychain = MacKeychain::new("com.pattobin.personal-assistant");
    let refresh = keychain
        .get(&prepared.account_id)
        .map_err(sanitized)?
        .ok_or("Microsoft credential is missing; reconnect the account")?;
    let tokens = match oauth::refresh_access_token(client_id, &refresh).await {
        Ok(tokens) => tokens,
        Err(error) => {
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "failed",
                    None,
                    "token_refresh_failed",
                )
                .map_err(sanitized)?;
            return Err(sanitized(error));
        }
    };
    if let Some(rotated) = tokens
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        keychain
            .put(&prepared.account_id, rotated)
            .map_err(sanitized)?;
    }
    let request = CalendarCreateRequest {
        subject: prepared.title,
        start_at: prepared.start_at,
        end_at: prepared.end_at,
        transaction_id: prepared.transaction_id,
    };
    match GraphClient::new()
        .map_err(sanitized)?
        .create_calendar_event(&tokens.access_token, request)
        .await
    {
        Ok(event) => {
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "succeeded",
                    Some(&event.id),
                    "provider_accepted",
                )
                .map_err(sanitized)?;
            Ok("Calendar event created in Microsoft.".into())
        }
        Err(error) => {
            let (outcome, reason) = if matches!(&error, GraphError::Transport(_)) {
                ("unknown", "transport_ambiguous")
            } else {
                ("failed", "provider_rejected")
            };
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    outcome,
                    None,
                    reason,
                )
                .map_err(sanitized)?;
            Err(sanitized(error))
        }
    }
}

#[tauri::command]
async fn execute_calendar_update_proposal(
    proposal_key: String,
    execution_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let prepared = state
        .database
        .prepare_calendar_update_execution(&proposal_key, &execution_key)
        .map_err(sanitized)?;
    let result_key = format!("{}-result", prepared.execution_key);
    let client_id = microsoft_client_id().ok_or("Microsoft connection is unavailable")?;
    let keychain = MacKeychain::new("com.pattobin.personal-assistant");
    let refresh = keychain
        .get(&prepared.account_id)
        .map_err(sanitized)?
        .ok_or("Microsoft credential is missing; reconnect the account")?;
    let tokens = match oauth::refresh_access_token(client_id, &refresh).await {
        Ok(tokens) => tokens,
        Err(error) => {
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "failed",
                    None,
                    "token_refresh_failed",
                )
                .map_err(sanitized)?;
            return Err(sanitized(error));
        }
    };
    if let Some(rotated) = tokens
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        keychain
            .put(&prepared.account_id, rotated)
            .map_err(sanitized)?;
    }
    let request = CalendarUpdateRequest {
        provider_event_id: prepared.provider_event_id.clone(),
        provider_etag: prepared.provider_etag,
        start_at: prepared.proposed_start_at,
        end_at: prepared.proposed_end_at,
    };
    match GraphClient::new()
        .map_err(sanitized)?
        .update_calendar_event(&tokens.access_token, request)
        .await
    {
        Ok(event) => {
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "succeeded",
                    Some(&event.id),
                    "provider_accepted",
                )
                .map_err(sanitized)?;
            Ok("Microsoft Calendar event updated once. Sync to refresh the local copy.".into())
        }
        Err(error) => {
            let (outcome, reason) = match &error {
                GraphError::Transport(_) => ("unknown", "transport_ambiguous"),
                GraphError::HttpStatus(412) => ("failed", "provider_etag_stale"),
                _ => ("failed", "provider_rejected"),
            };
            state
                .database
                .record_calendar_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    outcome,
                    None,
                    reason,
                )
                .map_err(sanitized)?;
            Err(sanitized(error))
        }
    }
}

#[tauri::command]
async fn execute_correspondence_proposal(
    proposal_key: String,
    execution_key: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let prepared = state
        .database
        .prepare_correspondence_execution(&proposal_key, &execution_key)
        .map_err(sanitized)?;
    let result_key = format!("{}-result", prepared.execution_key);
    let mut comment =
        match reply_draft_from_main_thread(&app, prepared.revision_key.clone()).await? {
            Some(value) => value,
            None => {
                state
                    .database
                    .record_correspondence_execution_result(
                        &prepared.execution_key,
                        &result_key,
                        "failed",
                        "draft_missing",
                    )
                    .map_err(sanitized)?;
                return Err("Private reply draft is missing from macOS Keychain".into());
            }
        };
    let digest = format!("{:x}", Sha256::digest(comment.as_bytes()));
    if digest != prepared.content_sha256 || comment.len() as i64 != prepared.content_bytes {
        comment.zeroize();
        state
            .database
            .record_correspondence_execution_result(
                &prepared.execution_key,
                &result_key,
                "failed",
                "draft_integrity_failed",
            )
            .map_err(sanitized)?;
        return Err("Private reply draft failed its local integrity check".into());
    }

    let client_id = microsoft_client_id().ok_or("Microsoft connection is unavailable")?;
    let credential_store = MacKeychain::new("com.pattobin.personal-assistant");
    let refresh = credential_store
        .get(&prepared.account_id)
        .map_err(sanitized)?
        .ok_or("Microsoft credential is missing; reconnect the account")?;
    let tokens = match oauth::refresh_access_token(client_id, &refresh).await {
        Ok(tokens) => tokens,
        Err(error) => {
            comment.zeroize();
            state
                .database
                .record_correspondence_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "failed",
                    "token_refresh_failed",
                )
                .map_err(sanitized)?;
            return Err(sanitized(error));
        }
    };
    if let Some(rotated) = tokens
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        credential_store
            .put(&prepared.account_id, rotated)
            .map_err(sanitized)?;
    }
    let request = ReplyToMessageRequest {
        provider_message_id: prepared.provider_message_id,
        comment,
    };
    match GraphClient::new()
        .map_err(sanitized)?
        .reply_to_message(&tokens.access_token, request)
        .await
    {
        Ok(()) => {
            state
                .database
                .record_correspondence_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    "succeeded",
                    "provider_accepted",
                )
                .map_err(sanitized)?;
            Ok("Microsoft reply sent once. Sync to refresh local metadata.".into())
        }
        Err(error) => {
            let (outcome, reason) = if matches!(&error, GraphError::Transport(_)) {
                ("unknown", "transport_ambiguous")
            } else {
                ("failed", "provider_rejected")
            };
            state
                .database
                .record_correspondence_execution_result(
                    &prepared.execution_key,
                    &result_key,
                    outcome,
                    reason,
                )
                .map_err(sanitized)?;
            Err(sanitized(error))
        }
    }
}

async fn reply_draft_from_main_thread(
    app: &tauri::AppHandle,
    revision_key: String,
) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = MacKeychain::named("com.pattobin.personal-assistant", "reply-drafts")
            .get(&revision_key)
            .map_err(sanitized);
        let _ = sender.send(result);
    })
    .map_err(sanitized)?;
    receiver
        .await
        .map_err(|_| "Private reply draft access was interrupted".to_owned())?
}

#[tauri::command]
fn focused_local_items(
    view: String,
    day_start: String,
    day_end: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<database::LocalItem>, String> {
    state
        .database
        .focused_local_items(&view, &day_start, &day_end)
        .map_err(sanitized)
}

#[tauri::command]
fn home_dashboard(
    day_start: String,
    day_end: String,
    state: tauri::State<'_, AppState>,
) -> Result<HomeDashboard, String> {
    let local = state.database.local_items(false).map_err(sanitized)?;
    let is_active = |item: &&database::LocalItem| {
        item.status == "active" && item.lifecycle_state != "completed"
    };
    let todo_count = local
        .iter()
        .filter(is_active)
        .filter(|item| item.kind == "task")
        .count();
    let waiting_count = local
        .iter()
        .filter(is_active)
        .filter(|item| item.kind == "waiting_for")
        .count();
    let calendar_count = local
        .iter()
        .filter(is_active)
        .filter(|item| item.kind == "appointment")
        .count();
    let today = state
        .database
        .focused_local_items("today", &day_start, &day_end)
        .map_err(sanitized)?
        .into_iter()
        .take(5)
        .collect();
    let accounts = state.database.accounts().map_err(sanitized)?;
    let mut healthy_accounts = 0;
    let mut attention_accounts = 0;
    let mut last_sync_at: Option<String> = None;
    for account in &accounts {
        match state
            .database
            .last_sync_run(&account.id)
            .map_err(sanitized)?
        {
            Some(sync) if sync.outcome == "success" => {
                healthy_accounts += 1;
                if let Some(finished) = sync.finished_at {
                    if last_sync_at
                        .as_ref()
                        .is_none_or(|latest| finished > *latest)
                    {
                        last_sync_at = Some(finished);
                    }
                }
            }
            _ => attention_accounts += 1,
        }
    }
    Ok(HomeDashboard {
        today,
        todo_count,
        waiting_count,
        calendar_count,
        review_count: state.database.pending_review_count().map_err(sanitized)?,
        sync: HomeSyncHealth {
            connected_accounts: accounts.len(),
            healthy_accounts,
            attention_accounts,
            last_sync_at,
        },
    })
}

#[tauri::command]
fn local_item_provenance(
    id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<database::LocalItemProvenance, String> {
    state.database.local_item_provenance(id).map_err(sanitized)
}

#[tauri::command]
fn open_local_item_source<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let raw = state
        .database
        .local_item_source_link(id)
        .map_err(sanitized)?;
    let url = validated_outlook_source_link(&raw)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| "The source email could not be opened safely".to_string())
}

fn validated_outlook_source_link(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "The source email link is invalid".to_string())?;
    let allowed_host = matches!(
        url.host_str(),
        Some("outlook.office.com" | "outlook.office365.com" | "outlook.live.com")
    );
    if url.scheme() != "https"
        || !allowed_host
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.port(), None | Some(443))
    {
        return Err("The source email link is not an approved Outlook URL".into());
    }
    Ok(url)
}

#[tauri::command]
async fn analyze_microsoft_message<R: Runtime>(
    app: tauri::AppHandle<R>,
    account_id: String,
    provider_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<MessageAnalysisResult, String> {
    if state.message_analysis_active.swap(true, Ordering::SeqCst) {
        return Err("A private message analysis is already running".into());
    }
    let result = analyze_microsoft_message_inner(&app, &account_id, &provider_id, &state).await;
    state.message_analysis_active.store(false, Ordering::SeqCst);
    result
}

async fn analyze_microsoft_message_inner<R: Runtime>(
    app: &tauri::AppHandle<R>,
    account_id: &str,
    provider_id: &str,
    state: &AppState,
) -> Result<MessageAnalysisResult, String> {
    let settings = state.database.settings().map_err(sanitized)?;
    if !settings.local_email_analysis_enabled {
        return Err("Enable private local email analysis in Settings first".into());
    }
    if !state
        .database
        .message_available(account_id, provider_id)
        .map_err(sanitized)?
    {
        return Err("The selected synchronized message is unavailable".into());
    }
    if state
        .database
        .message_analyzed(account_id, provider_id)
        .map_err(sanitized)?
    {
        return Err("This message already has a saved analysis".into());
    }
    let capabilities = ai::detect(&state.model_directory).map_err(sanitized)?;
    let artifact = capabilities.recommendation.artifact;
    if capabilities.lifecycle != ai::ModelLifecycle::Installed
        || !state
            .database
            .has_ai_qualification(
                &artifact.sha256,
                ai::QUALIFIED_RUNTIME_BUILD,
                ai::PERSISTENT_CORPUS_VERSION,
            )
            .map_err(sanitized)?
    {
        return Err("The installed private AI build is not qualified".into());
    }
    let model = ai::verify_model(&state.model_directory, &artifact).map_err(sanitized)?;
    let client_id = microsoft_client_id().ok_or("Microsoft connection is unavailable")?;
    let keychain = MacKeychain::new("com.pattobin.personal-assistant");
    let refresh = keychain
        .get(account_id)
        .map_err(sanitized)?
        .ok_or("Microsoft credential is missing; reconnect the account")?;
    let tokens = oauth::refresh_access_token(client_id, &refresh)
        .await
        .map_err(sanitized)?;
    if let Some(rotated) = tokens
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        keychain.put(account_id, rotated).map_err(sanitized)?;
    }
    let content = GraphClient::new()
        .map_err(sanitized)?
        .get_message_content(&tokens.access_token, provider_id)
        .await
        .map_err(sanitized)?;
    let mut source = String::with_capacity(
        content.subject.as_ref().map_or(0, String::len) + content.body.len() + 12,
    );
    if let Some(subject) = &content.subject {
        source.push_str("Subject: ");
        source.push_str(subject);
        source.push('\n');
    }
    source.push_str(&content.body);
    let reference_at = content
        .received_date_time
        .clone()
        .or_else(|| content.sent_date_time.clone());
    drop(content);

    let resource_dir = app.path().resource_dir().map_err(sanitized)?;
    let runtime = resource_dir.join("resources/llama-runtime/macos-arm64");
    let worker = resource_dir
        .join("resources/inference-worker/macos-arm64")
        .join("personal-assistant-inference-worker");
    let database = state.database.clone();
    let account_id = account_id.to_owned();
    let provider_id = provider_id.to_owned();
    let model_sha256 = artifact.sha256;
    tauri::async_runtime::spawn_blocking(move || {
        let extraction = run_single_message_inference(&worker, &runtime, &model, source)?;
        let plan = reference_at.as_deref().map_or_else(
            || rules::evaluate(&extraction),
            |reference| rules::evaluate_with_reference(&extraction, reference),
        );
        database
            .save_message_analysis(&account_id, &provider_id, &extraction, &plan, &model_sha256)
            .map_err(sanitized)?;
        let local_projection_count = database
            .apply_policy_local_projection(&account_id, &provider_id, &extraction, &plan)
            .map_err(sanitized)?;
        Ok(MessageAnalysisResult {
            provider_id,
            summary: extraction.summary,
            classification: extraction.classification,
            urgency: extraction.urgency,
            disposition: plan.disposition,
            suggestion_count: plan.proposals.len(),
            local_projection_count,
        })
    })
    .await
    .map_err(sanitized)?
}

#[tauri::command]
async fn connect_microsoft<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<MicrosoftAccountView, String> {
    let client_id = microsoft_client_id()
        .ok_or("This build does not include a Microsoft application client ID")?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(sanitized)?;
    let port = listener.local_addr().map_err(sanitized)?.port();
    let redirect =
        Url::parse(&format!("http://localhost:{port}/oauth/callback")).map_err(sanitized)?;
    let pending = oauth::begin_authorization(client_id, &redirect).map_err(sanitized)?;
    app.opener()
        .open_url(pending.authorization_url.as_str(), None::<&str>)
        .map_err(sanitized)?;
    let accepted = tokio::time::timeout(std::time::Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| "Microsoft sign-in timed out".to_string())?
        .map_err(sanitized)?;
    let (mut stream, _) = accepted;
    let mut buffer = vec![0_u8; 8192];
    let read = stream.read(&mut buffer).await.map_err(sanitized)?;
    let request = std::str::from_utf8(&buffer[..read])
        .map_err(|_| "Invalid local OAuth callback".to_string())?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("Invalid local OAuth callback")?;
    let callback = Url::parse(&format!("http://localhost:{port}{target}")).map_err(sanitized)?;
    let result = oauth::validate_callback(&callback, &redirect, &pending.state).map_err(sanitized);
    let (status, body) = if result.is_ok() {
        (
            "200 OK",
            "Microsoft account connected. You can return to Personal Assistant.",
        )
    } else {
        (
            "400 Bad Request",
            "Microsoft sign-in could not be completed. Return to Personal Assistant.",
        )
    };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\n\r\n{body}", body.len());
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(sanitized)?;
    let code = result?;
    let tokens = oauth::exchange_code(client_id, &redirect, &code, &pending.code_verifier)
        .await
        .map_err(sanitized)?;
    let graph = GraphClient::new().map_err(sanitized)?;
    let profile_url = Url::parse(&format!(
        "{GRAPH_BASE_URL}me?$select=id,displayName,mail,userPrincipalName"
    ))
    .map_err(sanitized)?;
    let profile: UserProfile = graph
        .get_json(&tokens.access_token, &profile_url)
        .await
        .map_err(sanitized)?;
    let refresh = tokens
        .refresh_token
        .ok_or("Microsoft did not return an offline refresh token")?;
    MacKeychain::new("com.pattobin.personal-assistant")
        .put(&profile.id, &refresh)
        .map_err(sanitized)?;
    let account = ConnectedAccount {
        id: profile.id.clone(),
        provider: "microsoft".into(),
        display_name: profile.display_name.clone(),
        email_address: profile.mail.unwrap_or(profile.user_principal_name),
        tenant_id: None,
    };
    state.database.upsert_account(&account).map_err(sanitized)?;
    Ok(MicrosoftAccountView {
        id: account.id,
        display_name: account.display_name,
        email_address: account.email_address,
        last_sync: None,
        recent_messages: Vec::new(),
    })
}

#[tauri::command]
async fn sync_microsoft(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<SyncResult, String> {
    run_account_sync(&state, account_id).await
}

async fn run_account_sync(state: &AppState, account_id: String) -> Result<SyncResult, String> {
    if account_id.is_empty() {
        return Err("Account ID is required".into());
    }
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut active = state
            .sync_cancellations
            .lock()
            .map_err(|_| "Synchronization state is unavailable")?;
        if active.contains_key(&account_id) {
            return Err("This account is already synchronizing".into());
        }
        active.insert(account_id.clone(), cancellation.clone());
    }
    let run_id = state
        .database
        .begin_sync_run(&account_id)
        .map_err(sanitized)?;
    let result = sync_microsoft_inner(&account_id, &state.database, &cancellation).await;
    if let Ok(mut active) = state.sync_cancellations.lock() {
        active.remove(&account_id);
    }
    let (outcome, counts, error_code) = match &result {
        Ok(value) => ("success", (value.inbox, value.sent, value.calendar), None),
        Err(error) if error == "Synchronization cancelled" => {
            ("cancelled", (0, 0, 0), Some("cancelled"))
        }
        Err(_) => ("error", (0, 0, 0), Some("sync_failed")),
    };
    state
        .database
        .finish_sync_run(run_id, outcome, counts, error_code)
        .map_err(sanitized)?;
    result
}

fn sync_run_view(run: SyncRunSummary) -> SyncRunView {
    SyncRunView {
        outcome: run.outcome,
        finished_at: run.finished_at,
        item_count: run.inbox_count + run.sent_count + run.calendar_count,
    }
}

async fn sync_microsoft_inner(
    account_id: &str,
    database: &Database,
    cancellation: &AtomicBool,
) -> Result<SyncResult, String> {
    let client_id = microsoft_client_id()
        .ok_or("This build does not include a Microsoft application client ID")?;
    let keychain = MacKeychain::new("com.pattobin.personal-assistant");
    let refresh = keychain
        .get(account_id)
        .map_err(sanitized)?
        .ok_or("Microsoft credential is missing; reconnect the account")?;
    let tokens = oauth::refresh_access_token(client_id, &refresh)
        .await
        .map_err(sanitized)?;
    if let Some(rotated) = tokens
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        keychain.put(account_id, rotated).map_err(sanitized)?;
    }
    let graph = GraphClient::new().map_err(sanitized)?;
    let inbox = sync_mail_folder(
        &graph,
        database,
        &tokens.access_token,
        account_id,
        "mail_inbox",
        MailFolder::Inbox,
        cancellation,
    )
    .await?;
    let sent = sync_mail_folder(
        &graph,
        database,
        &tokens.access_token,
        account_id,
        "mail_sent",
        MailFolder::SentItems,
        cancellation,
    )
    .await?;
    let start = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let end = (chrono::Utc::now() + chrono::Duration::days(365)).to_rfc3339();
    let mut next = Some(initial_calendar_url(&start, &end).map_err(sanitized)?);
    let mut calendar = 0;
    while let Some(url) = next.take() {
        ensure_not_cancelled(cancellation)?;
        let page = graph
            .get_page::<CalendarEvent>(&tokens.access_token, &url)
            .await
            .map_err(sanitized)?;
        calendar += page.value.len();
        database
            .apply_calendar_page(account_id, &page.value)
            .map_err(sanitized)?;
        next = page.next_link;
    }
    Ok(SyncResult {
        inbox,
        sent,
        calendar,
    })
}

async fn sync_mail_folder(
    graph: &GraphClient,
    database: &Database,
    access_token: &str,
    account_id: &str,
    resource: &str,
    folder: MailFolder,
    cancellation: &AtomicBool,
) -> Result<usize, String> {
    let mut next = match database
        .sync_cursor(account_id, resource)
        .map_err(sanitized)?
    {
        Some(cursor) => Url::parse(&cursor).map_err(sanitized)?,
        None => initial_mail_delta_url(folder),
    };
    let mut count = 0;
    loop {
        ensure_not_cancelled(cancellation)?;
        let page = graph
            .get_page::<MessageMetadata>(access_token, &next)
            .await
            .map_err(sanitized)?;
        count += page.value.len();
        let final_cursor = page.delta_link.as_ref().map(Url::as_str);
        database
            .apply_message_page(account_id, resource, &page.value, final_cursor)
            .map_err(sanitized)?;
        if let Some(url) = page.next_link {
            next = url;
        } else {
            break;
        }
    }
    Ok(count)
}

fn ensure_not_cancelled(cancellation: &AtomicBool) -> Result<(), String> {
    if cancellation.load(Ordering::Relaxed) {
        Err("Synchronization cancelled".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn cancel_microsoft_sync(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let active = state
        .sync_cancellations
        .lock()
        .map_err(|_| "Synchronization state is unavailable")?;
    Ok(if let Some(flag) = active.get(&account_id) {
        flag.store(true, Ordering::Relaxed);
        true
    } else {
        false
    })
}

#[tauri::command]
fn disconnect_microsoft(
    account_id: String,
    delete_local_data: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    MacKeychain::new("com.pattobin.personal-assistant")
        .delete(&account_id)
        .map_err(sanitized)?;
    if delete_local_data {
        delete_reply_draft_secrets(&account_id, &state)?;
        state
            .database
            .delete_account_data(&account_id)
            .map_err(sanitized)?;
    } else {
        state
            .database
            .disable_account(&account_id)
            .map_err(sanitized)?;
    }
    Ok(())
}

#[tauri::command]
fn delete_microsoft_local_data(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if state
        .sync_cancellations
        .lock()
        .map_err(|_| "Synchronization state is unavailable")?
        .contains_key(&account_id)
    {
        return Err("Cancel synchronization before deleting downloaded data".into());
    }
    delete_reply_draft_secrets(&account_id, &state)?;
    state
        .database
        .clear_synced_data(&account_id)
        .map_err(sanitized)
}

fn delete_reply_draft_secrets(
    account_id: &str,
    state: &tauri::State<'_, AppState>,
) -> Result<(), String> {
    let keys = state
        .database
        .reply_draft_revision_keys(account_id)
        .map_err(sanitized)?;
    let store = MacKeychain::named("com.pattobin.personal-assistant", "reply-drafts");
    for key in keys {
        store.delete(&key).map_err(sanitized)?;
    }
    Ok(())
}

async fn background_sync_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;
    loop {
        interval.tick().await;
        if microsoft_client_id().is_none() {
            continue;
        }
        if state.database.apply_retention().is_err() {
            continue;
        }
        let accounts = match state.database.accounts() {
            Ok(values) => values,
            Err(_) => continue,
        };
        for account in accounts {
            let _ = run_account_sync(&state, account.id).await;
        }
    }
}

#[cfg(target_os = "macos")]
fn notification_schedule_event_key(event_type: &str, delivery_key: &str) -> String {
    let digest = Sha256::digest(format!("{event_type}:{delivery_key}").as_bytes());
    format!("notification-{event_type}-{digest:x}")
}

#[cfg(target_os = "macos")]
async fn reconcile_notification_schedule(state: &AppState) -> Result<(), String> {
    let now = chrono::Local::now().to_rfc3339();
    let settings = state.database.settings().map_err(sanitized)?;
    let authorized = macos_notifications::permission_status().await? == "granted";
    let desired = if settings.notification_delivery_enabled && authorized {
        state
            .database
            .notification_previews(&now)
            .map_err(sanitized)?
    } else {
        Vec::new()
    };
    let satisfied = state
        .database
        .satisfied_notification_delivery_keys(&now)
        .map_err(sanitized)?;
    let outcome = macos_notifications::reconcile_schedule(desired, &satisfied).await?;
    for event in outcome.scheduled {
        state
            .database
            .record_notification_schedule_event(
                &notification_schedule_event_key(event.event_type, &event.delivery_key),
                &event.delivery_key,
                event.event_type,
                event.kind,
                &event.deliver_at,
                event.reason_code,
            )
            .map_err(sanitized)?;
    }
    for delivery_key in outcome.cancelled {
        state
            .database
            .record_notification_schedule_cancellation(
                &notification_schedule_event_key("cancelled", &delivery_key),
                &delivery_key,
            )
            .map_err(sanitized)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn background_notification_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let _ = reconcile_notification_schedule(&state).await;
    }
}

const DEFAULT_MICROSOFT_CLIENT_ID: &str = "5ed5a791-1959-4866-8eb9-c9439cf04043";

fn microsoft_client_id() -> Option<&'static str> {
    option_env!("PA_MICROSOFT_CLIENT_ID")
        .filter(|value| !value.is_empty())
        .or(Some(DEFAULT_MICROSOFT_CLIENT_ID))
}
fn sanitized(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(
                    embedded_updater_public_key().expect("embedded updater public key is missing"),
                )
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            macos_notifications::install_foreground_delegate();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let startup_restore_status = match restore::apply_pending(&data_dir)
                .map_err(Box::<dyn std::error::Error>::from)?
            {
                restore::ApplyOutcome::None => None,
                restore::ApplyOutcome::Applied { rollback_path } => Some(format!(
                    "Encrypted backup restored successfully. A rollback database is retained at {}.",
                    rollback_path.display()
                )),
                restore::ApplyOutcome::Rejected => Some(
                    "The prepared restore failed validation. Existing local data was preserved."
                        .into(),
                ),
            };
            let model_directory = data_dir.join("models");
            std::fs::create_dir_all(&model_directory)?;
            let database = Database::open(data_dir.join("personal-assistant.db"))
                .map_err(Box::<dyn std::error::Error>::from)?;
            let state = AppState {
                database: Arc::new(database),
                sync_cancellations: Arc::new(Mutex::new(HashMap::new())),
                model_directory,
                model_download_active: Arc::new(AtomicBool::new(false)),
                worker_evaluation_active: Arc::new(AtomicBool::new(false)),
                message_analysis_active: Arc::new(AtomicBool::new(false)),
                family_display_listener: Arc::new(tokio::sync::Mutex::new(None)),
                family_display_pairing: Arc::new(display_server::PairingCoordinator::default()),
                data_directory: data_dir,
                startup_restore_status,
                updater_active: Arc::new(AtomicBool::new(false)),
            };
            #[cfg(target_os = "macos")]
            let family_display_startup =
                saved_family_display_listener_config(&state).ok().flatten();
            let _ = state.database.apply_retention();
            app.manage(state.clone());
            #[cfg(target_os = "macos")]
            if let Some(config) = family_display_startup {
                let display_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(listener) = display_server::start_listener_with_pairing(
                        display_state.database.clone(),
                        config,
                        display_state.family_display_pairing.clone(),
                    )
                    .await
                    {
                        *display_state.family_display_listener.lock().await = Some(listener);
                    }
                });
            }
            tauri::async_runtime::spawn(background_sync_loop(state.clone()));
            #[cfg(target_os = "macos")]
            tauri::async_runtime::spawn(background_notification_loop(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            create_encrypted_backup,
            verify_encrypted_backup,
            prepare_encrypted_restore,
            restart_for_restore,
            restore_status,
            updater_status,
            check_for_update,
            install_update,
            ai_capabilities,
            download_ai_model,
            remove_ai_model,
            test_ai_runtime,
            test_ai_evaluation,
            test_persistent_ai_evaluation,
            microsoft_status,
            review_items,
            review_history,
            decide_review,
            local_items,
            undo_local_item,
            transition_local_item,
            local_item_events,
            action_proposals,
            correspondence_execution_history,
            automation_audit_history,
            family_displays,
            revoke_family_display,
            family_display_service_status,
            begin_family_display_pairing,
            enable_family_display_service,
            disable_family_display_service,
            calendar_update_candidates,
            queue_calendar_update_proposal,
            save_reply_draft,
            verify_reply_draft_storage,
            queue_reply_draft_proposal,
            notification_previews,
            notification_permission_status,
            notification_schedule_status,
            request_notification_permission,
            send_generic_test_notification,
            schedule_generic_test_notification,
            decide_action_proposal,
            execute_calendar_proposal,
            execute_calendar_update_proposal,
            execute_correspondence_proposal,
            focused_local_items,
            home_dashboard,
            local_item_provenance,
            open_local_item_source,
            analyze_microsoft_message,
            connect_microsoft,
            sync_microsoft,
            cancel_microsoft_sync,
            disconnect_microsoft,
            delete_microsoft_local_data
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Personal Assistant");
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::{
        decode_sha256_hex, load_family_display_certificate, notification_schedule_event_key,
        write_private_atomic,
    };
    use super::{
        read_bounded_backup_file, valid_expected_update_version, validated_outlook_source_link,
        write_new_private_file, UPDATE_ENDPOINT,
    };

    #[test]
    fn updater_endpoint_and_expected_versions_are_narrowly_validated() {
        let endpoint = url::Url::parse(UPDATE_ENDPOINT).unwrap();
        assert_eq!(endpoint.scheme(), "https");
        assert_eq!(endpoint.host_str(), Some("github.com"));
        assert_eq!(
            endpoint.path(),
            "/xvsystemslimerick/personalassistant/releases/latest/download/latest.json"
        );
        for valid in ["1.2.0", "1.2.0-beta.1", "v2.0.0+arm64"] {
            assert!(valid_expected_update_version(valid));
        }
        for invalid in ["", "1.2.0/../../x", "1.2.0 latest", &"a".repeat(33)] {
            assert!(!valid_expected_update_version(invalid));
        }
    }

    #[test]
    fn updater_plugin_configuration_contains_the_embedded_trust_anchor() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid Tauri config");
        let configured_key = config
            .pointer("/plugins/updater/pubkey")
            .and_then(serde_json::Value::as_str);

        assert_eq!(configured_key, super::embedded_updater_public_key());
    }

    #[test]
    fn direct_distribution_flag_disables_update_discovery() {
        assert!(!super::updater_enabled(Some("1")));
        assert!(super::updater_enabled(None));
        assert!(super::updater_enabled(Some("0")));
    }

    #[test]
    fn source_email_links_require_exact_https_outlook_hosts() {
        assert!(
            validated_outlook_source_link("https://outlook.office.com/mail/deeplink/read/abc")
                .is_ok()
        );
        assert!(
            validated_outlook_source_link("https://outlook.live.com/mail/0/inbox/id/abc").is_ok()
        );
        for rejected in [
            "http://outlook.office.com/mail/abc",
            "https://outlook.office.com.evil.example/mail/abc",
            "https://evil.example/?next=https://outlook.office.com",
            "https://user@outlook.office.com/mail/abc",
            "https://outlook.office.com:8443/mail/abc",
        ] {
            assert!(
                validated_outlook_source_link(rejected).is_err(),
                "{rejected}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn notification_schedule_event_keys_are_stable_bounded_and_event_specific() {
        let delivery = "notification-delivery-key-00000000000000000000000000000000000000000001";
        let scheduled = notification_schedule_event_key("scheduled", delivery);
        assert_eq!(
            scheduled,
            notification_schedule_event_key("scheduled", delivery)
        );
        assert_ne!(
            scheduled,
            notification_schedule_event_key("cancelled", delivery)
        );
        assert!((16..=100).contains(&scheduled.len()));
        assert!(scheduled
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn family_display_identity_files_are_private_atomic_and_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.der");
        write_private_atomic(&path, b"certificate-one").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"certificate-one");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write_private_atomic(&path, b"certificate-two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"certificate-two");
        assert!(write_private_atomic(&path, &vec![0_u8; 16 * 1024 + 1]).is_err());
        assert!(directory.path().read_dir().unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn family_display_public_certificate_requires_private_integrity_checked_storage() {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let certificate = b"public-certificate";
        let digest = super::encode_sha256_hex(&<[u8; 32]>::from(Sha256::digest(certificate)));
        let path = super::family_display_certificate_path(directory.path(), &digest);
        write_private_atomic(&path, certificate).unwrap();
        assert_eq!(
            load_family_display_certificate(directory.path(), &digest).unwrap(),
            certificate
        );
        std::fs::write(&path, b"tampered").unwrap();
        assert!(load_family_display_certificate(directory.path(), &digest).is_err());
    }

    #[test]
    fn encrypted_backup_writer_is_private_and_never_overwrites() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup.pabackup");
        let value = vec![7_u8; 128];
        write_new_private_file(&path, &value).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), value);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_new_private_file(&path, &[8_u8; 128]).is_err());
    }

    #[test]
    fn backup_reader_rejects_symlinks_and_oversized_files_before_reading() {
        let directory = tempfile::tempdir().unwrap();
        let regular = directory.path().join("regular.pabackup");
        std::fs::write(&regular, b"bounded").unwrap();
        assert_eq!(read_bounded_backup_file(&regular).unwrap(), b"bounded");
        let oversized = directory.path().join("oversized.pabackup");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(513 * 1024 * 1024 + 1)
            .unwrap();
        assert!(read_bounded_backup_file(&oversized).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&regular, directory.path().join("link.pabackup")).unwrap();
            assert!(
                read_bounded_backup_file(directory.path().join("link.pabackup").as_path()).is_err()
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn family_display_fingerprint_decoder_is_exact() {
        assert_eq!(decode_sha256_hex(&"12".repeat(32)).unwrap(), [0x12; 32]);
        for invalid in ["12", &"zz".repeat(32), &"12".repeat(33)] {
            assert!(decode_sha256_hex(invalid).is_err());
        }
    }
}
