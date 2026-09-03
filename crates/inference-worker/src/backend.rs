use ai::{
    build_extraction_prompt, build_extraction_repair_prompt, deterministic_waiting_extraction,
    extraction_grammar, normalize_explicit_source_semantics, parse_and_validate_model_output,
    required_semantic_classification, validate_source_semantic_consistency, Extraction,
};
use std::{ffi::CString, path::Path, ptr::NonNull};
use thiserror::Error;

const OUTPUT_CAPACITY: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend configuration is invalid")]
    InvalidConfiguration,
    #[error("persistent model could not be loaded")]
    ModelLoad,
    #[error("local generation failed at stage {0}")]
    Generation(i32),
    #[error("local generation returned invalid output")]
    InvalidOutput,
}

unsafe extern "C" {
    fn pa_llama_backend_open(
        library_path: *const std::ffi::c_char,
        runtime_dir: *const std::ffi::c_char,
        model_path: *const std::ffi::c_char,
        gpu_layers: i32,
    ) -> *mut std::ffi::c_void;
    fn pa_llama_backend_generate(
        backend: *mut std::ffi::c_void,
        prompt: *const std::ffi::c_char,
        grammar: *const std::ffi::c_char,
        max_tokens: i32,
        output: *mut std::ffi::c_char,
        output_capacity: usize,
    ) -> i32;
    fn pa_llama_backend_close(backend: *mut std::ffi::c_void);
}

pub struct PersistentBackend {
    inner: NonNull<std::ffi::c_void>,
}

impl PersistentBackend {
    pub fn open(
        runtime_directory: &Path,
        model_path: &Path,
        gpu_layers: i32,
    ) -> Result<Self, BackendError> {
        let library = path_string(&runtime_directory.join("libllama.0.1.0.dylib"))?;
        let runtime = path_string(runtime_directory)?;
        let model = path_string(model_path)?;
        // SAFETY: all strings are NUL-terminated and remain alive for the call.
        // The returned allocation is exclusively owned by this wrapper.
        let inner = unsafe {
            pa_llama_backend_open(
                library.as_ptr(),
                runtime.as_ptr(),
                model.as_ptr(),
                gpu_layers,
            )
        };
        Ok(Self {
            inner: NonNull::new(inner).ok_or(BackendError::ModelLoad)?,
        })
    }

    pub fn extract(&mut self, source: &str) -> Result<Extraction, BackendError> {
        if let Some(extraction) = deterministic_waiting_extraction(source) {
            return Ok(extraction);
        }
        let prompt = build_extraction_prompt(source).map_err(|_| BackendError::InvalidOutput)?;
        match self.generate_and_validate(source, &prompt) {
            Ok(extraction)
                if required_semantic_classification(source).is_some()
                    && !extraction.waiting_for.is_empty() =>
            {
                let normalized = normalize_explicit_source_semantics(source, extraction);
                validate_source_semantic_consistency(source, &normalized)
                    .map_err(|_| BackendError::InvalidOutput)?;
                Ok(normalized)
            }
            Ok(extraction) if validate_source_semantic_consistency(source, &extraction).is_ok() => {
                Ok(extraction)
            }
            Ok(extraction) => {
                let repair_classification =
                    required_semantic_classification(source).unwrap_or(extraction.classification);
                let repair = build_extraction_repair_prompt(source, repair_classification)
                    .map_err(|_| BackendError::InvalidOutput)?;
                let repaired = self.generate_and_validate(source, &repair)?;
                validate_source_semantic_consistency(source, &repaired)
                    .map_err(|_| BackendError::InvalidOutput)?;
                Ok(repaired)
            }
            Err(BackendError::InvalidOutput) => {
                let repair_classification =
                    required_semantic_classification(source).unwrap_or(ai::Classification::Other);
                let repair = build_extraction_repair_prompt(source, repair_classification)
                    .map_err(|_| BackendError::InvalidOutput)?;
                let repaired = self.generate_and_validate(source, &repair)?;
                validate_source_semantic_consistency(source, &repaired)
                    .map_err(|_| BackendError::InvalidOutput)?;
                Ok(repaired)
            }
            Err(error) => Err(error),
        }
    }

    fn generate_and_validate(
        &mut self,
        source: &str,
        prompt: &str,
    ) -> Result<Extraction, BackendError> {
        let prompt = CString::new(prompt).map_err(|_| BackendError::InvalidConfiguration)?;
        let grammar =
            CString::new(extraction_grammar()).map_err(|_| BackendError::InvalidConfiguration)?;
        let mut output = vec![0_u8; OUTPUT_CAPACITY];
        // SAFETY: the backend is exclusively borrowed, input pointers are live,
        // and the output buffer has the capacity passed to the C shim.
        let status = unsafe {
            pa_llama_backend_generate(
                self.inner.as_ptr(),
                prompt.as_ptr(),
                grammar.as_ptr(),
                512,
                output.as_mut_ptr().cast(),
                output.len(),
            )
        };
        if status != 0 {
            return Err(BackendError::Generation(status));
        }
        let length = output
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(BackendError::InvalidOutput)?;
        parse_and_validate_model_output(source, &output[..length])
            .map_err(|_| BackendError::InvalidOutput)
    }
}

impl Drop for PersistentBackend {
    fn drop(&mut self) {
        // SAFETY: `inner` was created by `pa_llama_backend_open`, is uniquely
        // owned, and Drop runs once.
        unsafe { pa_llama_backend_close(self.inner.as_ptr()) };
    }
}

fn path_string(path: &Path) -> Result<CString, BackendError> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| BackendError::InvalidConfiguration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_model_fails_closed() {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/src-tauri/resources/llama-runtime/macos-arm64");
        assert!(matches!(
            PersistentBackend::open(&runtime, Path::new("/missing/model.gguf"), 0),
            Err(BackendError::ModelLoad)
        ));
    }
}
