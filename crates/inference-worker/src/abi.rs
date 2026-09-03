use std::{ffi::CString, path::Path};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum AbiError {
    #[error("runtime library path is invalid")]
    InvalidPath,
    #[error("runtime library could not be loaded")]
    LoadFailed,
    #[error("runtime library API is incompatible")]
    MissingSymbol,
}

unsafe extern "C" {
    fn pa_llama_probe_library(path: *const std::ffi::c_char) -> i32;
}

pub fn probe_runtime_library(path: &Path) -> Result<(), AbiError> {
    let encoded =
        CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| AbiError::InvalidPath)?;
    // SAFETY: `encoded` is a live NUL-terminated string for the duration of the
    // call. The C shim does not retain it and returns a numeric status only.
    match unsafe { pa_llama_probe_library(encoded.as_ptr()) } {
        0 => Ok(()),
        1 => Err(AbiError::InvalidPath),
        2 => Err(AbiError::LoadFailed),
        _ => Err(AbiError::MissingSymbol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_fails_without_exposing_loader_text() {
        assert_eq!(
            probe_runtime_library(Path::new("/definitely/not/a/library.dylib")),
            Err(AbiError::LoadFailed)
        );
    }

    #[test]
    fn path_with_nul_is_rejected_before_ffi() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"bad\0path"));
        assert_eq!(probe_runtime_library(path), Err(AbiError::InvalidPath));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundled_b10434_library_exports_required_persistent_api() {
        let library = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/src-tauri/resources/llama-runtime/macos-arm64")
            .join("libllama.0.1.0.dylib");
        assert_eq!(probe_runtime_library(&library), Ok(()));
    }
}
