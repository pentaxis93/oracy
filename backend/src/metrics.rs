use std::io;
use std::path::Path;

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use prometheus::{
    IntCounter, IntCounterVec, IntGauge, Opts, Registry, TextEncoder, core::Collector,
};

use crate::state::AppState;

const PROMETHEUS_TEXT_FORMAT: &str = "text/plain; version=0.0.4; charset=utf-8";
const WORKER_OUTCOME: &str = "outcome";
const FAILURE_CLASS: &str = "failure_class";
const CLEANUP_OUTCOME: &str = "outcome";
const ARTIFACT: &str = "artifact";

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    worker_jobs_total: IntCounterVec,
    retention_cleanup_artifacts_total: IntCounterVec,
    transcription_abandonments_total: IntCounter,
    retained_audio_bytes: IntGauge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionArtifact {
    Chunk,
    ComposedAudio,
}

impl RetentionArtifact {
    fn as_label(self) -> &'static str {
        match self {
            Self::Chunk => "chunk",
            Self::ComposedAudio => "composed_audio",
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();
        let worker_jobs_total = IntCounterVec::new(
            Opts::new(
                "oracy_transcription_worker_jobs_total",
                "Transcription worker job outcomes.",
            ),
            &[WORKER_OUTCOME, FAILURE_CLASS],
        )
        .expect("worker metric definition is valid");
        let retention_cleanup_artifacts_total = IntCounterVec::new(
            Opts::new(
                "oracy_retention_cleanup_artifacts_total",
                "Retention cleanup artifact outcomes.",
            ),
            &[CLEANUP_OUTCOME, ARTIFACT],
        )
        .expect("retention cleanup metric definition is valid");
        let transcription_abandonments_total = IntCounter::new(
            "oracy_transcription_abandonments_total",
            "Transcription jobs abandoned before finalize.",
        )
        .expect("abandonment metric definition is valid");
        let retained_audio_bytes = IntGauge::new(
            "oracy_retained_audio_bytes",
            "Current retained accepted-audio bytes.",
        )
        .expect("retained audio metric definition is valid");

        register(&registry, worker_jobs_total.clone());
        register(&registry, retention_cleanup_artifacts_total.clone());
        register(&registry, transcription_abandonments_total.clone());
        register(&registry, retained_audio_bytes.clone());

        let metrics = Self {
            registry,
            worker_jobs_total,
            retention_cleanup_artifacts_total,
            transcription_abandonments_total,
            retained_audio_bytes,
        };
        metrics.initialize_series();
        metrics
    }

    pub fn record_worker_succeeded(&self) {
        self.worker_jobs_total
            .with_label_values(&["succeeded", "none"])
            .inc();
    }

    pub fn record_worker_retry_waiting(&self, failure_code: &str) {
        self.worker_jobs_total
            .with_label_values(&["retry_waiting", bounded_failure_class(failure_code)])
            .inc();
    }

    pub fn record_worker_failed(&self, failure_code: &str) {
        self.worker_jobs_total
            .with_label_values(&["failed", bounded_failure_class(failure_code)])
            .inc();
    }

    pub fn record_retention_cleanup_succeeded(&self, artifact: RetentionArtifact) {
        self.retention_cleanup_artifacts_total
            .with_label_values(&["succeeded", artifact.as_label()])
            .inc();
    }

    pub fn record_retention_cleanup_failed(&self, artifact: RetentionArtifact) {
        self.retention_cleanup_artifacts_total
            .with_label_values(&["failed", artifact.as_label()])
            .inc();
    }

    pub fn record_transcription_abandoned(&self) {
        self.transcription_abandonments_total.inc();
    }

    fn refresh_retained_audio_bytes(&self, accepted_audio_dir: &Path) -> io::Result<()> {
        // v0.1.0 favors scrape-time filesystem truth over maintaining a second
        // byte ledger. If scrape latency matters later, this metric can be
        // moved behind a cached walk without changing the exposition contract.
        // Concurrent mutation during the walk is normal; missing entries are
        // skipped silently.
        self.retained_audio_bytes
            .set(retained_audio_bytes(accepted_audio_dir)?);
        Ok(())
    }

    fn encode(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut output = String::new();
        encoder.encode_utf8(&metric_families, &mut output)?;
        Ok(output)
    }

    fn initialize_series(&self) {
        for (outcome, failure_class) in [
            ("succeeded", "none"),
            ("retry_waiting", "engine_timeout"),
            ("retry_waiting", "engine_rate_limited"),
            ("retry_waiting", "engine_error"),
            ("failed", "audio_invalid"),
            ("failed", "engine_timeout"),
            ("failed", "engine_rate_limited"),
            ("failed", "engine_error"),
            ("failed", "storage_error"),
            ("failed", "internal_error"),
        ] {
            let _ = self
                .worker_jobs_total
                .get_metric_with_label_values(&[outcome, failure_class])
                .expect("initialized worker metric label values are valid");
        }

        for (outcome, artifact) in [
            ("succeeded", "chunk"),
            ("succeeded", "composed_audio"),
            ("failed", "chunk"),
            ("failed", "composed_audio"),
        ] {
            let _ = self
                .retention_cleanup_artifacts_total
                .get_metric_with_label_values(&[outcome, artifact])
                .expect("initialized retention metric label values are valid");
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn prometheus_metrics(State(state): State<AppState>) -> impl IntoResponse {
    if state
        .metrics
        .refresh_retained_audio_bytes(&state.accepted_audio_dir)
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "failed to collect metrics\n".to_owned(),
        );
    }

    match state.metrics.encode() {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, PROMETHEUS_TEXT_FORMAT)],
            body,
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "failed to encode metrics\n".to_owned(),
        ),
    }
}

fn bounded_failure_class(failure_code: &str) -> &'static str {
    match failure_code {
        "audio_invalid" => "audio_invalid",
        "engine_timeout" => "engine_timeout",
        "engine_rate_limited" => "engine_rate_limited",
        "engine_error" => "engine_error",
        "storage_error" => "storage_error",
        "internal_error" => "internal_error",
        _ => "internal_error",
    }
}

fn register<C>(registry: &Registry, collector: C)
where
    C: Collector + Clone + 'static,
{
    registry
        .register(Box::new(collector))
        .expect("metric registration is valid");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedAudioEntryMetadata {
    Directory,
    File { len: u64 },
    Other,
}

trait RetainedAudioFilesystem {
    fn read_dir_paths(&self, path: &Path) -> io::Result<Vec<std::path::PathBuf>>;

    fn metadata(&self, path: &Path) -> io::Result<RetainedAudioEntryMetadata>;
}

struct StdRetainedAudioFilesystem;

impl RetainedAudioFilesystem for StdRetainedAudioFilesystem {
    fn read_dir_paths(&self, path: &Path) -> io::Result<Vec<std::path::PathBuf>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect()
    }

    fn metadata(&self, path: &Path) -> io::Result<RetainedAudioEntryMetadata> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            Ok(RetainedAudioEntryMetadata::Directory)
        } else if metadata.is_file() {
            Ok(RetainedAudioEntryMetadata::File {
                len: metadata.len(),
            })
        } else {
            Ok(RetainedAudioEntryMetadata::Other)
        }
    }
}

fn retained_audio_bytes(path: &Path) -> io::Result<i64> {
    retained_audio_bytes_in(path, &StdRetainedAudioFilesystem)
}

fn retained_audio_bytes_in(
    path: &Path,
    filesystem: &impl RetainedAudioFilesystem,
) -> io::Result<i64> {
    let mut total = 0_i64;
    for entry_path in filesystem.read_dir_paths(path)? {
        total += retained_audio_entry_bytes_in(&entry_path, filesystem)?;
    }
    Ok(total)
}

#[cfg(test)]
fn retained_audio_entry_bytes(path: &Path) -> io::Result<i64> {
    retained_audio_entry_bytes_in(path, &StdRetainedAudioFilesystem)
}

fn retained_audio_entry_bytes_in(
    path: &Path,
    filesystem: &impl RetainedAudioFilesystem,
) -> io::Result<i64> {
    let metadata = match filesystem.metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };

    match metadata {
        RetainedAudioEntryMetadata::Directory => match retained_audio_bytes_in(path, filesystem) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        },
        RetainedAudioEntryMetadata::File { len } => Ok(len as i64),
        RetainedAudioEntryMetadata::Other => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{
        RetainedAudioEntryMetadata, RetainedAudioFilesystem, retained_audio_bytes_in,
        retained_audio_entry_bytes,
    };

    struct MockRetainedAudioFilesystem {
        root: PathBuf,
        remaining: PathBuf,
        vanished: PathBuf,
    }

    impl RetainedAudioFilesystem for MockRetainedAudioFilesystem {
        fn read_dir_paths(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
            if path == self.root {
                Ok(vec![self.remaining.clone(), self.vanished.clone()])
            } else {
                panic!("unexpected read_dir path: {}", path.display());
            }
        }

        fn metadata(&self, path: &Path) -> io::Result<RetainedAudioEntryMetadata> {
            if path == self.remaining {
                Ok(RetainedAudioEntryMetadata::File { len: 5 })
            } else if path == self.vanished {
                Err(io::Error::new(io::ErrorKind::NotFound, "vanished"))
            } else {
                panic!("unexpected metadata path: {}", path.display());
            }
        }
    }

    #[test]
    fn retained_audio_entry_bytes_skips_entries_removed_before_metadata_read() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let vanished = tempdir.path().join("vanished.wav");

        let bytes = retained_audio_entry_bytes(&vanished).expect("vanished entry is ignored");

        assert_eq!(bytes, 0);
    }

    #[test]
    fn retained_audio_bytes_skips_entries_removed_after_directory_snapshot() {
        let root = PathBuf::from("/accepted-audio");
        let remaining = root.join("remaining.wav");
        let vanished = root.join("vanished.wav");
        let filesystem = MockRetainedAudioFilesystem {
            root: root.clone(),
            remaining,
            vanished,
        };

        let bytes = retained_audio_bytes_in(&root, &filesystem).expect("walk should succeed");

        assert_eq!(bytes, 5);
    }
}
