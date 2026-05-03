use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::Multipart;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use oracy_backend::storage::{AcceptJobOutcome, NewTranscriptionJob, Storage};
use oracy_backend::transcription_worker::{
    AudioSliceError, AudioSlicer, DurationProbe, DurationProbeError, EngineFailure,
    OpenAiTranscriptionEngine, ProcessOutcome, TranscriptionEngine, TranscriptionInput,
    TranscriptionOutput, WorkerConfig, process_one_available_job,
};
use serde_json::json;
use tempfile::TempDir;
use time::Duration as TimeDuration;
use time::OffsetDateTime;
use time::macros::datetime;

#[tokio::test]
async fn worker_materializes_a_queued_job_with_a_fake_engine() {
    let fixture = WorkerFixture::new().await;
    let job = fixture.create_queued_job("attempt-1", b"hello audio").await;
    let engine = FakeEngine::success("transcribed text");
    let probe = FakeDurationProbe::success(1_480);

    let outcome =
        process_one_available_job(&fixture.storage, &engine, &probe, WorkerConfig::test())
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
    assert!(
        fixture
            .storage
            .get_current_embedding("owner-a", &voice_note.id)
            .await
            .expect("embedding lookup")
            .is_none()
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
async fn worker_marks_duration_probe_failure_as_audio_invalid() {
    let fixture = WorkerFixture::new().await;
    let job = fixture
        .create_queued_job("attempt-invalid-audio", b"not audio")
        .await;
    let engine = FakeEngine::success("unused");
    let probe = FakeDurationProbe::invalid_audio();

    let outcome =
        process_one_available_job(&fixture.storage, &engine, &probe, WorkerConfig::test())
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

    let outcome =
        process_one_available_job(&fixture.storage, &engine, &probe, WorkerConfig::test())
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
        process_one_available_job(&fixture.storage, &engine, &probe, WorkerConfig::test()),
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
        process_one_available_job(&fixture.storage, &engine, &probe, WorkerConfig::test()),
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

    let handle = tokio::spawn(async move {
        process_one_available_job(&storage_for_worker, &engine, &probe, config).await
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
