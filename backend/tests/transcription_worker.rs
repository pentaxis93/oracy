use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Multipart;
use axum::http::Request;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::embedding::{
    EmbeddingEngine, EmbeddingFailure, EmbeddingInput, EmbeddingOutput,
};
use oracy_backend::metrics::Metrics;
use oracy_backend::retention_cleanup::{
    RetainedAudioReleaser, RetentionCleanupError, cleanup_retained_audio_once,
    cleanup_retained_audio_once_with_releaser,
};
use oracy_backend::router::build_operator_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::{
    AcceptJobOutcome, AcceptedChunk, FinalizeJobOutcome, NewOpenTranscriptionJob,
    NewTranscriptionJob, OpenJobOutcome, Storage, StoreChunkOutcome, decode_embedding_vector,
};
use oracy_backend::transcription_worker::{
    AudioSliceError, AudioSlicer, DurationProbe, DurationProbeError, EngineFailure,
    OpenAiTranscriptionEngine, ProcessOutcome, TranscriptionEngine, TranscriptionInput,
    TranscriptionOutput, WorkerConfig, process_one_available_job,
};
use serde_json::json;
use sqlx::Row;
use tempfile::TempDir;
use time::Duration as TimeDuration;
use time::OffsetDateTime;
use time::macros::datetime;
use tokio::sync::Barrier;
use tower::util::ServiceExt;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

#[tokio::test]
async fn worker_materializes_a_queued_job_with_a_fake_engine() {
    let fixture = WorkerFixture::new().await;
    let job = fixture.create_queued_job("attempt-1", b"hello audio").await;
    let engine = FakeEngine::success("transcribed text");
    let embedding_engine = FakeEmbeddingEngine::success(vec![0.25, 0.5, 0.75]);
    let probe = FakeDurationProbe::success(1_480);

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &embedding_engine,
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Succeeded {
            job_id: job.id.clone()
        }
    );
    assert_eq!(engine.inputs.lock().expect("inputs").len(), 1);
    assert_eq!(
        engine.inputs.lock().expect("inputs")[0].model,
        "gpt-4o-mini-transcribe"
    );
    let completed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed.status, "succeeded");
    let voice_note = fixture
        .storage
        .get_voice_note(
            "owner-a",
            completed.voice_note_id.as_deref().expect("voice note id"),
        )
        .await
        .expect("voice note lookup")
        .expect("voice note exists");
    assert_eq!(voice_note.text, "transcribed text");
    assert_eq!(voice_note.audio_duration_seconds, 1.48);
    assert_eq!(voice_note.model, "gpt-4o-mini-transcribe");
    let embedding = fixture
        .storage
        .get_current_embedding("owner-a", &voice_note.id)
        .await
        .expect("embedding lookup")
        .expect("current embedding exists before succeeded");
    assert_eq!(embedding.model, "text-embedding-3-small");
    assert_eq!(
        decode_embedding_vector(&embedding.vector).expect("encoded vector"),
        vec![0.25, 0.5, 0.75]
    );
    let segments = fixture
        .storage
        .list_segments("owner-a", &voice_note.id)
        .await
        .expect("segments");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].start_ms, 0);
    assert_eq!(segments[0].end_ms, 1_480);
    assert_eq!(segments[0].text, "transcribed text");
}

#[tokio::test]
async fn worker_releases_chunks_and_composed_audio_after_success() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_chunked_queued_job(
            "attempt-success-cleanup",
            &[b"first audio", b"second audio"],
        )
        .await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;
    for artifact in &artifacts {
        assert!(
            tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact before processing: {}",
            artifact.display()
        );
    }
    let engine = FakeEngine::success("transcribed text");
    let probe = FakeDurationProbe::success(1_480);

    let metrics = Metrics::new();

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &metrics,
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Succeeded {
            job_id: job.id.clone()
        }
    );
    for artifact in &artifacts {
        assert!(
            !tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact released after success: {}",
            artifact.display()
        );
    }
    assert_eq!(fixture.chunk_row_count(&job.id).await, 2);
    let completed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.chunks_received, 2);
    let voice_note = fixture
        .storage
        .get_voice_note(
            "owner-a",
            completed.voice_note_id.as_deref().expect("voice note id"),
        )
        .await
        .expect("voice note lookup")
        .expect("voice note exists");
    assert_eq!(voice_note.audio_duration_seconds, 1.48);
    assert_eq!(voice_note.audio_format, "wav");
    assert_eq!(voice_note.audio_size_bytes, 23);

    let body = operator_metrics_text(&fixture, metrics).await;
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="succeeded""#, r#"artifact="chunk""#],
        "2",
    );
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="succeeded""#, r#"artifact="composed_audio""#],
        "1",
    );
}

#[tokio::test]
async fn worker_releases_chunks_and_composed_audio_after_failure() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_chunked_queued_job("attempt-failed-cleanup", &[b"bad audio"])
        .await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;
    let engine = FakeEngine::success("unused");
    let probe = FakeDurationProbe::invalid_audio();

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Failed {
            job_id: job.id.clone()
        }
    );
    for artifact in &artifacts {
        assert!(
            !tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact released after failure: {}",
            artifact.display()
        );
    }
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.failure_code.as_deref(), Some("audio_invalid"));
    assert_eq!(failed.retryable_by_client, Some(false));
    assert_eq!(failed.chunks_received, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn retained_terminal_audio_is_released_by_restart_sweep() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_chunked_queued_job("attempt-restart-cleanup", &[b"first", b"second"])
        .await;
    fixture.mark_job_failed(&job.id).await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;
    let events = CapturedEvents::default();
    let _guard = tracing::subscriber::set_default(Registry::default().with(events.clone()));

    cleanup_retained_audio_once(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &Metrics::new(),
    )
    .await
    .expect("cleanup sweep");

    for artifact in &artifacts {
        assert!(
            !tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact released by sweep: {}",
            artifact.display()
        );
    }
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");
    assert!(events.contains_fields(&[
        ("message", "retention cleanup released artifact"),
        ("job_id", &job.id),
        ("artifact_kind", "chunk"),
    ]));
    assert!(events.contains_fields(&[
        ("message", "retention cleanup released artifact"),
        ("job_id", &job.id),
        ("artifact_kind", "composed_audio"),
    ]));
}

#[tokio::test]
async fn cleanup_releases_audio_when_accepted_audio_dir_contains_parent_components() {
    let fixture = WorkerFixture::new_with_parent_component_accepted_audio_dir().await;
    let job = fixture
        .create_queued_job("attempt-parent-component-cleanup", b"hello audio")
        .await;
    fixture.mark_job_failed(&job.id).await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;

    cleanup_retained_audio_once(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &Metrics::new(),
    )
    .await
    .expect("cleanup sweep");

    for artifact in &artifacts {
        assert!(
            !tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact released from parent-component root: {}",
            artifact.display()
        );
    }
    assert_eq!(fixture.retained_artifact_count(&job.id).await, 0);
}

#[tokio::test]
async fn cleanup_rejects_lexical_escape_from_accepted_audio_dir() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-lexical-escape-cleanup", b"hello audio")
        .await;
    fixture.mark_job_failed(&job.id).await;
    let escape_path = fixture
        .accepted_audio_dir
        .join("..")
        .join("outside")
        .join("missing.wav");
    fixture
        .set_composed_retained_path(&job.id, &escape_path)
        .await;

    cleanup_retained_audio_once(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &Metrics::new(),
    )
    .await
    .expect("unsafe cleanup is recorded internally");

    assert_eq!(fixture.retained_artifact_count(&job.id).await, 1);
    assert_eq!(fixture.composed_cleanup_attempts(&job.id).await, 1);
}

#[tokio::test]
async fn cleanup_rejects_symlink_escape_from_accepted_audio_dir() {
    use std::os::unix::fs::symlink;

    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-symlink-escape-cleanup", b"hello audio")
        .await;
    fixture.mark_job_failed(&job.id).await;

    let outside_dir = fixture
        .accepted_audio_dir
        .parent()
        .expect("accepted audio dir has parent")
        .join("outside");
    tokio::fs::create_dir(&outside_dir)
        .await
        .expect("create outside dir");
    let outside_artifact = outside_dir.join("escaped.wav");
    tokio::fs::write(&outside_artifact, b"outside audio")
        .await
        .expect("write outside artifact");
    let symlink_path = fixture.accepted_audio_dir.join("outside-link");
    symlink(&outside_dir, &symlink_path).expect("create outside symlink");
    let retained_path = symlink_path.join("escaped.wav");
    fixture
        .set_composed_retained_path(&job.id, &retained_path)
        .await;

    cleanup_retained_audio_once(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &Metrics::new(),
    )
    .await
    .expect("unsafe cleanup is recorded internally");

    assert!(
        tokio::fs::try_exists(&outside_artifact)
            .await
            .expect("outside artifact existence check"),
        "symlink escape target should not be removed"
    );
    assert_eq!(fixture.retained_artifact_count(&job.id).await, 1);
    assert_eq!(fixture.composed_cleanup_attempts(&job.id).await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn overlapping_cleaners_record_success_once_for_one_retained_artifact() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-overlapping-cleanup", b"hello audio")
        .await;
    fixture.mark_job_failed(&job.id).await;
    let metrics = Metrics::new();
    let events = global_captured_events();
    let releaser = BarrierReleaser {
        barrier: Arc::new(Barrier::new(2)),
    };
    let other_releaser = releaser.clone();

    let (first, second) = tokio::join!(
        cleanup_retained_audio_once_with_releaser(&fixture.storage, &metrics, &releaser),
        cleanup_retained_audio_once_with_releaser(&fixture.storage, &metrics, &other_releaser),
    );
    first.expect("first cleanup");
    second.expect("second cleanup");

    let body = operator_metrics_text(&fixture, metrics).await;
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="succeeded""#, r#"artifact="composed_audio""#],
        "1",
    );
    assert_eq!(
        events.count_fields(&[
            ("message", "retention cleanup released artifact"),
            ("job_id", &job.id),
            ("artifact_kind", "composed_audio"),
        ]),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cleanup_failures_retry_without_changing_terminal_state_and_log_attempt_count() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_chunked_queued_job("attempt-cleanup-retry", &[b"first"])
        .await;
    fixture.mark_job_failed(&job.id).await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;
    let metrics = Metrics::new();
    let events = CapturedEvents::default();
    let _guard = tracing::subscriber::set_default(Registry::default().with(events.clone()));

    cleanup_retained_audio_once_with_releaser(&fixture.storage, &metrics, &FailingReleaser)
        .await
        .expect("cleanup failure is recorded internally");

    for artifact in &artifacts {
        assert!(
            tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "failed cleanup should leave artifact for retry: {}",
            artifact.display()
        );
    }
    assert_eq!(fixture.composed_cleanup_attempts(&job.id).await, 1);
    assert_eq!(fixture.chunk_cleanup_attempts(&job.id, 0).await, 1);
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");

    let body = operator_metrics_text(&fixture, metrics).await;
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="failed""#, r#"artifact="chunk""#],
        "1",
    );
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="failed""#, r#"artifact="composed_audio""#],
        "1",
    );
    assert!(events.contains_fields(&[
        ("message", "retention cleanup failed to release artifact"),
        ("job_id", &job.id),
        ("artifact_kind", "chunk"),
        ("attempt_count", "1"),
    ]));
}

#[tokio::test]
async fn worker_success_increments_operator_success_counter() {
    let fixture = WorkerFixture::new().await;
    fixture
        .create_queued_job("attempt-metrics", b"hello audio")
        .await;
    let engine = FakeEngine::success("transcribed text");
    let probe = FakeDurationProbe::success(1_480);
    let metrics = Metrics::new();

    process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &metrics,
    )
    .await
    .expect("process one job");

    let body = operator_metrics_text(&fixture, metrics).await;

    assert_metric_sample_has_value(
        &body,
        "oracy_transcription_worker_jobs_total",
        &[r#"outcome="succeeded""#, r#"failure_class="none""#],
        "1",
    );
}

#[tokio::test]
async fn worker_retry_increments_operator_retry_counter_with_failure_class() {
    let fixture = WorkerFixture::new().await;
    fixture
        .create_queued_job("attempt-metrics-retry", b"hello audio")
        .await;
    let engine = FakeEngine::transient("engine_error", "temporary engine failure", Some(30));
    let probe = FakeDurationProbe::success(1_480);
    let metrics = Metrics::new();

    process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &metrics,
    )
    .await
    .expect("process one job");

    let body = operator_metrics_text(&fixture, metrics).await;

    assert_metric_sample_has_value(
        &body,
        "oracy_transcription_worker_jobs_total",
        &[
            r#"outcome="retry_waiting""#,
            r#"failure_class="engine_error""#,
        ],
        "1",
    );
}

#[tokio::test]
async fn worker_terminal_failure_increments_operator_failure_counter_with_failure_class() {
    let fixture = WorkerFixture::new().await;
    fixture
        .create_queued_job("attempt-metrics-failure", b"not audio")
        .await;
    let engine = FakeEngine::success("unused");
    let probe = FakeDurationProbe::invalid_audio();
    let metrics = Metrics::new();

    process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &metrics,
    )
    .await
    .expect("process one job");

    let body = operator_metrics_text(&fixture, metrics).await;

    assert_metric_sample_has_value(
        &body,
        "oracy_transcription_worker_jobs_total",
        &[r#"outcome="failed""#, r#"failure_class="audio_invalid""#],
        "1",
    );
}

#[tokio::test]
async fn worker_marks_duration_probe_failure_as_audio_invalid() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-invalid-audio", b"not audio")
        .await;
    let engine = FakeEngine::success("unused");
    let probe = FakeDurationProbe::invalid_audio();

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Failed {
            job_id: job.id.clone()
        }
    );
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.failure_code.as_deref(), Some("audio_invalid"));
    assert_eq!(failed.retryable_by_client, Some(false));
    assert_eq!(failed.voice_note_id, None);
}

#[tokio::test]
async fn worker_routes_transient_engine_failure_to_retry_waiting() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-transient-engine", b"hello audio")
        .await;
    let engine = FakeEngine::transient("engine_error", "temporary engine failure", Some(30));
    let probe = FakeDurationProbe::success(1_480);

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::RetryWaiting {
            job_id: job.id.clone()
        }
    );
    let retrying = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(retrying.status, "retry_waiting");
    assert_eq!(retrying.retry_count, 1);
    assert_eq!(retrying.failure_code.as_deref(), Some("engine_error"));
    assert!(retrying.next_attempt_at.is_some());
}

#[tokio::test]
async fn worker_routes_transient_embedding_failure_to_retry_waiting_before_materialization() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-transient-embedding", b"hello audio")
        .await;
    let engine = FakeEngine::success("transcribed text");
    let embedding_engine =
        FakeEmbeddingEngine::transient("engine_rate_limited", "slow down", Some(42));
    let probe = FakeDurationProbe::success(1_480);

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &embedding_engine,
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::RetryWaiting {
            job_id: job.id.clone()
        }
    );
    let retrying = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(retrying.status, "retry_waiting");
    assert_eq!(
        retrying.failure_code.as_deref(),
        Some("engine_rate_limited")
    );
    assert!(retrying.next_attempt_at.is_some());
    assert!(retrying.voice_note_id.is_none());
}

#[tokio::test]
async fn worker_fails_empty_transcription_without_materializing_a_voice_note() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-empty-transcript", b"silent audio")
        .await;
    let engine = FakeEngine::success(" \n\t");
    let embedding_engine = FakeEmbeddingEngine::success(vec![1.0]);
    let probe = FakeDurationProbe::success(1_480);

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &embedding_engine,
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Failed {
            job_id: job.id.clone()
        }
    );
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.failure_code.as_deref(), Some("audio_invalid"));
    assert!(failed.voice_note_id.is_none());
}

#[tokio::test]
async fn worker_releases_retained_audio_when_retry_exhaustion_fails_the_job() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_chunked_queued_job("attempt-retry-exhaustion-cleanup", &[b"hello audio"])
        .await;
    fixture.set_retry_count(&job.id, 2).await;
    let artifacts = fixture.retained_artifact_paths(&job.id).await;
    let engine = FakeEngine::transient("engine_error", "temporary engine failure", Some(30));
    let probe = FakeDurationProbe::success(1_480);

    let outcome = process_one_available_job(
        &fixture.storage,
        &fixture.accepted_audio_dir,
        &engine,
        &FakeEmbeddingEngine::success(vec![1.0]),
        &probe,
        WorkerConfig::test(),
        &Metrics::new(),
    )
    .await
    .expect("process one job");

    assert_eq!(
        outcome,
        ProcessOutcome::Failed {
            job_id: job.id.clone()
        }
    );
    for artifact in &artifacts {
        assert!(
            !tokio::fs::try_exists(artifact)
                .await
                .expect("artifact existence check"),
            "expected retained artifact released after retry exhaustion: {}",
            artifact.display()
        );
    }
    let failed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.retry_count, 3);
    assert_eq!(failed.failure_code.as_deref(), Some("engine_error"));
}

#[tokio::test]
async fn stalled_openai_request_records_transient_timeout_and_worker_processes_next_job() {
    let fixture = WorkerFixture::new().await;
    let stalled_job = fixture
        .create_queued_job("attempt-stalled-openai", b"stalled audio")
        .await;
    let next_job = fixture
        .create_queued_job("attempt-next-openai", b"next audio")
        .await;
    let base_url = spawn_stalling_openai().await;
    let engine = OpenAiTranscriptionEngine::with_request_timeout(
        base_url,
        "test-openai-key".to_owned(),
        PassthroughSlicer,
        Duration::from_millis(50),
    );
    let probe = FakeDurationProbe::success(1_480);

    let first = tokio::time::timeout(
        Duration::from_secs(1),
        process_one_available_job(
            &fixture.storage,
            &fixture.accepted_audio_dir,
            &engine,
            &FakeEmbeddingEngine::success(vec![1.0]),
            &probe,
            WorkerConfig::test(),
            &Metrics::new(),
        ),
    )
    .await
    .expect("stalled request should be bounded")
    .expect("process stalled job");
    assert_eq!(
        first,
        ProcessOutcome::RetryWaiting {
            job_id: stalled_job.id.clone()
        }
    );
    let retrying = fixture
        .storage
        .get_job("owner-a", &stalled_job.id)
        .await
        .expect("stalled job lookup")
        .expect("stalled job exists");
    assert_eq!(retrying.status, "retry_waiting");
    assert_eq!(retrying.failure_code.as_deref(), Some("engine_timeout"));

    let second = tokio::time::timeout(
        Duration::from_secs(1),
        process_one_available_job(
            &fixture.storage,
            &fixture.accepted_audio_dir,
            &engine,
            &FakeEmbeddingEngine::success(vec![1.0]),
            &probe,
            WorkerConfig::test(),
            &Metrics::new(),
        ),
    )
    .await
    .expect("worker should remain available after timeout")
    .expect("process next job");
    assert_eq!(
        second,
        ProcessOutcome::Succeeded {
            job_id: next_job.id.clone()
        }
    );
    let completed = fixture
        .storage
        .get_job("owner-a", &next_job.id)
        .await
        .expect("next job lookup")
        .expect("next job exists");
    assert_eq!(completed.status, "succeeded");
    let voice_note = fixture
        .storage
        .get_voice_note(
            "owner-a",
            completed.voice_note_id.as_deref().expect("voice note id"),
        )
        .await
        .expect("voice note lookup")
        .expect("voice note exists");
    assert_eq!(voice_note.text, "transcribed after timeout");
}

#[tokio::test]
async fn worker_renews_processing_lease_while_transcription_outlives_initial_lease() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-long-transcription", b"long audio")
        .await;
    let storage_for_worker = fixture.storage.clone();
    let engine = SlowEngine::new(Duration::from_millis(140), "long transcription");
    let probe = FakeDurationProbe::success(1_480);
    let config = WorkerConfig {
        lease_duration: TimeDuration::milliseconds(50),
        lease_renewal_interval: Duration::from_millis(10),
        idle_sleep: Duration::from_millis(10),
        error_sleep: Duration::from_millis(10),
    };

    let accepted_audio_dir = fixture.accepted_audio_dir.clone();
    let handle = tokio::spawn(async move {
        process_one_available_job(
            &storage_for_worker,
            &accepted_audio_dir,
            &engine,
            &FakeEmbeddingEngine::success(vec![1.0]),
            &probe,
            config,
            &Metrics::new(),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(75)).await;

    assert!(
        fixture
            .storage
            .claim_next_transcription_job(
                "competing-lease",
                OffsetDateTime::now_utc(),
                OffsetDateTime::now_utc() + TimeDuration::milliseconds(50),
            )
            .await
            .expect("competing claim")
            .is_none()
    );

    let outcome = handle.await.expect("worker task").expect("process job");
    assert_eq!(
        outcome,
        ProcessOutcome::Succeeded {
            job_id: job.id.clone()
        }
    );
    let completed = fixture
        .storage
        .get_job("owner-a", &job.id)
        .await
        .expect("job lookup")
        .expect("job exists");
    assert_eq!(completed.status, "succeeded");
}

struct WorkerFixture {
    _tempdir: TempDir,
    accepted_audio_dir: PathBuf,
    storage: Storage,
}

impl WorkerFixture {
    async fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let database_path = tempdir.path().join("oracy.sqlite");
        let accepted_audio_dir = tempdir.path().join("accepted-audio");
        tokio::fs::create_dir(&accepted_audio_dir)
            .await
            .expect("create accepted audio dir");
        let storage = Storage::connect(&database_path)
            .await
            .expect("connect storage");
        Self {
            _tempdir: tempdir,
            accepted_audio_dir,
            storage,
        }
    }

    async fn new_with_parent_component_accepted_audio_dir() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let database_path = tempdir.path().join("oracy.sqlite");
        let config_dir = tempdir.path().join("config");
        tokio::fs::create_dir(&config_dir)
            .await
            .expect("create config dir");
        let canonical_accepted_audio_dir = tempdir.path().join("accepted-audio");
        tokio::fs::create_dir(&canonical_accepted_audio_dir)
            .await
            .expect("create accepted audio dir");
        let accepted_audio_dir = config_dir.join("../accepted-audio");
        let storage = Storage::connect(&database_path)
            .await
            .expect("connect storage");
        Self {
            _tempdir: tempdir,
            accepted_audio_dir,
            storage,
        }
    }

    async fn create_queued_job(
        &self,
        idempotency_key: &str,
        audio: &[u8],
    ) -> oracy_backend::storage::TranscriptionJobRecord {
        let path = self
            .accepted_audio_dir
            .join(format!("{idempotency_key}.wav"));
        tokio::fs::write(&path, audio)
            .await
            .expect("write accepted audio");
        match self
            .storage
            .accept_job(NewTranscriptionJob {
                api_key_id: "owner-a".to_owned(),
                idempotency_key: idempotency_key.to_owned(),
                audio_sha256_hex: "hash-a".to_owned(),
                recorded_at: datetime!(2026-04-24 17:59:00 UTC),
                session_id: None,
                language: Some("en".to_owned()),
                accepted_audio_path: path,
                max_retries: 3,
                now: datetime!(2026-04-24 18:00:00 UTC),
            })
            .await
            .expect("accept job")
        {
            AcceptJobOutcome::Created(job) => job,
            other => panic!("expected created job, got {other:?}"),
        }
    }

    async fn create_chunked_queued_job(
        &self,
        idempotency_key: &str,
        chunks: &[&[u8]],
    ) -> oracy_backend::storage::TranscriptionJobRecord {
        let opened = match self
            .storage
            .open_job(NewOpenTranscriptionJob {
                api_key_id: "owner-a".to_owned(),
                idempotency_key: idempotency_key.to_owned(),
                recorded_at: datetime!(2026-04-24 17:59:00 UTC),
                session_id: None,
                language: Some("en".to_owned()),
                chunk_count: chunks.len() as i64,
                audio_format: "wav".to_owned(),
                max_retries: 3,
                now: datetime!(2026-04-24 18:00:00 UTC),
            })
            .await
            .expect("open job")
        {
            OpenJobOutcome::Created(job) => job,
            other => panic!("expected opened job, got {other:?}"),
        };

        for (index, bytes) in chunks.iter().enumerate() {
            let chunk_path = self
                .accepted_audio_dir
                .join(&opened.id)
                .join("chunks")
                .join(format!("{index}.chunk"));
            tokio::fs::create_dir_all(chunk_path.parent().expect("chunk parent"))
                .await
                .expect("create chunk directory");
            tokio::fs::write(&chunk_path, bytes)
                .await
                .expect("write chunk");
            let outcome = self
                .storage
                .store_chunk(AcceptedChunk {
                    api_key_id: "owner-a".to_owned(),
                    job_id: opened.id.clone(),
                    chunk_index: index as i64,
                    chunk_sha256_hex: sha256_hex(bytes),
                    chunk_path,
                    chunk_size_bytes: bytes.len() as i64,
                    accepted_at: datetime!(2026-04-24 18:00:01 UTC),
                })
                .await
                .expect("store chunk");
            assert_eq!(outcome, StoreChunkOutcome::Stored);
        }

        let composed_path = self
            .accepted_audio_dir
            .join(&opened.id)
            .join("accepted.wav");
        let mut composed = Vec::new();
        for bytes in chunks {
            composed.extend_from_slice(bytes);
        }
        tokio::fs::write(&composed_path, composed)
            .await
            .expect("write composed audio");

        match self
            .storage
            .finalize_job(
                "owner-a",
                &opened.id,
                "composed-hash",
                &composed_path,
                "gpt-4o-mini-transcribe",
                datetime!(2026-04-24 18:00:02 UTC),
            )
            .await
            .expect("finalize job")
        {
            FinalizeJobOutcome::Accepted(job) => job,
            other => panic!("expected finalized job, got {other:?}"),
        }
    }

    async fn retained_artifact_paths(&self, job_id: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let accepted_audio_path: String = sqlx::query_scalar(
            r#"
            SELECT accepted_audio_path
            FROM transcription_jobs
            WHERE api_key_id = 'owner-a' AND id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("accepted audio path");
        paths.push(PathBuf::from(accepted_audio_path));

        let rows = sqlx::query(
            r#"
            SELECT chunk_path
            FROM transcription_job_chunks
            WHERE api_key_id = 'owner-a' AND job_id = ?
            ORDER BY chunk_index ASC
            "#,
        )
        .bind(job_id)
        .fetch_all(self.storage.pool())
        .await
        .expect("chunk paths");
        paths.extend(
            rows.into_iter()
                .map(|row| PathBuf::from(row.get::<String, _>("chunk_path"))),
        );
        paths
    }

    async fn retained_artifact_count(&self, job_id: &str) -> i64 {
        let composed_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_jobs
            WHERE api_key_id = 'owner-a' AND id = ? AND accepted_audio_path <> ''
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("composed retained artifact count");
        let chunk_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_job_chunks
            WHERE api_key_id = 'owner-a' AND job_id = ? AND chunk_path <> ''
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("chunk retained artifact count");
        composed_count + chunk_count
    }

    async fn set_composed_retained_path(&self, job_id: &str, path: &std::path::Path) {
        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET accepted_audio_path = ?
            WHERE api_key_id = 'owner-a' AND id = ?
            "#,
        )
        .bind(path.to_string_lossy().into_owned())
        .bind(job_id)
        .execute(self.storage.pool())
        .await
        .expect("set composed retained path");
        assert_eq!(result.rows_affected(), 1);
    }

    async fn chunk_row_count(&self, job_id: &str) -> i64 {
        sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_job_chunks
            WHERE api_key_id = 'owner-a' AND job_id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("chunk row count")
    }

    async fn mark_job_failed(&self, job_id: &str) {
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
        .bind(job_id)
        .execute(self.storage.pool())
        .await
        .expect("mark job failed");
        assert_eq!(result.rows_affected(), 1);
    }

    async fn set_retry_count(&self, job_id: &str, retry_count: i64) {
        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET retry_count = ?
            WHERE api_key_id = 'owner-a' AND id = ?
            "#,
        )
        .bind(retry_count)
        .bind(job_id)
        .execute(self.storage.pool())
        .await
        .expect("set retry count");
        assert_eq!(result.rows_affected(), 1);
    }

    async fn composed_cleanup_attempts(&self, job_id: &str) -> i64 {
        sqlx::query_scalar(
            r#"
            SELECT accepted_audio_cleanup_attempts
            FROM transcription_jobs
            WHERE api_key_id = 'owner-a' AND id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("composed cleanup attempts")
    }

    async fn chunk_cleanup_attempts(&self, job_id: &str, chunk_index: i64) -> i64 {
        sqlx::query_scalar(
            r#"
            SELECT cleanup_attempts
            FROM transcription_job_chunks
            WHERE api_key_id = 'owner-a' AND job_id = ? AND chunk_index = ?
            "#,
        )
        .bind(job_id)
        .bind(chunk_index)
        .fetch_one(self.storage.pool())
        .await
        .expect("chunk cleanup attempts")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct FailingReleaser;

impl RetainedAudioReleaser for FailingReleaser {
    async fn release(
        &self,
        artifact: &oracy_backend::storage::RetainedAudioArtifact,
    ) -> Result<(), RetentionCleanupError> {
        Err(RetentionCleanupError::Remove {
            path: artifact.path.clone(),
            source: std::io::Error::other("simulated cleanup failure"),
        })
    }
}

#[derive(Clone)]
struct BarrierReleaser {
    barrier: Arc<Barrier>,
}

impl RetainedAudioReleaser for BarrierReleaser {
    async fn release(
        &self,
        _artifact: &oracy_backend::storage::RetainedAudioArtifact,
    ) -> Result<(), RetentionCleanupError> {
        self.barrier.wait().await;
        Ok(())
    }
}

#[derive(Clone, Default)]
struct CapturedEvents {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

static GLOBAL_CAPTURED_EVENTS: OnceLock<CapturedEvents> = OnceLock::new();

fn global_captured_events() -> CapturedEvents {
    GLOBAL_CAPTURED_EVENTS
        .get_or_init(|| {
            let events = CapturedEvents::default();
            tracing::subscriber::set_global_default(Registry::default().with(events.clone()))
                .expect("global tracing subscriber");
            events
        })
        .clone()
}

impl CapturedEvents {
    fn contains_fields(&self, expected: &[(&str, &str)]) -> bool {
        self.events.lock().expect("events").iter().any(|event| {
            expected.iter().all(|(field, value)| {
                event
                    .fields
                    .iter()
                    .any(|(name, actual)| name == field && actual.contains(value))
            })
        })
    }

    fn count_fields(&self, expected: &[(&str, &str)]) -> usize {
        self.events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| {
                expected.iter().all(|(field, value)| {
                    event
                        .fields
                        .iter()
                        .any(|(name, actual)| name == field && actual.contains(value))
                })
            })
            .count()
    }
}

#[derive(Debug, Clone)]
struct CapturedEvent {
    fields: Vec<(String, String)>,
}

impl<S> Layer<S> for CapturedEvents
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        let mut visitor = EventVisitor { fields: Vec::new() };
        event.record(&mut visitor);
        self.events.lock().expect("events").push(CapturedEvent {
            fields: visitor.fields,
        });
    }
}

struct EventVisitor {
    fields: Vec<(String, String)>,
}

impl Visit for EventVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .push((field.name().to_owned(), format!("{value:?}")));
    }
}

async fn operator_metrics_text(fixture: &WorkerFixture, metrics: Metrics) -> String {
    let auth_store = AuthStore::try_from_configs(&[ApiKeyConfig {
        api_key_id: "owner-a".to_owned(),
        key: "owner-secret".to_owned(),
    }])
    .expect("auth config");
    let state = AppState {
        accepted_audio_dir: fixture.accepted_audio_dir.clone(),
        auth_store: Arc::new(auth_store),
        metrics,
        operator_listen_addr: "127.0.0.1:9090".parse().expect("operator listen addr"),
        openai_api_key: "test-openai-key".to_owned(),
        openai_base_url: "http://127.0.0.1".to_owned(),
        storage: fixture.storage.clone(),
    };
    let response = build_operator_router(state)
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
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

#[derive(Clone)]
struct FakeEngine {
    output: Result<TranscriptionOutput, EngineFailure>,
    inputs: Arc<Mutex<Vec<TranscriptionInput>>>,
}

impl FakeEngine {
    fn success(text: &str) -> Self {
        Self {
            output: Ok(TranscriptionOutput {
                text: text.to_owned(),
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn transient(failure_code: &str, message: &str, retry_after_seconds: Option<i64>) -> Self {
        Self {
            output: Err(EngineFailure::Transient {
                failure_code: failure_code.to_owned(),
                message: message.to_owned(),
                retry_after_seconds,
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl TranscriptionEngine for FakeEngine {
    async fn transcribe(
        &self,
        input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, EngineFailure> {
        self.inputs.lock().expect("inputs").push(input);
        self.output.clone()
    }
}

#[derive(Clone)]
struct FakeEmbeddingEngine {
    output: Result<EmbeddingOutput, EmbeddingFailure>,
    inputs: Arc<Mutex<Vec<EmbeddingInput>>>,
}

impl FakeEmbeddingEngine {
    fn success(vector: Vec<f32>) -> Self {
        Self {
            output: Ok(EmbeddingOutput {
                model: "text-embedding-3-small".to_owned(),
                vector,
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn transient(failure_code: &str, message: &str, retry_after_seconds: Option<i64>) -> Self {
        Self {
            output: Err(EmbeddingFailure::Transient {
                failure_code: failure_code.to_owned(),
                message: message.to_owned(),
                retry_after_seconds,
            }),
            inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl EmbeddingEngine for FakeEmbeddingEngine {
    async fn embed(&self, input: EmbeddingInput) -> Result<EmbeddingOutput, EmbeddingFailure> {
        self.inputs.lock().expect("inputs").push(input);
        self.output.clone()
    }
}

#[derive(Clone)]
struct SlowEngine {
    delay: Duration,
    text: String,
}

impl SlowEngine {
    fn new(delay: Duration, text: &str) -> Self {
        Self {
            delay,
            text: text.to_owned(),
        }
    }
}

impl TranscriptionEngine for SlowEngine {
    async fn transcribe(
        &self,
        _input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, EngineFailure> {
        tokio::time::sleep(self.delay).await;
        Ok(TranscriptionOutput {
            text: self.text.clone(),
        })
    }
}

#[derive(Clone)]
struct FakeDurationProbe {
    result: Result<i64, DurationProbeError>,
}

impl FakeDurationProbe {
    fn success(duration_ms: i64) -> Self {
        Self {
            result: Ok(duration_ms),
        }
    }

    fn invalid_audio() -> Self {
        Self {
            result: Err(DurationProbeError::InvalidAudio),
        }
    }
}

impl DurationProbe for FakeDurationProbe {
    async fn duration_ms(
        &self,
        _input: &std::path::Path,
        _audio_format: &str,
    ) -> Result<i64, DurationProbeError> {
        self.result.clone()
    }
}

#[derive(Clone)]
struct PassthroughSlicer;

impl AudioSlicer for PassthroughSlicer {
    async fn slices(
        &self,
        input: &std::path::Path,
        _audio_format: &str,
    ) -> Result<Vec<PathBuf>, AudioSliceError> {
        Ok(vec![input.to_path_buf()])
    }
}

async fn spawn_stalling_openai() -> String {
    let app = Router::new().route("/v1/audio/transcriptions", post(stalling_transcription));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake openai");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve fake openai");
    });
    format!("http://{addr}")
}

async fn stalling_transcription(mut multipart: Multipart) -> impl IntoResponse {
    let mut file_bytes = Vec::new();
    while let Some(field) = multipart.next_field().await.expect("multipart field") {
        if field.name() == Some("file") {
            file_bytes = field.bytes().await.expect("file bytes").to_vec();
        }
    }

    if file_bytes == b"stalled audio" {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }

    Json(json!({ "text": "transcribed after timeout" }))
}
