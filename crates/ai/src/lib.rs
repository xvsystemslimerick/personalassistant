use serde::Serialize;
use std::path::Path;
use thiserror::Error;

mod evaluation;
mod extraction;
mod model;
mod runtime;
mod temporal;

pub use evaluation::{evaluate_case, evaluation_corpus, EvaluationCase};
pub use extraction::{
    build_extraction_prompt, build_extraction_repair_prompt, contains_prompt_injection_signal,
    deterministic_waiting_extraction, extraction_grammar, extraction_json_schema,
    normalize_explicit_source_semantics, parse_and_validate_extraction,
    parse_and_validate_model_output, required_semantic_classification,
    validate_semantic_consistency, validate_source_semantic_consistency, AppointmentCandidate,
    Classification, Evidence, Extraction, ExtractionError, TaskCandidate, Urgency,
    WaitingForCandidate,
};
pub use model::{
    download_model, model_status, remove_model, verify_model, DownloadProgress, ModelArtifact,
};
pub use runtime::{
    health_check, run_evaluation_corpus, run_fixture_extraction, EvaluationCaseResult,
    EvaluationReport, RuntimeAcceleration, RuntimeHealth,
};
pub use temporal::{
    resolve_temporal_expression, ResolvedTemporal, TemporalError, TemporalPrecision,
    TemporalRelation,
};

const GIB: u64 = 1024 * 1024 * 1024;
pub const QUALIFIED_RUNTIME_BUILD: &str = "build 10434";
pub const PERSISTENT_CORPUS_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiCapabilities {
    pub hardware: HardwareProfile,
    pub recommendation: ModelRecommendation,
    pub lifecycle: ModelLifecycle,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub architecture: String,
    pub logical_cpu_count: usize,
    pub physical_memory_bytes: u64,
    pub available_disk_bytes: u64,
    pub apple_silicon: bool,
    pub acceleration: Acceleration,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Acceleration {
    Metal,
    Cpu,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelTier {
    Compact,
    Standard,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecommendation {
    pub tier: ModelTier,
    pub estimated_download_bytes: u64,
    pub required_storage_bytes: u64,
    pub eligible: bool,
    pub reason: String,
    pub artifact: ModelArtifact,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelLifecycle {
    NotInstalled,
    Installed,
}

pub fn detect(model_directory: &Path) -> Result<AiCapabilities, CapabilityError> {
    let hardware = HardwareProfile {
        architecture: std::env::consts::ARCH.to_owned(),
        logical_cpu_count: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        physical_memory_bytes: physical_memory_bytes()?,
        available_disk_bytes: available_disk_bytes(model_directory)?,
        apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        acceleration: if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Acceleration::Metal
        } else {
            Acceleration::Cpu
        },
    };
    let recommendation = recommend_model(&hardware);
    let lifecycle = model_status(model_directory, &recommendation.artifact)
        .map_err(|_| CapabilityError::ModelStatus)?;
    Ok(AiCapabilities {
        hardware,
        recommendation,
        lifecycle,
    })
}

pub fn recommend_model(hardware: &HardwareProfile) -> ModelRecommendation {
    let standard = hardware.apple_silicon && hardware.physical_memory_bytes >= 16 * GIB;
    let (tier, artifact) = if standard {
        (ModelTier::Standard, model::standard_artifact())
    } else {
        (ModelTier::Compact, model::compact_artifact())
    };
    let estimated_download_bytes = artifact.size_bytes;
    let required_storage_bytes = estimated_download_bytes
        .saturating_mul(2)
        .saturating_add(GIB);
    let eligible = hardware.available_disk_bytes >= required_storage_bytes;
    let reason = if !eligible {
        "Not enough free storage for the model and safe download staging.".to_owned()
    } else if standard {
        "Apple Silicon and available memory support the standard private model tier.".to_owned()
    } else {
        "The compact tier preserves responsiveness on this hardware.".to_owned()
    };
    ModelRecommendation {
        tier,
        estimated_download_bytes,
        required_storage_bytes,
        eligible,
        reason,
        artifact,
    }
}

#[cfg(target_os = "macos")]
fn physical_memory_bytes() -> Result<u64, CapabilityError> {
    sysctl_u64(c"hw.memsize")
}

#[cfg(not(target_os = "macos"))]
fn physical_memory_bytes() -> Result<u64, CapabilityError> {
    Ok(0)
}

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &std::ffi::CStr) -> Result<u64, CapabilityError> {
    let mut value = 0_u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `value` is a valid writable u64, `size` matches it, and `name` is NUL-terminated.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 && size == std::mem::size_of::<u64>() {
        Ok(value)
    } else {
        Err(CapabilityError::HardwareProbe)
    }
}

#[cfg(target_os = "macos")]
fn available_disk_bytes(path: &Path) -> Result<u64, CapabilityError> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| CapabilityError::InvalidPath)?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `stats` points to writable storage for statvfs.
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(CapabilityError::StorageProbe);
    }
    // SAFETY: statvfs returned success and initialized `stats`.
    let stats = unsafe { stats.assume_init() };
    Ok(u64::from(stats.f_bavail).saturating_mul(stats.f_frsize))
}

#[cfg(not(target_os = "macos"))]
fn available_disk_bytes(_path: &Path) -> Result<u64, CapabilityError> {
    Err(CapabilityError::UnsupportedPlatform)
}

#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("hardware capability detection failed")]
    HardwareProbe,
    #[error("available storage detection failed")]
    StorageProbe,
    #[error("application model path was invalid")]
    InvalidPath,
    #[error("private model installation status could not be read")]
    ModelStatus,
    #[error("hardware detection is not implemented for this platform")]
    UnsupportedPlatform,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hardware(memory_gib: u64, disk_gib: u64, apple_silicon: bool) -> HardwareProfile {
        HardwareProfile {
            architecture: "aarch64".into(),
            logical_cpu_count: 8,
            physical_memory_bytes: memory_gib * GIB,
            available_disk_bytes: disk_gib * GIB,
            apple_silicon,
            acceleration: if apple_silicon {
                Acceleration::Metal
            } else {
                Acceleration::Cpu
            },
        }
    }

    #[test]
    fn recommends_standard_tier_for_suitable_apple_silicon() {
        let recommendation = recommend_model(&hardware(16, 20, true));
        assert_eq!(recommendation.tier, ModelTier::Standard);
        assert!(recommendation.eligible);
    }

    #[test]
    fn recommends_compact_tier_for_lower_memory() {
        let recommendation = recommend_model(&hardware(8, 20, true));
        assert_eq!(recommendation.tier, ModelTier::Compact);
        assert!(recommendation.eligible);
    }

    #[test]
    fn rejects_download_when_safe_staging_space_is_unavailable() {
        let recommendation = recommend_model(&hardware(16, 5, true));
        assert_eq!(recommendation.tier, ModelTier::Standard);
        assert!(!recommendation.eligible);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_current_macos_capabilities() {
        let capabilities = detect(&std::env::temp_dir()).unwrap();
        assert!(capabilities.hardware.physical_memory_bytes > 0);
        assert!(capabilities.hardware.available_disk_bytes > 0);
        assert!(capabilities.hardware.logical_cpu_count > 0);
    }
}
