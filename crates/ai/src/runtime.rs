use crate::{
    build_extraction_prompt, contains_prompt_injection_signal, evaluate_case, evaluation_corpus,
    extraction_grammar, parse_and_validate_model_output, verify_model, Extraction, ModelArtifact,
};
use serde::Serialize;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use thiserror::Error;

const EXPECTED_BUILD: &str = crate::QUALIFIED_RUNTIME_BUILD;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeAcceleration {
    Metal,
    Cpu,
}

pub fn run_fixture_extraction(
    runtime_directory: &Path,
    model_directory: &Path,
    artifact: &ModelArtifact,
    source: &str,
) -> Result<Extraction, RuntimeError> {
    let model =
        verify_model(model_directory, artifact).map_err(|_| RuntimeError::ModelIntegrity)?;
    let executable = checked_runtime(runtime_directory)?;
    let acceleration = detect_acceleration(&executable)?;
    run_verified_extraction(&executable, &model, acceleration, source)
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationCaseResult {
    pub id: String,
    pub passed: bool,
    pub observed_classification: Option<crate::Classification>,
    pub task_count: Option<usize>,
    pub appointment_count: Option<usize>,
    pub waiting_count: Option<usize>,
    pub safety_rejected: bool,
    pub failure_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationReport {
    pub passed: usize,
    pub total: usize,
    pub all_passed: bool,
    pub cases: Vec<EvaluationCaseResult>,
}

pub fn run_evaluation_corpus(
    runtime_directory: &Path,
    model_directory: &Path,
    artifact: &ModelArtifact,
) -> Result<EvaluationReport, RuntimeError> {
    let model =
        verify_model(model_directory, artifact).map_err(|_| RuntimeError::ModelIntegrity)?;
    let executable = checked_runtime(runtime_directory)?;
    let acceleration = detect_acceleration(&executable)?;
    let cases = evaluation_corpus()
        .iter()
        .map(|case| {
            let result = run_verified_extraction(&executable, &model, acceleration, case.source);
            let (passed, classification, tasks, appointments, waiting, safety_rejected, failure) =
                match result {
                    Ok(extraction) => (
                        !case.expected_safety_rejection && evaluate_case(case, &extraction),
                        Some(extraction.classification),
                        Some(extraction.tasks.len()),
                        Some(extraction.appointments.len()),
                        Some(extraction.waiting_for.len()),
                        false,
                        None,
                    ),
                    Err(RuntimeError::PromptInjection) => (
                        case.expected_safety_rejection,
                        None,
                        None,
                        None,
                        None,
                        true,
                        None,
                    ),
                    Err(error) => (
                        false,
                        None,
                        None,
                        None,
                        None,
                        false,
                        Some(evaluation_failure_code(&error).to_owned()),
                    ),
                };
            EvaluationCaseResult {
                id: case.id.to_owned(),
                passed,
                observed_classification: classification,
                task_count: tasks,
                appointment_count: appointments,
                waiting_count: waiting,
                safety_rejected,
                failure_code: failure,
            }
        })
        .collect::<Vec<_>>();
    let passed = cases.iter().filter(|case| case.passed).count();
    let total = cases.len();
    Ok(EvaluationReport {
        passed,
        total,
        all_passed: passed == total,
        cases,
    })
}

fn evaluation_failure_code(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::InvalidExtraction(crate::ExtractionError::InvalidJson) => "invalid_json",
        RuntimeError::InvalidExtraction(crate::ExtractionError::InvalidEvidence) => {
            "invalid_evidence"
        }
        RuntimeError::InvalidExtraction(crate::ExtractionError::AmbiguousEvidence) => {
            "ambiguous_evidence"
        }
        RuntimeError::InvalidExtraction(crate::ExtractionError::InvalidOutputSize) => {
            "invalid_output_size"
        }
        RuntimeError::InvalidExtraction(_) => "invalid_contract",
        RuntimeError::InvalidFraming => "invalid_framing",
        RuntimeError::Timeout => "timeout",
        RuntimeError::InferenceFailed => "inference_failed",
        RuntimeError::ModelIntegrity => "model_integrity",
        RuntimeError::MissingRuntime | RuntimeError::VersionMismatch => "runtime_integrity",
        RuntimeError::DeviceProbe => "device_probe",
        RuntimeError::HealthCheckFailed | RuntimeError::Process(_) => "runtime_process",
        RuntimeError::PromptInjection => "safety_rejected",
    }
}

fn run_verified_extraction(
    executable: &Path,
    model: &Path,
    acceleration: RuntimeAcceleration,
    source: &str,
) -> Result<Extraction, RuntimeError> {
    if contains_prompt_injection_signal(source) {
        return Err(RuntimeError::PromptInjection);
    }
    let prompt = build_extraction_prompt(source).map_err(RuntimeError::InvalidExtraction)?;
    let mut command = Command::new(executable);
    command.args(["--model"]).arg(model).args([
        "--prompt",
        &prompt,
        "--grammar",
        extraction_grammar(),
        "--n-predict",
        "512",
        "--ctx-size",
        "4096",
        "--temp",
        "0",
        "--seed",
        "1",
        "--reasoning",
        "off",
        "--no-display-prompt",
        "--no-conversation",
        "--single-turn",
        "--simple-io",
        "--no-show-timings",
        "--log-disable",
    ]);
    configure_acceleration(&mut command, acceleration);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    wait_for_child(&mut child, Duration::from_secs(180))?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or(RuntimeError::InferenceFailed)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut output)?;
    let framed = framed_json(&output).ok_or(RuntimeError::InvalidFraming)?;
    parse_and_validate_model_output(source, framed).map_err(RuntimeError::InvalidExtraction)
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealth {
    pub ready: bool,
    pub runtime_version: String,
    pub acceleration: RuntimeAcceleration,
    pub model_id: String,
}

pub fn health_check(
    runtime_directory: &Path,
    model_directory: &Path,
    artifact: &ModelArtifact,
) -> Result<RuntimeHealth, RuntimeError> {
    let model =
        verify_model(model_directory, artifact).map_err(|_| RuntimeError::ModelIntegrity)?;
    let executable = checked_runtime(runtime_directory)?;
    let version_text = runtime_version(&executable)?;
    let acceleration = detect_acceleration(&executable)?;

    let mut command = Command::new(executable);
    command.args(["--model"]).arg(model).args([
        "--prompt",
        "Local runtime health check",
        "--n-predict",
        "1",
        "--ctx-size",
        "256",
        "--no-warmup",
        "--no-display-prompt",
        "--no-conversation",
        "--single-turn",
        "--simple-io",
        "--log-disable",
    ]);
    configure_acceleration(&mut command, acceleration);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    wait_for_child(&mut child, Duration::from_secs(90))?;
    Ok(RuntimeHealth {
        ready: true,
        runtime_version: version_text,
        acceleration,
        model_id: artifact.id.clone(),
    })
}

fn checked_runtime(runtime_directory: &Path) -> Result<std::path::PathBuf, RuntimeError> {
    let executable = runtime_directory.join("llama-cli");
    if !executable.is_file() {
        return Err(RuntimeError::MissingRuntime);
    }
    runtime_version(&executable)?;
    Ok(executable)
}

fn runtime_version(executable: &Path) -> Result<String, RuntimeError> {
    let version = Command::new(executable).arg("--version").output()?;
    let text = process_text(&version.stdout, &version.stderr);
    if !version.status.success() || !text.contains(EXPECTED_BUILD) {
        return Err(RuntimeError::VersionMismatch);
    }
    Ok(text)
}

fn detect_acceleration(executable: &Path) -> Result<RuntimeAcceleration, RuntimeError> {
    let devices = Command::new(executable).arg("--list-devices").output()?;
    if !devices.status.success() {
        return Err(RuntimeError::DeviceProbe);
    }
    if String::from_utf8_lossy(&devices.stdout)
        .to_ascii_lowercase()
        .contains("metal")
    {
        Ok(RuntimeAcceleration::Metal)
    } else {
        Ok(RuntimeAcceleration::Cpu)
    }
}

fn configure_acceleration(command: &mut Command, acceleration: RuntimeAcceleration) {
    match acceleration {
        RuntimeAcceleration::Metal => {
            command.args(["--gpu-layers", "99"]);
        }
        RuntimeAcceleration::Cpu => {
            command.args(["--device", "none", "--gpu-layers", "0", "--no-op-offload"]);
        }
    }
}

fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> Result<(), RuntimeError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(RuntimeError::InferenceFailed)
            };
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RuntimeError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn process_text(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    match (stdout.trim(), stderr.trim()) {
        ("", stderr) => stderr.to_owned(),
        (stdout, "") => stdout.to_owned(),
        (stdout, stderr) => format!("{stdout}\n{stderr}"),
    }
}

fn framed_json(output: &[u8]) -> Option<&[u8]> {
    let start = output.iter().position(|byte| *byte == b'{')?;
    let end = output.iter().rposition(|byte| *byte == b'}')?;
    (start < end).then_some(&output[start..=end])
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("private model failed integrity verification before load")]
    ModelIntegrity,
    #[error("bundled local inference runtime is missing")]
    MissingRuntime,
    #[error("bundled local inference runtime version did not match the signed build")]
    VersionMismatch,
    #[error("local inference device detection failed")]
    DeviceProbe,
    #[error("local inference runtime health check failed")]
    HealthCheckFailed,
    #[error("local inference process failed")]
    InferenceFailed,
    #[error("untrusted content contains a prompt-injection signal and requires review")]
    PromptInjection,
    #[error("local inference output failed strict extraction validation: {0}")]
    InvalidExtraction(crate::ExtractionError),
    #[error("local inference output did not contain one framed JSON object")]
    InvalidFraming,
    #[error("local inference runtime health check timed out")]
    Timeout,
    #[error("local inference runtime process failed")]
    Process(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_health_contract_serializes_without_private_input() {
        let health = RuntimeHealth {
            ready: true,
            runtime_version: "build 10434".into(),
            acceleration: RuntimeAcceleration::Metal,
            model_id: "catalog-model".into(),
        };
        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("runtimeVersion"));
        assert!(!json.contains("prompt"));
    }

    #[test]
    fn version_capture_accepts_llama_cpp_stderr_output() {
        let text = process_text(b"", b"version: 0.1.0-dev (build 10434, commit 7e4c0a968)\n");
        assert!(text.contains(EXPECTED_BUILD));
    }

    #[test]
    fn response_framing_ignores_cli_banner_and_footer() {
        let output = b"Loading model...\n{\"schemaVersion\":1}\nExiting...\n";
        assert_eq!(framed_json(output), Some(&b"{\"schemaVersion\":1}"[..]));
    }

    #[test]
    fn evaluation_failure_codes_do_not_include_private_content() {
        assert_eq!(
            evaluation_failure_code(&RuntimeError::InvalidExtraction(
                crate::ExtractionError::InvalidEvidence
            )),
            "invalid_evidence"
        );
        assert_eq!(evaluation_failure_code(&RuntimeError::Timeout), "timeout");
    }
}
