use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::router::build_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::Storage;
use oracy_backend::transcription_worker::{
    TranscriptionEngine, TranscriptionEngineError, TranscriptionInput, TranscriptionOutput,
    process_one_queued_job,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use time::macros::datetime;
use tower::util::ServiceExt;

#[tokio::test]
async fn chunked_submission_finalizes_durably_and_exposes_job_progress() {
    let fixture = TranscriptionJobFixture::new().await;

    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some(("Idempotency-Key", "attempt-one")),
            json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "language": "en"
            }),
        )
        .await;

    assert_eq!(opened.status, StatusCode::CREATED);
    assert_eq!(opened.body["status"], "accepting_chunks");
    assert_eq!(opened.body["chunk_count"], 1);
    assert_eq!(opened.body["chunks_received"], 0);
    assert_eq!(opened.body["retry_count"], 0);
    assert_eq!(opened.body["voice_note_id"], Value::Null);
    let job_id = opened.body["id"].as_str().expect("job id").to_owned();

    let chunk = b"not actually wav yet";
    let response = fixture
        .multipart_chunk(&job_id, 0, &sha256_hex(chunk), chunk)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let progressed = fixture.get_job(&job_id).await;
    assert_eq!(progressed.status, StatusCode::OK);
    assert_eq!(progressed.body["status"], "accepting_chunks");
    assert_eq!(progressed.body["chunks_received"], 1);

    let finalized = fixture
        .empty_request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
        )
        .await;
    assert_eq!(finalized.status, StatusCode::ACCEPTED);
    assert_eq!(finalized.body["status"], "queued");
    assert_eq!(finalized.body["chunks_received"], 1);
    assert_eq!(finalized.body["voice_note_id"], Value::Null);

    let fetched = fixture.get_job(&job_id).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body, finalized.body);
}

#[tokio::test]
async fn queued_job_processing_materializes_voice_note_and_single_coarse_segment() {
    let fixture = TranscriptionJobFixture::new().await;
    let job_id = fixture.open_single_chunk_wav("attempt-process").await;
    let chunk = one_tenth_second_wav();
    let response = fixture
        .multipart_chunk(&job_id, 0, &sha256_hex(&chunk), &chunk)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fixture
            .empty_request(
                "POST",
                &format!("/api/v1/transcription-jobs/{job_id}/finalize")
            )
            .await
            .status,
        StatusCode::ACCEPTED
    );

    let processed = process_one_queued_job(
        &fixture.app_state.storage,
        &FakeTranscriptionEngine,
        datetime!(2026-04-24 18:00:30 UTC),
    )
    .await
    .expect("process queued job");
    assert!(processed);

    let job = fixture.get_job(&job_id).await;
    assert_eq!(job.status, StatusCode::OK);
    assert_eq!(job.body["status"], "succeeded");
    let voice_note_id = job.body["voice_note_id"]
        .as_str()
        .expect("voice note id")
        .to_owned();

    let voice_note = fixture
        .empty_request("GET", &format!("/api/v1/voice-notes/{voice_note_id}"))
        .await;
    assert_eq!(voice_note.status, StatusCode::OK);
    assert_eq!(voice_note.body["text"], "hello from fake engine");
    assert_eq!(voice_note.body["audio_duration_seconds"], 0.1);
    assert_eq!(voice_note.body["model"], "gpt-4o-mini-transcribe");

    let segments = fixture
        .empty_request(
            "GET",
            &format!("/api/v1/voice-notes/{voice_note_id}/segments"),
        )
        .await;
    assert_eq!(segments.status, StatusCode::OK);
    assert_eq!(
        segments.body["items"].as_array().expect("segments").len(),
        1
    );
    assert_eq!(segments.body["items"][0]["position"], 0);
    assert_eq!(segments.body["items"][0]["start_ms"], 0);
    assert_eq!(segments.body["items"][0]["end_ms"], 100);
    assert_eq!(segments.body["items"][0]["text"], "hello from fake engine");
}

#[tokio::test]
async fn transient_engine_failure_moves_job_to_retry_waiting() {
    let fixture = TranscriptionJobFixture::new().await;
    let job_id = fixture.open_single_chunk_wav("attempt-retry").await;
    let chunk = one_tenth_second_wav();
    let response = fixture
        .multipart_chunk(&job_id, 0, &sha256_hex(&chunk), &chunk)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fixture
            .empty_request(
                "POST",
                &format!("/api/v1/transcription-jobs/{job_id}/finalize")
            )
            .await
            .status,
        StatusCode::ACCEPTED
    );

    let processed = process_one_queued_job(
        &fixture.app_state.storage,
        &TransientFailureEngine,
        datetime!(2026-04-24 18:00:30 UTC),
    )
    .await
    .expect("record retryable failure");
    assert!(processed);

    let job = fixture.get_job(&job_id).await;
    assert_eq!(job.status, StatusCode::OK);
    assert_eq!(job.body["status"], "retry_waiting");
    assert_eq!(job.body["retry_count"], 1);
    assert_eq!(job.body["next_attempt_at"], "2026-04-24T18:01:00Z");
    assert_eq!(job.body["failure_code"], Value::Null);
    assert_eq!(job.body["voice_note_id"], Value::Null);
}

#[tokio::test]
async fn open_replay_matches_original_body_and_conflicts_on_mismatch() {
    let fixture = TranscriptionJobFixture::new().await;
    let first = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some(("Idempotency-Key", "attempt-replay")),
            json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "language": "en"
            }),
        )
        .await;
    assert_eq!(first.status, StatusCode::CREATED);

    let replay = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some(("Idempotency-Key", "attempt-replay")),
            json!({
                "recorded_at": "2026-04-24T17:59:00.000Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "language": "en"
            }),
        )
        .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(replay.body["id"], first.body["id"]);

    let conflict = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some(("Idempotency-Key", "attempt-replay")),
            json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 2,
                "audio_format": "wav",
                "language": "en"
            }),
        )
        .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(conflict.body["error_code"], "conflict");
}

#[tokio::test]
async fn malformed_job_ids_return_validation_before_not_found() {
    let fixture = TranscriptionJobFixture::new().await;

    for uri in [
        "/api/v1/transcription-jobs/not-a-ulid",
        "/api/v1/transcription-jobs/not-a-ulid/finalize",
    ] {
        let response = fixture.empty_request("GET", uri).await;
        let response = if response.status == StatusCode::METHOD_NOT_ALLOWED {
            fixture.empty_request("POST", uri).await
        } else {
            response
        };
        assert_eq!(response.status, StatusCode::BAD_REQUEST);
        assert_eq!(response.body["error_code"], "validation_error");
    }
}

struct JsonResponse {
    status: StatusCode,
    body: Value,
}

struct TranscriptionJobFixture {
    _tempdir: TempDir,
    app_state: AppState,
}

impl TranscriptionJobFixture {
    async fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let accepted_audio_dir = tempdir.path().join("accepted-audio");
        std::fs::create_dir(&accepted_audio_dir).expect("accepted audio dir");
        let database_path = tempdir.path().join("oracy.sqlite");
        let storage = Storage::connect(&database_path).await.expect("storage");
        let auth_store = AuthStore::try_from_configs(&[
            ApiKeyConfig {
                api_key_id: "alpha".to_owned(),
                key: "alpha-secret".to_owned(),
            },
            ApiKeyConfig {
                api_key_id: "beta".to_owned(),
                key: "beta-secret".to_owned(),
            },
        ])
        .expect("auth store");

        let app_state = AppState {
            accepted_audio_dir,
            auth_store: Arc::new(auth_store),
            storage,
        };

        Self {
            _tempdir: tempdir,
            app_state,
        }
    }

    fn app(&self) -> axum::Router {
        build_router(self.app_state.clone())
    }

    async fn json_request(
        &self,
        method: &str,
        uri: &str,
        extra_header: Option<(&str, &str)>,
        body: Value,
    ) -> JsonResponse {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Authorization", "Bearer alpha-secret")
            .header("Content-Type", "application/json");
        if let Some((name, value)) = extra_header {
            builder = builder.header(name, value);
        }
        let response = self
            .app()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .expect("response");
        json_response(response).await
    }

    async fn empty_request(&self, method: &str, uri: &str) -> JsonResponse {
        let response = self
            .app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("Authorization", "Bearer alpha-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        json_response(response).await
    }

    async fn get_job(&self, job_id: &str) -> JsonResponse {
        self.empty_request("GET", &format!("/api/v1/transcription-jobs/{job_id}"))
            .await
    }

    async fn open_single_chunk_wav(&self, idempotency_key: &str) -> String {
        let opened = self
            .json_request(
                "POST",
                "/api/v1/transcription-jobs",
                Some(("Idempotency-Key", idempotency_key)),
                json!({
                    "recorded_at": "2026-04-24T17:59:00Z",
                    "chunk_count": 1,
                    "audio_format": "wav",
                    "language": "en"
                }),
            )
            .await;
        assert_eq!(opened.status, StatusCode::CREATED);
        opened.body["id"].as_str().expect("job id").to_owned()
    }

    async fn multipart_chunk(
        &self,
        job_id: &str,
        chunk_index: i64,
        chunk_sha256: &str,
        chunk: &[u8],
    ) -> axum::response::Response {
        let boundary = "oracy-test-boundary";
        let mut body = Vec::new();
        push_part(
            &mut body,
            boundary,
            "chunk_index",
            chunk_index.to_string().as_bytes(),
        );
        push_part(&mut body, boundary, "chunk_sha256", chunk_sha256.as_bytes());
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"chunk.wav\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(chunk);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        self.app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/transcription-jobs/{job_id}/chunks"))
                    .header("Authorization", "Bearer alpha-secret")
                    .header(
                        "Content-Type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .expect("response")
    }
}

async fn json_response(response: axum::response::Response) -> JsonResponse {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json")
    };
    JsonResponse { status, body }
}

fn push_part(body: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

struct FakeTranscriptionEngine;

impl TranscriptionEngine for FakeTranscriptionEngine {
    fn transcribe(
        &self,
        input: TranscriptionInput<'_>,
    ) -> Result<TranscriptionOutput, TranscriptionEngineError> {
        assert_eq!(input.model, "gpt-4o-mini-transcribe");
        assert_eq!(input.language, Some("en"));
        assert!(input.audio_path.ends_with("accepted.wav"));
        Ok(TranscriptionOutput {
            text: "hello from fake engine".to_owned(),
            model: input.model.to_owned(),
            processing_time_ms: 42,
            cost_cents: None,
        })
    }
}

struct TransientFailureEngine;

impl TranscriptionEngine for TransientFailureEngine {
    fn transcribe(
        &self,
        _input: TranscriptionInput<'_>,
    ) -> Result<TranscriptionOutput, TranscriptionEngineError> {
        Err(TranscriptionEngineError::Transient)
    }
}

fn one_tenth_second_wav() -> Vec<u8> {
    let sample_rate = 8_000_u32;
    let samples = 800_u32;
    let data_bytes = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    bytes.resize(bytes.len() + data_bytes as usize, 0);
    bytes
}
