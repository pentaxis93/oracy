use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use oracy_backend::audio_store::MAX_CHUNK_BYTES;
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::router::build_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::Storage;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::util::ServiceExt;

#[tokio::test]
async fn chunked_submission_reaches_queued_after_all_chunks_are_finalized() {
    let fixture = TranscriptionJobFixture::new().await;

    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-happy-path"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 2,
                "audio_format": "wav",
                "language": "en"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    assert_eq!(opened.body["status"], "accepting_chunks");
    assert_eq!(opened.body["chunk_count"], 2);
    assert_eq!(opened.body["chunks_received"], 0);
    let job_id = opened.body["id"].as_str().expect("job id");

    fixture.push_chunk(job_id, 0, b"first audio bytes").await;
    fixture.push_chunk(job_id, 1, b"second audio bytes").await;

    let finalized = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finalized.status(), StatusCode::ACCEPTED);
    let finalized_body = json_body(finalized).await;
    assert_eq!(finalized_body["status"], "queued");
    assert_eq!(finalized_body["chunks_received"], 2);
    assert_eq!(finalized_body["voice_note_id"], Value::Null);

    let fetched = fixture
        .get_json(&format!("/api/v1/transcription-jobs/{job_id}"))
        .await;
    assert_eq!(fetched["status"], "queued");
    assert_eq!(fetched["chunk_count"], 2);
    assert_eq!(fetched["chunks_received"], 2);
    assert_eq!(fetched["retry_count"], 0);
}

#[tokio::test]
async fn spec_ceiling_chunk_is_accepted_through_the_multipart_route_limit() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-spec-ceiling-chunk"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");
    let bytes = vec![0xA5; MAX_CHUNK_BYTES];

    let response = fixture.push_chunk_response(job_id, 0, &bytes).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let chunk_path = fixture.accepted_chunk_path(job_id, 0).await;
    assert_eq!(
        std::fs::metadata(&chunk_path)
            .expect("accepted chunk metadata")
            .len(),
        MAX_CHUNK_BYTES as u64
    );
}

#[tokio::test]
async fn finalize_composes_large_multi_chunk_submission_in_chunk_order() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-large-finalize"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 3,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");
    let chunks = [
        vec![0x11; 10 * 1024 * 1024],
        vec![0x22; 10 * 1024 * 1024],
        vec![0x33; 10 * 1024 * 1024],
    ];
    for (index, bytes) in chunks.iter().enumerate() {
        fixture.push_chunk(job_id, index, bytes).await;
    }

    let finalized = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;

    assert_eq!(finalized.status(), StatusCode::ACCEPTED);
    let accepted_audio_path = fixture.accepted_audio_path(job_id).await;
    assert_eq!(
        std::fs::metadata(&accepted_audio_path)
            .expect("accepted audio metadata")
            .len(),
        chunks.iter().map(|chunk| chunk.len() as u64).sum::<u64>()
    );
    assert_eq!(
        fixture.file_prefix(&accepted_audio_path, 4).await,
        vec![0x11; 4]
    );
    assert_eq!(
        fixture.file_suffix(&accepted_audio_path, 4).await,
        vec![0x33; 4]
    );
    assert_eq!(
        fixture.stored_audio_hash(job_id).await,
        composed_audio_hash_hex(&chunks)
    );
}

#[tokio::test]
async fn session_scoped_submission_finalizes_without_mutating_replay_tuple() {
    let fixture = TranscriptionJobFixture::new().await;
    let created_session = fixture
        .json_request(
            "POST",
            "/api/v1/sessions",
            None,
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(created_session.status, StatusCode::CREATED);
    let session_id = created_session.body["id"].as_str().expect("session id");

    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-session-finalize"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "session_id": session_id
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");

    fixture.push_chunk(job_id, 0, b"audio").await;
    let finalized = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finalized.status(), StatusCode::ACCEPTED);
    assert_eq!(json_body(finalized).await["status"], "queued");

    let stored_session_id: Option<String> = sqlx::query_scalar(
        "SELECT session_id FROM transcription_jobs WHERE api_key_id = 'alpha' AND id = ?",
    )
    .bind(job_id)
    .fetch_one(fixture.storage.pool())
    .await
    .expect("job row");
    assert_eq!(stored_session_id.as_deref(), Some(session_id));
}

#[tokio::test]
async fn open_replays_matching_idempotency_keys_and_rejects_mismatched_bodies() {
    let fixture = TranscriptionJobFixture::new().await;
    let body = json!({
        "recorded_at": "2026-04-24T17:59:00Z",
        "chunk_count": 1,
        "audio_format": "wav"
    });

    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-replay"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id").to_owned();

    let replayed = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-replay"),
            Some(body),
        )
        .await;
    assert_eq!(replayed.status, StatusCode::CREATED);
    assert_eq!(replayed.body["id"], job_id);

    let conflict = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-replay"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 2,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);

    let missing_key = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            None,
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(missing_key.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn concurrent_open_replays_return_the_same_job() {
    let fixture = TranscriptionJobFixture::new().await;
    let body = json!({
        "recorded_at": "2026-04-24T17:59:00Z",
        "chunk_count": 1,
        "audio_format": "wav"
    });

    let (first, second) = tokio::join!(
        fixture.json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-concurrent-open"),
            Some(body.clone()),
        ),
        fixture.json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-concurrent-open"),
            Some(body),
        ),
    );

    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(second.status, StatusCode::CREATED);
    assert_eq!(first.body["id"], second.body["id"]);
    assert_eq!(
        fixture
            .job_count_by_idempotency_key("attempt-concurrent-open")
            .await,
        1
    );
}

#[tokio::test]
async fn malformed_job_ids_are_rejected_before_existence_checks() {
    let fixture = TranscriptionJobFixture::new().await;

    let get = fixture
        .request(
            "GET",
            "/api/v1/transcription-jobs/not-a-ulid",
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(get.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(get).await["details"][0]["field"], "job_id");

    let finalize = fixture
        .request(
            "POST",
            "/api/v1/transcription-jobs/not-a-ulid/finalize",
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finalize.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(finalize).await["details"][0]["field"], "job_id");

    let response = fixture
        .push_chunk_response("not-a-ulid", 0, b"audio bytes")
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["details"][0]["field"], "job_id");
}

#[tokio::test]
async fn chunk_replay_is_noop_and_conflicting_chunk_preserves_accepted_file() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-chunk-conflict"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");

    fixture.push_chunk(job_id, 0, b"accepted bytes").await;
    let chunk_path = fixture.accepted_chunk_path(job_id, 0).await;
    assert_eq!(
        std::fs::read(&chunk_path).expect("accepted chunk"),
        b"accepted bytes"
    );

    let replay = fixture
        .push_chunk_response(job_id, 0, b"accepted bytes")
        .await;
    assert_eq!(replay.status(), StatusCode::NO_CONTENT);

    let conflict = fixture
        .push_chunk_response(job_id, 0, b"different bytes")
        .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read(&chunk_path).expect("accepted chunk unchanged"),
        b"accepted bytes"
    );
}

#[tokio::test]
async fn concurrent_same_hash_chunk_pushes_are_both_idempotent_successes() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-concurrent-same-chunk"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");

    let (first, second) = tokio::join!(
        fixture.push_chunk_response(job_id, 0, b"accepted bytes"),
        fixture.push_chunk_response(job_id, 0, b"accepted bytes"),
    );

    assert_eq!(first.status(), StatusCode::NO_CONTENT);
    assert_eq!(second.status(), StatusCode::NO_CONTENT);
    assert_eq!(fixture.chunk_count(job_id).await, 1);
}

#[tokio::test]
async fn concurrent_different_hash_chunk_pushes_return_success_and_conflict() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-concurrent-conflicting-chunk"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");

    let (first, second) = tokio::join!(
        fixture.push_chunk_response(job_id, 0, b"accepted bytes"),
        fixture.push_chunk_response(job_id, 0, b"different bytes"),
    );
    let mut statuses = [first.status(), second.status()];
    statuses.sort_by_key(|status| status.as_u16());

    assert_eq!(statuses, [StatusCode::NO_CONTENT, StatusCode::CONFLICT]);
    assert_eq!(fixture.chunk_count(job_id).await, 1);
    let accepted_path = fixture.accepted_chunk_path(job_id, 0).await;
    let accepted_bytes = std::fs::read(&accepted_path).expect("accepted chunk");
    assert!(accepted_bytes == b"accepted bytes" || accepted_bytes == b"different bytes");
}

#[tokio::test]
async fn finalize_requires_all_chunks_and_replays_after_acceptance() {
    let fixture = TranscriptionJobFixture::new().await;
    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-finalize-replay"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 2,
                "audio_format": "wav"
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");
    fixture.push_chunk(job_id, 0, b"first").await;

    let missing = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(missing.status(), StatusCode::CONFLICT);

    fixture.push_chunk(job_id, 1, b"second").await;
    let accepted = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);

    let replay = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["status"], "queued");
}

#[tokio::test]
async fn finalize_captures_current_settings_and_nulls_sessions_deleted_after_open() {
    let fixture = TranscriptionJobFixture::new().await;
    let created_session = fixture
        .json_request(
            "POST",
            "/api/v1/sessions",
            None,
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(created_session.status, StatusCode::CREATED);
    let session_id = created_session.body["id"].as_str().expect("session id");

    let opened = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-settings-and-session"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "session_id": session_id
            })),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED);
    let job_id = opened.body["id"].as_str().expect("job id");

    let settings = fixture
        .json_request(
            "PATCH",
            "/api/v1/settings",
            None,
            Some(json!({"transcription_model": "gpt-4o-transcribe"})),
        )
        .await;
    assert_eq!(settings.status, StatusCode::OK);

    let deleted = fixture
        .request(
            "DELETE",
            &format!("/api/v1/sessions/{session_id}"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    fixture.push_chunk(job_id, 0, b"audio").await;
    let finalized = fixture
        .request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/finalize"),
            None,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finalized.status(), StatusCode::ACCEPTED);

    let row: (Option<String>, String) = sqlx::query_as(
        r#"
        SELECT session_id, transcription_model
        FROM transcription_jobs
        WHERE api_key_id = 'alpha' AND id = ?
        "#,
    )
    .bind(job_id)
    .fetch_one(fixture.storage.pool())
    .await
    .expect("job row");
    assert_eq!(row.0.as_deref(), Some(session_id));
    assert_eq!(row.1, "gpt-4o-transcribe");

    let replayed = fixture
        .json_request(
            "POST",
            "/api/v1/transcription-jobs",
            Some("attempt-settings-and-session"),
            Some(json!({
                "recorded_at": "2026-04-24T17:59:00Z",
                "chunk_count": 1,
                "audio_format": "wav",
                "session_id": session_id
            })),
        )
        .await;
    assert_eq!(replayed.status, StatusCode::OK);
    assert_eq!(replayed.body["id"], job_id);
    assert_eq!(replayed.body["status"], "queued");
}

struct JsonResponse {
    status: StatusCode,
    body: Value,
}

struct TranscriptionJobFixture {
    _tempdir: TempDir,
    storage: Storage,
    app: axum::Router,
}

impl TranscriptionJobFixture {
    async fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let accepted_audio_dir = tempdir.path().join("accepted-audio");
        std::fs::create_dir(&accepted_audio_dir).expect("create accepted audio dir");
        let storage = Storage::connect(&tempdir.path().join("oracy.sqlite"))
            .await
            .expect("connect storage");
        let auth_store = AuthStore::try_from_configs(&[ApiKeyConfig {
            api_key_id: "alpha".to_owned(),
            key: "alpha-secret".to_owned(),
        }])
        .expect("auth config");
        let app = build_router(AppState {
            accepted_audio_dir: accepted_audio_dir.clone(),
            auth_store: Arc::new(auth_store),
            metrics: oracy_backend::metrics::Metrics::new(),
            operator_listen_addr: "127.0.0.1:9090".parse().expect("operator listen addr"),
            openai_api_key: "test-openai-key".to_owned(),
            storage: storage.clone(),
        });

        Self {
            _tempdir: tempdir,
            storage,
            app,
        }
    }

    async fn get_json(&self, path: &str) -> Value {
        let response = self.request("GET", path, None, Body::empty(), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    async fn json_request(
        &self,
        method: &str,
        path: &str,
        idempotency_key: Option<&str>,
        body: Option<Value>,
    ) -> JsonResponse {
        let response = self
            .request(
                method,
                path,
                idempotency_key,
                Body::from(body.map(|value| value.to_string()).unwrap_or_default()),
                Some("application/json"),
            )
            .await;
        let status = response.status();
        let body = json_body(response).await;
        JsonResponse { status, body }
    }

    async fn push_chunk(&self, job_id: &str, chunk_index: usize, bytes: &[u8]) {
        let response = self.push_chunk_response(job_id, chunk_index, bytes).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    async fn push_chunk_response(
        &self,
        job_id: &str,
        chunk_index: usize,
        bytes: &[u8],
    ) -> axum::response::Response {
        let boundary = format!("oracy-boundary-{chunk_index}");
        let chunk_sha256 = sha256_hex(bytes);
        let body = multipart_body(&boundary, chunk_index, &chunk_sha256, bytes);
        self.request(
            "POST",
            &format!("/api/v1/transcription-jobs/{job_id}/chunks"),
            None,
            Body::from(body),
            Some(&format!("multipart/form-data; boundary={boundary}")),
        )
        .await
    }

    async fn accepted_chunk_path(&self, job_id: &str, chunk_index: usize) -> std::path::PathBuf {
        let path: String = sqlx::query_scalar(
            r#"
            SELECT chunk_path
            FROM transcription_job_chunks
            WHERE api_key_id = 'alpha' AND job_id = ? AND chunk_index = ?
            "#,
        )
        .bind(job_id)
        .bind(chunk_index as i64)
        .fetch_one(self.storage.pool())
        .await
        .expect("accepted chunk path");
        std::path::PathBuf::from(path)
    }

    async fn accepted_audio_path(&self, job_id: &str) -> std::path::PathBuf {
        let path: String = sqlx::query_scalar(
            r#"
            SELECT accepted_audio_path
            FROM transcription_jobs
            WHERE api_key_id = 'alpha' AND id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("accepted audio path");
        std::path::PathBuf::from(path)
    }

    async fn stored_audio_hash(&self, job_id: &str) -> String {
        sqlx::query_scalar(
            r#"
            SELECT audio_sha256_hex
            FROM transcription_jobs
            WHERE api_key_id = 'alpha' AND id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("stored audio hash")
    }

    async fn file_prefix(&self, path: &std::path::Path, len: usize) -> Vec<u8> {
        use tokio::io::AsyncReadExt;

        let mut file = tokio::fs::File::open(path).await.expect("open file");
        let mut prefix = vec![0; len];
        file.read_exact(&mut prefix).await.expect("read prefix");
        prefix
    }

    async fn file_suffix(&self, path: &std::path::Path, len: usize) -> Vec<u8> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut file = tokio::fs::File::open(path).await.expect("open file");
        let size = file.metadata().await.expect("file metadata").len();
        file.seek(std::io::SeekFrom::Start(size - len as u64))
            .await
            .expect("seek suffix");
        let mut suffix = vec![0; len];
        file.read_exact(&mut suffix).await.expect("read suffix");
        suffix
    }

    async fn chunk_count(&self, job_id: &str) -> i64 {
        sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_job_chunks
            WHERE api_key_id = 'alpha' AND job_id = ?
            "#,
        )
        .bind(job_id)
        .fetch_one(self.storage.pool())
        .await
        .expect("chunk count")
    }

    async fn job_count_by_idempotency_key(&self, idempotency_key: &str) -> i64 {
        sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_jobs
            WHERE api_key_id = 'alpha' AND idempotency_key = ?
            "#,
        )
        .bind(idempotency_key)
        .fetch_one(self.storage.pool())
        .await
        .expect("job count")
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        idempotency_key: Option<&str>,
        body: Body,
        content_type: Option<&str>,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", "Bearer alpha-secret");
        if let Some(content_type) = content_type {
            builder = builder.header("Content-Type", content_type);
        }
        if let Some(idempotency_key) = idempotency_key {
            builder = builder.header("Idempotency-Key", idempotency_key);
        }

        self.app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .expect("response")
    }
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes).expect("valid json")
}

fn multipart_body(boundary: &str, chunk_index: usize, chunk_sha256: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"chunk_index\"\r\n\r\n");
    body.extend_from_slice(chunk_index.to_string().as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"chunk_sha256\"\r\n\r\n");
    body.extend_from_slice(chunk_sha256.as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"chunk.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn composed_audio_hash_hex(chunks: &[Vec<u8>]) -> String {
    let mut hasher = Sha256::new();
    for chunk in chunks {
        let chunk_hash = Sha256::digest(chunk);
        hasher.update(chunk_hash);
    }
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
