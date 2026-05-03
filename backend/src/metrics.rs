use std::io;
use std::path::Path;

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use prometheus::{IntCounterVec, IntGauge, Opts, Registry, TextEncoder, core::Collector};

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
        let retained_audio_bytes = IntGauge::new(
            "oracy_retained_audio_bytes",
            "Current retained accepted-audio bytes.",
        )
        .expect("retained audio metric definition is valid");

        register(&registry, worker_jobs_total.clone());
        register(&registry, retention_cleanup_artifacts_total.clone());
        register(&registry, retained_audio_bytes.clone());

        let metrics = Self {
            registry,
            worker_jobs_total,
            retention_cleanup_artifacts_total,
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

    fn refresh_retained_audio_bytes(&self, accepted_audio_dir: &Path) -> io::Result<()> {
        // v0.1.0 favors scrape-time filesystem truth over maintaining a second
        // byte ledger. If scrape latency matters later, this metric can be
        // moved behind a cached walk without changing the exposition contract.
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

fn retained_audio_bytes(path: &Path) -> io::Result<i64> {
    let mut total = 0_i64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total += retained_audio_bytes(&entry.path())?;
        } else if metadata.is_file() {
            total += metadata.len() as i64;
        }
    }
    Ok(total)
}
