use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::metrics::{Metrics, RetentionArtifact};
use crate::storage::{RetainedAudioArtifact, RetainedAudioArtifactKind, Storage, StorageError};
use thiserror::Error;

const DEFAULT_CLEANUP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct RetentionCleanupConfig {
    pub interval: std::time::Duration,
}

impl Default for RetentionCleanupConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_CLEANUP_INTERVAL,
        }
    }
}

impl RetentionCleanupConfig {
    pub fn test() -> Self {
        Self {
            interval: std::time::Duration::from_millis(10),
        }
    }
}

#[derive(Debug, Error)]
pub enum RetentionCleanupError {
    #[error("retained audio path is outside accepted audio directory: {0}")]
    UnsafePath(PathBuf),
    #[error("failed to inspect retained audio path {path}: {source}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to remove retained audio path {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Storage(#[from] StorageError),
}

#[allow(async_fn_in_trait)]
pub trait RetainedAudioReleaser {
    async fn release(&self, artifact: &RetainedAudioArtifact) -> Result<(), RetentionCleanupError>;
}

#[derive(Debug, Clone)]
pub struct FileRetainedAudioReleaser {
    accepted_audio_dir: PathBuf,
}

impl FileRetainedAudioReleaser {
    pub fn new(accepted_audio_dir: PathBuf) -> Self {
        Self { accepted_audio_dir }
    }
}

impl RetainedAudioReleaser for FileRetainedAudioReleaser {
    async fn release(&self, artifact: &RetainedAudioArtifact) -> Result<(), RetentionCleanupError> {
        let target = safe_retained_audio_path(&self.accepted_audio_dir, &artifact.path).await?;
        match tokio::fs::remove_file(&target).await {
            Ok(()) => {
                cleanup_empty_parent_dirs(&self.accepted_audio_dir, target.parent()).await;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(RetentionCleanupError::Remove {
                path: target,
                source,
            }),
        }
    }
}

pub async fn cleanup_retained_audio_for_job(
    storage: &Storage,
    accepted_audio_dir: &Path,
    api_key_id: &str,
    job_id: &str,
    metrics: &Metrics,
) -> Result<(), RetentionCleanupError> {
    let releaser = FileRetainedAudioReleaser::new(accepted_audio_dir.to_path_buf());
    cleanup_retained_audio_for_job_with_releaser(storage, api_key_id, job_id, metrics, &releaser)
        .await
}

pub async fn cleanup_retained_audio_for_job_with_releaser<R>(
    storage: &Storage,
    api_key_id: &str,
    job_id: &str,
    metrics: &Metrics,
    releaser: &R,
) -> Result<(), RetentionCleanupError>
where
    R: RetainedAudioReleaser,
{
    let artifacts = storage
        .retained_audio_artifacts_for_job(api_key_id, job_id)
        .await?;
    cleanup_artifacts(storage, metrics, releaser, artifacts).await
}

pub async fn cleanup_retained_audio_once(
    storage: &Storage,
    accepted_audio_dir: &Path,
    metrics: &Metrics,
) -> Result<(), RetentionCleanupError> {
    let releaser = FileRetainedAudioReleaser::new(accepted_audio_dir.to_path_buf());
    cleanup_retained_audio_once_with_releaser(storage, metrics, &releaser).await
}

pub async fn cleanup_retained_audio_once_with_releaser<R>(
    storage: &Storage,
    metrics: &Metrics,
    releaser: &R,
) -> Result<(), RetentionCleanupError>
where
    R: RetainedAudioReleaser,
{
    let artifacts = storage.retained_audio_artifacts_for_terminal_jobs().await?;
    cleanup_artifacts(storage, metrics, releaser, artifacts).await
}

pub async fn run_retention_cleanup_loop(
    storage: Storage,
    accepted_audio_dir: PathBuf,
    metrics: Metrics,
    config: RetentionCleanupConfig,
) {
    loop {
        if let Err(error) =
            cleanup_retained_audio_once(&storage, &accepted_audio_dir, &metrics).await
        {
            tracing::error!("retention cleanup sweep failed: {error}");
        }
        tokio::time::sleep(config.interval).await;
    }
}

async fn cleanup_artifacts<R>(
    storage: &Storage,
    metrics: &Metrics,
    releaser: &R,
    artifacts: Vec<RetainedAudioArtifact>,
) -> Result<(), RetentionCleanupError>
where
    R: RetainedAudioReleaser,
{
    for artifact in artifacts {
        match releaser.release(&artifact).await {
            Ok(()) => {
                let released = storage
                    .mark_retained_audio_artifact_released(&artifact)
                    .await?;
                if released {
                    metrics.record_retention_cleanup_succeeded(metric_artifact(&artifact));
                    tracing::info!(
                        job_id = %artifact.job_id,
                        artifact_kind = artifact_label(&artifact),
                        artifact_path = %artifact.path.display(),
                        "retention cleanup released artifact"
                    );
                }
            }
            Err(error) => {
                let failure_reason = error.to_string();
                let attempt_count = storage
                    .record_retained_audio_artifact_cleanup_failure(&artifact)
                    .await?;
                metrics.record_retention_cleanup_failed(metric_artifact(&artifact));
                tracing::error!(
                    job_id = %artifact.job_id,
                    artifact_kind = artifact_label(&artifact),
                    artifact_path = %artifact.path.display(),
                    failure_reason = %failure_reason,
                    attempt_count,
                    "retention cleanup failed to release artifact"
                );
            }
        }
    }
    Ok(())
}

async fn safe_retained_audio_path(
    accepted_audio_dir: &Path,
    path: &Path,
) -> Result<PathBuf, RetentionCleanupError> {
    let root = tokio::fs::canonicalize(accepted_audio_dir)
        .await
        .map_err(|source| RetentionCleanupError::Inspect {
            path: accepted_audio_dir.to_path_buf(),
            source,
        })?;
    let target = if path.is_absolute() {
        path.to_path_buf()
    } else {
        accepted_audio_dir.join(path)
    };
    let target = lexically_normalize_path(&target);

    match tokio::fs::canonicalize(&target).await {
        Ok(canonical_target) => {
            if canonical_target.starts_with(&root) {
                Ok(target)
            } else {
                Err(RetentionCleanupError::UnsafePath(target))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let resolved_target = resolve_missing_path(&target).await.map_err(|source| {
                RetentionCleanupError::Inspect {
                    path: target.clone(),
                    source,
                }
            })?;
            if resolved_target.starts_with(&root) {
                Ok(target)
            } else {
                Err(RetentionCleanupError::UnsafePath(target))
            }
        }
        Err(source) => Err(RetentionCleanupError::Inspect {
            path: target,
            source,
        }),
    }
}

fn lexically_normalize_path(path: &Path) -> PathBuf {
    let mut parts = Vec::new();
    let mut has_root = false;

    for component in path.components() {
        match component {
            Component::RootDir => {
                has_root = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() && !has_root {
                    parts.push(OsString::from(".."));
                }
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::Prefix(_) => {}
        }
    }

    let mut normalized = if has_root {
        PathBuf::from("/")
    } else {
        PathBuf::new()
    };
    for part in parts {
        normalized.push(part);
    }
    normalized
}

async fn resolve_missing_path(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();

    loop {
        match tokio::fs::canonicalize(&ancestor).await {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(file_name) = ancestor.file_name() else {
                    return Err(error);
                };
                missing.push(file_name.to_os_string());
                if !ancestor.pop() {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

async fn cleanup_empty_parent_dirs(root: &Path, parent: Option<&Path>) {
    let Some(parent) = parent else {
        return;
    };
    let Ok(root) = tokio::fs::canonicalize(root).await else {
        return;
    };
    let mut current = parent.to_path_buf();
    while current.starts_with(&root) && current != root {
        if tokio::fs::remove_dir(&current).await.is_err() {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
}

fn artifact_label(artifact: &RetainedAudioArtifact) -> &'static str {
    match artifact.kind {
        RetainedAudioArtifactKind::Chunk => "chunk",
        RetainedAudioArtifactKind::ComposedAudio => "composed_audio",
    }
}

fn metric_artifact(artifact: &RetainedAudioArtifact) -> RetentionArtifact {
    match artifact.kind {
        RetainedAudioArtifactKind::Chunk => RetentionArtifact::Chunk,
        RetainedAudioArtifactKind::ComposedAudio => RetentionArtifact::ComposedAudio,
    }
}
