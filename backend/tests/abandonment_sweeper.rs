use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use oracy_backend::abandonment_sweeper::{AbandonmentSweeperConfig, sweep_abandoned_jobs_once};
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::metrics::Metrics;
use oracy_backend::router::build_operator_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::{NewOpenTranscriptionJob, OpenJobOutcome, Storage};
use tempfile::TempDir;
use time::Duration;
use time::macros::datetime;
use tower::util::ServiceExt;

#[tokio::test]
async fn sweeper_fails_only_jobs_past_the_abandonment_window_and_counts_transitions() {
    let fixture = SweeperFixture::new().await;
    let old = fixture
        .open_job("attempt-old", datetime!(2026-04-24 17:00:00 UTC))
        .await;
    let recent = fixture
        .open_job("attempt-recent", datetime!(2026-04-25 17:30:00 UTC))
        .await;
    let finalized = fixture
        .open_job("attempt-finalized", datetime!(2026-04-24 17:15:00 UTC))
        .await;
    mark_open_job_queued(&fixture.storage, &finalized.id).await;

    let outcome = sweep_abandoned_jobs_once(
        &fixture.storage,
        &fixture.metrics,
        AbandonmentSweeperConfig {
            window: Duration::hours(24),
            interval: std::time::Duration::from_secs(300),
        },
        datetime!(2026-04-25 18:00:00 UTC),
    )
    .await
    .expect("sweep abandoned jobs");

    assert_eq!(outcome.abandoned_count, 1);
    assert_eq!(fixture.job_status(&old.id).await, "failed");
    assert_eq!(
        fixture.job_failure_code(&old.id).await.as_deref(),
        Some("submission_abandoned")
    );
    assert_eq!(fixture.job_status(&recent.id).await, "accepting_chunks");
    assert_eq!(fixture.job_status(&finalized.id).await, "queued");

    let metrics = fixture.operator_metrics().await;
    assert_metric_sample_has_value(&metrics, "oracy_transcription_abandonments_total", &[], "1");
}

struct SweeperFixture {
    _tempdir: TempDir,
    accepted_audio_dir: std::path::PathBuf,
    storage: Storage,
    metrics: Metrics,
}

impl SweeperFixture {
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
            metrics: Metrics::new(),
        }
    }

    async fn open_job(
        &self,
        idempotency_key: &str,
        now: time::OffsetDateTime,
    ) -> oracy_backend::storage::TranscriptionJobRecord {
        match self
            .storage
            .open_job(NewOpenTranscriptionJob {
                api_key_id: "alpha".to_owned(),
                idempotency_key: idempotency_key.to_owned(),
                recorded_at: datetime!(2026-04-24 16:59:00 UTC),
                session_id: None,
                language: Some("en".to_owned()),
                chunk_count: 1,
                audio_format: "wav".to_owned(),
                max_retries: 3,
                now,
            })
            .await
            .expect("open job")
        {
            OpenJobOutcome::Created(job) => job,
            other => panic!("expected opened job, got {other:?}"),
        }
    }

    async fn job_status(&self, job_id: &str) -> String {
        sqlx::query_scalar("SELECT status FROM transcription_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(self.storage.pool())
            .await
            .expect("job status")
    }

    async fn job_failure_code(&self, job_id: &str) -> Option<String> {
        sqlx::query_scalar("SELECT failure_code FROM transcription_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(self.storage.pool())
            .await
            .expect("job failure code")
    }

    async fn operator_metrics(&self) -> String {
        let auth_store = AuthStore::try_from_configs(&[ApiKeyConfig {
            api_key_id: "alpha".to_owned(),
            key: "alpha-secret".to_owned(),
        }])
        .expect("auth config");
        let state = AppState {
            accepted_audio_dir: self.accepted_audio_dir.clone(),
            auth_store: Arc::new(auth_store),
            metrics: self.metrics.clone(),
            operator_listen_addr: "127.0.0.1:9090".parse().expect("operator listen addr"),
            openai_api_key: "test-openai-key".to_owned(),
            storage: self.storage.clone(),
        };
        let response = build_operator_router(state)
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("metrics response");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        String::from_utf8(bytes.to_vec()).expect("utf8 metrics")
    }
}

async fn mark_open_job_queued(storage: &Storage, job_id: &str) {
    let result = sqlx::query(
        r#"
        UPDATE transcription_jobs
        SET audio_sha256_hex = 'queued-open-job-hash',
            accepted_audio_path = '/var/lib/oracy/accepted-audio/queued-open-job.wav',
            status = 'queued'
        WHERE api_key_id = 'alpha' AND id = ?
        "#,
    )
    .bind(job_id)
    .execute(storage.pool())
    .await
    .expect("mark open job queued");
    assert_eq!(result.rows_affected(), 1);
}

fn assert_metric_sample_has_value(body: &str, name: &str, labels: &[&str], value: &str) {
    assert!(
        body.lines().any(|line| {
            line.starts_with(name)
                && labels.iter().all(|label| line.contains(label))
                && line.ends_with(&format!(" {value}"))
        }),
        "expected {name} with labels {labels:?} and value {value} in:\n{body}"
    );
}
