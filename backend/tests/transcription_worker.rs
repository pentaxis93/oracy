use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use oracy_backend::storage::{AcceptJobOutcome, NewTranscriptionJob, Storage};
use oracy_backend::transcription_worker::{
    DurationProbe, DurationProbeError, EngineFailure, ProcessOutcome, TranscriptionEngine,
    TranscriptionInput, TranscriptionOutput, WorkerConfig, process_one_available_job,
};
use tempfile::TempDir;
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
