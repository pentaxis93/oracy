use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::audio_durability::sync_parent_after_entry_removal;
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
    #[error("failed to sync retained audio deletion parent for {path}: {source}")]
    SyncDeletion {
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

    async fn after_release_recorded(&self, _artifact: &RetainedAudioArtifact) {}
}

trait RetainedAudioDeletionSync: Send + Sync {
    fn sync_parent_after_entry_removal(&self, target: &Path) -> io::Result<()>;
}

#[derive(Debug)]
struct PosixRetainedAudioDeletionSync;

impl RetainedAudioDeletionSync for PosixRetainedAudioDeletionSync {
    fn sync_parent_after_entry_removal(&self, target: &Path) -> io::Result<()> {
        sync_parent_after_entry_removal(target)
    }
}

#[derive(Clone)]
pub struct FileRetainedAudioReleaser {
    accepted_audio_dir: PathBuf,
    deletion_sync: Arc<dyn RetainedAudioDeletionSync>,
}

impl FileRetainedAudioReleaser {
    pub fn new(accepted_audio_dir: PathBuf) -> Self {
        Self {
            accepted_audio_dir,
            deletion_sync: Arc::new(PosixRetainedAudioDeletionSync),
        }
    }

    #[cfg(test)]
    fn new_with_deletion_sync(
        accepted_audio_dir: PathBuf,
        deletion_sync: Arc<dyn RetainedAudioDeletionSync>,
    ) -> Self {
        Self {
            accepted_audio_dir,
            deletion_sync,
        }
    }
}

impl RetainedAudioReleaser for FileRetainedAudioReleaser {
    async fn release(&self, artifact: &RetainedAudioArtifact) -> Result<(), RetentionCleanupError> {
        let target = safe_retained_audio_path(&self.accepted_audio_dir, &artifact.path).await?;
        match tokio::fs::remove_file(&target).await {
            Ok(()) => {
                self.deletion_sync
                    .sync_parent_after_entry_removal(&target)
                    .map_err(|source| RetentionCleanupError::SyncDeletion {
                        path: target,
                        source,
                    })?;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.deletion_sync
                    .sync_parent_after_entry_removal(&target)
                    .map_err(|source| RetentionCleanupError::SyncDeletion {
                        path: target,
                        source,
                    })?;
                Ok(())
            }
            Err(source) => Err(RetentionCleanupError::Remove {
                path: target,
                source,
            }),
        }
    }

    async fn after_release_recorded(&self, artifact: &RetainedAudioArtifact) {
        if let Ok(target) = safe_retained_audio_path(&self.accepted_audio_dir, &artifact.path).await
        {
            cleanup_empty_parent_dirs(&self.accepted_audio_dir, target.parent()).await;
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
                    releaser.after_release_recorded(&artifact).await;
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;
    use time::macros::datetime;

    use super::*;
    use crate::metrics::Metrics;
    use crate::storage::{AcceptJobOutcome, NewTranscriptionJob};

    #[tokio::test]
    async fn cleanup_records_release_after_releaser_reports_durable_absence() {
        let fixture = RetentionCleanupFixture::new().await;
        let artifact_path = fixture
            .create_terminal_retained_audio_job("durable-ordering")
            .await;
        let artifact = fixture.retained_artifacts().await.remove(0);
        let events = Arc::new(Mutex::new(Vec::new()));
        let releaser = RecordingReleaser {
            events: Arc::clone(&events),
            storage: fixture.storage.clone(),
        };

        cleanup_artifacts(&fixture.storage, &Metrics::new(), &releaser, vec![artifact])
            .await
            .expect("cleanup artifacts");

        assert_eq!(
            events.lock().expect("events").as_slice(),
            ["remove_file", "sync_parent_dir", "db_release_recorded"]
        );
        assert_eq!(fixture.retained_artifact_count().await, 0);
        assert!(
            tokio::fs::try_exists(&artifact_path)
                .await
                .expect("artifact existence check"),
            "recording releaser does not remove the file"
        );
    }

    #[tokio::test]
    async fn parent_directory_sync_failure_keeps_retained_audio_retryable() {
        let fixture = RetentionCleanupFixture::new().await;
        let artifact_path = fixture
            .create_terminal_retained_audio_job("sync-failure")
            .await;
        let releaser = FileRetainedAudioReleaser::new_with_deletion_sync(
            fixture.accepted_audio_dir.clone(),
            Arc::new(FailingDeletionSync),
        );

        cleanup_retained_audio_once_with_releaser(&fixture.storage, &Metrics::new(), &releaser)
            .await
            .expect("cleanup failure is recorded internally");

        assert!(
            !tokio::fs::try_exists(&artifact_path)
                .await
                .expect("artifact existence check"),
            "unlink happens before the failed parent-directory sync"
        );
        assert_eq!(fixture.retained_artifact_count().await, 1);
        assert_eq!(fixture.composed_cleanup_attempts().await, 1);
    }

    #[tokio::test]
    async fn cleanup_retries_after_sync_failure_when_retained_audio_file_is_already_absent() {
        let fixture = RetentionCleanupFixture::new().await;
        let artifact_path = fixture
            .create_terminal_retained_audio_job("sync-failure-retry")
            .await;
        let failing_releaser = FileRetainedAudioReleaser::new_with_deletion_sync(
            fixture.accepted_audio_dir.clone(),
            Arc::new(FailingDeletionSync),
        );
        cleanup_retained_audio_once_with_releaser(
            &fixture.storage,
            &Metrics::new(),
            &failing_releaser,
        )
        .await
        .expect("initial cleanup failure is recorded internally");

        let retry_releaser = FileRetainedAudioReleaser::new_with_deletion_sync(
            fixture.accepted_audio_dir.clone(),
            Arc::new(RecordingDeletionSync::default()),
        );
        cleanup_retained_audio_once_with_releaser(
            &fixture.storage,
            &Metrics::new(),
            &retry_releaser,
        )
        .await
        .expect("retry cleanup");

        assert!(
            !tokio::fs::try_exists(&artifact_path)
                .await
                .expect("artifact existence check"),
            "retry observes the retained file is already gone"
        );
        assert_eq!(fixture.retained_artifact_count().await, 0);
        assert_eq!(fixture.composed_cleanup_attempts().await, 1);
    }

    #[derive(Default)]
    struct RecordingDeletionSync;

    impl RetainedAudioDeletionSync for RecordingDeletionSync {
        fn sync_parent_after_entry_removal(&self, _target: &Path) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailingDeletionSync;

    impl RetainedAudioDeletionSync for FailingDeletionSync {
        fn sync_parent_after_entry_removal(&self, _target: &Path) -> io::Result<()> {
            Err(io::Error::other("simulated parent-directory sync failure"))
        }
    }

    struct RecordingReleaser {
        events: Arc<Mutex<Vec<&'static str>>>,
        storage: Storage,
    }

    impl RetainedAudioReleaser for RecordingReleaser {
        async fn release(
            &self,
            _artifact: &RetainedAudioArtifact,
        ) -> Result<(), RetentionCleanupError> {
            self.events.lock().expect("events").push("remove_file");
            self.events.lock().expect("events").push("sync_parent_dir");
            Ok(())
        }

        async fn after_release_recorded(&self, _artifact: &RetainedAudioArtifact) {
            assert!(
                self.storage
                    .retained_audio_artifacts_for_terminal_jobs()
                    .await
                    .expect("retained artifacts")
                    .is_empty(),
                "storage row is cleared before post-recorded release hook"
            );
            self.events
                .lock()
                .expect("events")
                .push("db_release_recorded");
        }
    }

    struct RetentionCleanupFixture {
        _tempdir: TempDir,
        accepted_audio_dir: PathBuf,
        storage: Storage,
    }

    impl RetentionCleanupFixture {
        async fn new() -> Self {
            let tempdir = TempDir::new().expect("tempdir");
            let accepted_audio_dir = tempdir.path().join("accepted-audio");
            tokio::fs::create_dir(&accepted_audio_dir)
                .await
                .expect("create accepted audio dir");
            let storage = Storage::connect(&tempdir.path().join("oracy.sqlite"))
                .await
                .expect("connect storage");
            Self {
                _tempdir: tempdir,
                accepted_audio_dir,
                storage,
            }
        }

        async fn create_terminal_retained_audio_job(&self, idempotency_key: &str) -> PathBuf {
            let artifact_path = self
                .accepted_audio_dir
                .join(format!("{idempotency_key}.wav"));
            tokio::fs::write(&artifact_path, b"retained audio")
                .await
                .expect("write retained audio");
            let job = match self
                .storage
                .accept_job(NewTranscriptionJob {
                    api_key_id: "owner-a".to_owned(),
                    idempotency_key: idempotency_key.to_owned(),
                    audio_sha256_hex: "audio-hash".to_owned(),
                    recorded_at: datetime!(2026-04-24 17:59:00 UTC),
                    session_id: None,
                    language: Some("en".to_owned()),
                    accepted_audio_path: artifact_path.clone(),
                    max_retries: 3,
                    now: datetime!(2026-04-24 18:00:00 UTC),
                })
                .await
                .expect("accept job")
            {
                AcceptJobOutcome::Created(job) => job,
                other => panic!("expected created job, got {other:?}"),
            };
            let result = sqlx::query(
                r#"
                UPDATE transcription_jobs
                SET status = 'failed',
                    failure_code = 'engine_error',
                    failure_message = 'terminal failure',
                    retryable_by_client = 1
                WHERE api_key_id = 'owner-a' AND id = ?
                "#,
            )
            .bind(&job.id)
            .execute(self.storage.pool())
            .await
            .expect("mark job failed");
            assert_eq!(result.rows_affected(), 1);
            artifact_path
        }

        async fn retained_artifacts(&self) -> Vec<RetainedAudioArtifact> {
            self.storage
                .retained_audio_artifacts_for_terminal_jobs()
                .await
                .expect("retained artifacts")
        }

        async fn retained_artifact_count(&self) -> i64 {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM transcription_jobs
                WHERE api_key_id = 'owner-a' AND accepted_audio_path <> ''
                "#,
            )
            .fetch_one(self.storage.pool())
            .await
            .expect("retained artifact count")
        }

        async fn composed_cleanup_attempts(&self) -> i64 {
            sqlx::query_scalar(
                r#"
                SELECT accepted_audio_cleanup_attempts
                FROM transcription_jobs
                WHERE api_key_id = 'owner-a'
                "#,
            )
            .fetch_one(self.storage.pool())
            .await
            .expect("cleanup attempts")
        }
    }
}
