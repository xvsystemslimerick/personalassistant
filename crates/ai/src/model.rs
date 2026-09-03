use crate::{ModelLifecycle, ModelTier};
use reqwest::{header, redirect::Policy, StatusCode};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

const CATALOG_LICENSE: &str = "Apache-2.0";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelArtifact {
    pub id: String,
    pub display_name: String,
    pub tier: ModelTier,
    pub revision: String,
    pub filename: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub license: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

pub(crate) fn compact_artifact() -> ModelArtifact {
    let revision = "90862c4b9d2787eaed51d12237eafdfe7c5f6077";
    let filename = "Qwen3-1.7B-Q8_0.gguf";
    ModelArtifact {
        id: "qwen3-1.7b-q8".into(),
        display_name: "Qwen3 1.7B Q8".into(),
        tier: ModelTier::Compact,
        revision: revision.into(),
        filename: filename.into(),
        url: format!("https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/{revision}/{filename}?download=true"),
        size_bytes: 1_834_426_016,
        sha256: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a".into(),
        license: CATALOG_LICENSE.into(),
    }
}

pub(crate) fn standard_artifact() -> ModelArtifact {
    let revision = "bc640142c66e1fdd12af0bd68f40445458f3869b";
    let filename = "Qwen3-4B-Q4_K_M.gguf";
    ModelArtifact {
        id: "qwen3-4b-q4-k-m".into(),
        display_name: "Qwen3 4B Q4_K_M".into(),
        tier: ModelTier::Standard,
        revision: revision.into(),
        filename: filename.into(),
        url: format!(
            "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/{revision}/{filename}?download=true"
        ),
        size_bytes: 2_497_280_256,
        sha256: "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5".into(),
        license: CATALOG_LICENSE.into(),
    }
}

pub fn model_status(
    directory: &Path,
    artifact: &ModelArtifact,
) -> Result<ModelLifecycle, ModelError> {
    let installed = directory.join(&artifact.filename);
    let marker = marker_path(directory, artifact);
    if !installed.exists() || !marker.exists() {
        return Ok(ModelLifecycle::NotInstalled);
    }
    let metadata = std::fs::metadata(installed)?;
    let recorded_hash = std::fs::read_to_string(marker)?;
    if metadata.len() == artifact.size_bytes && recorded_hash.trim() == artifact.sha256 {
        Ok(ModelLifecycle::Installed)
    } else {
        Ok(ModelLifecycle::NotInstalled)
    }
}

pub async fn download_model<F>(
    directory: &Path,
    artifact: &ModelArtifact,
    mut progress: F,
) -> Result<PathBuf, ModelError>
where
    F: FnMut(DownloadProgress) + Send,
{
    validate_artifact(artifact)?;
    tokio::fs::create_dir_all(directory).await?;
    let final_path = directory.join(&artifact.filename);
    let partial_path = directory.join(format!("{}.part", artifact.filename));
    let mut downloaded = tokio::fs::metadata(&partial_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if downloaded > artifact.size_bytes {
        tokio::fs::remove_file(&partial_path).await?;
        downloaded = 0;
    }

    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(std::time::Duration::from_secs(60 * 30))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !trusted_download_url(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let mut request = client.get(&artifact.url);
    if downloaded > 0 {
        request = request.header(header::RANGE, format!("bytes={downloaded}-"));
    }
    let mut response = request.send().await?;
    if response.status() != StatusCode::OK && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(ModelError::HttpStatus(response.status().as_u16()));
    }
    if downloaded > 0 && response.status() == StatusCode::OK {
        downloaded = 0;
    }
    validate_response_size(&response, downloaded, artifact.size_bytes)?;
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if downloaded == 0 {
        options.truncate(true);
    } else {
        options.append(true);
    }
    let mut file = options.open(&partial_path).await?;
    progress(DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes: artifact.size_bytes,
    });
    while let Some(chunk) = response.chunk().await? {
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or(ModelError::SizeMismatch)?;
        if downloaded > artifact.size_bytes {
            return Err(ModelError::SizeMismatch);
        }
        file.write_all(&chunk).await?;
        progress(DownloadProgress {
            downloaded_bytes: downloaded,
            total_bytes: artifact.size_bytes,
        });
    }
    file.flush().await?;
    drop(file);
    if downloaded != artifact.size_bytes || sha256_file(&partial_path)? != artifact.sha256 {
        return Err(ModelError::Integrity);
    }
    tokio::fs::rename(&partial_path, &final_path).await?;
    let marker = marker_path(directory, artifact);
    let marker_partial = directory.join(format!("{}.sha256.part", artifact.id));
    tokio::fs::write(&marker_partial, artifact.sha256.as_bytes()).await?;
    tokio::fs::rename(marker_partial, marker).await?;
    Ok(final_path)
}

pub async fn remove_model(directory: &Path, artifact: &ModelArtifact) -> Result<(), ModelError> {
    validate_artifact(artifact)?;
    for path in [
        directory.join(&artifact.filename),
        directory.join(format!("{}.part", artifact.filename)),
        marker_path(directory, artifact),
    ] {
        match tokio::fs::remove_file(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn validate_artifact(artifact: &ModelArtifact) -> Result<(), ModelError> {
    let parsed = reqwest::Url::parse(&artifact.url).map_err(|_| ModelError::UntrustedSource)?;
    if !trusted_download_url(&parsed)
        || artifact.filename.contains('/')
        || artifact.filename.contains('\\')
        || artifact.size_bytes == 0
        || artifact.sha256.len() != 64
        || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(ModelError::UntrustedSource);
    }
    Ok(())
}

fn trusted_download_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some(
                "huggingface.co"
                    | "cdn-lfs.huggingface.co"
                    | "cas-bridge.xethub.hf.co"
                    | "us.aws.cdn.hf.co"
            )
        )
}

fn validate_response_size(
    response: &reqwest::Response,
    existing: u64,
    expected: u64,
) -> Result<(), ModelError> {
    let remaining = expected
        .checked_sub(existing)
        .ok_or(ModelError::SizeMismatch)?;
    if response.content_length() != Some(remaining) {
        return Err(ModelError::SizeMismatch);
    }
    Ok(())
}

fn marker_path(directory: &Path, artifact: &ModelArtifact) -> PathBuf {
    directory.join(format!("{}.sha256", artifact.id))
}

fn sha256_file(path: &Path) -> Result<String, ModelError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn verify_model(directory: &Path, artifact: &ModelArtifact) -> Result<PathBuf, ModelError> {
    validate_artifact(artifact)?;
    let path = directory.join(&artifact.filename);
    if std::fs::metadata(&path)?.len() != artifact.size_bytes
        || sha256_file(&path)? != artifact.sha256
    {
        return Err(ModelError::Integrity);
    }
    Ok(path)
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("private model source was not trusted")]
    UntrustedSource,
    #[error("private model server returned HTTP {0}")]
    HttpStatus(u16),
    #[error("private model download size did not match the catalog")]
    SizeMismatch,
    #[error("private model failed integrity verification")]
    Integrity,
    #[error("private model storage operation failed")]
    Io(#[from] std::io::Error),
    #[error("private model network operation failed")]
    Network(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_pinned_to_https_revision_and_digest() {
        for artifact in [compact_artifact(), standard_artifact()] {
            validate_artifact(&artifact).unwrap();
            assert!(!artifact.url.contains("/main/"));
            assert_eq!(artifact.license, "Apache-2.0");
        }
    }

    #[test]
    fn official_hugging_face_delivery_hosts_are_narrowly_allowed() {
        assert!(trusted_download_url(
            &reqwest::Url::parse("https://us.aws.cdn.hf.co/xet-bridge-us/artifact").unwrap()
        ));
        assert!(!trusted_download_url(
            &reqwest::Url::parse("https://aws.cdn.hf.co.example.com/artifact").unwrap()
        ));
    }

    #[test]
    fn installed_status_requires_exact_size_and_verified_marker() {
        let mut artifact = compact_artifact();
        artifact.id = format!("fixture-{}", std::process::id());
        artifact.filename = format!("{}.gguf", artifact.id);
        artifact.size_bytes = 3;
        artifact.sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into();
        let directory = std::env::temp_dir().join(&artifact.id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(&artifact.filename), b"abc").unwrap();
        assert_eq!(
            model_status(&directory, &artifact).unwrap(),
            ModelLifecycle::NotInstalled
        );
        std::fs::write(marker_path(&directory, &artifact), &artifact.sha256).unwrap();
        assert_eq!(
            sha256_file(&directory.join(&artifact.filename)).unwrap(),
            artifact.sha256
        );
        assert_eq!(
            model_status(&directory, &artifact).unwrap(),
            ModelLifecycle::Installed
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn removal_is_scoped_and_idempotent() {
        let artifact = compact_artifact();
        let directory = std::env::temp_dir().join(format!("pa-ai-remove-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(&artifact.filename), b"fixture").unwrap();
        std::fs::write(directory.join("unrelated.txt"), b"keep").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            remove_model(&directory, &artifact).await.unwrap();
            remove_model(&directory, &artifact).await.unwrap();
        });
        assert!(directory.join("unrelated.txt").exists());
        std::fs::remove_file(directory.join("unrelated.txt")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
