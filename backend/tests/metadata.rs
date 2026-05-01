use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::router::build_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::Storage;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::util::ServiceExt;

const TAG_ALPHA: &str = "01JS9P0Q0THR2X3E4A5B6C7D8E";
const TAG_BETA: &str = "01JS9P0Q0THR2X3E4A5B6C7D8F";
const TAG_GAMMA: &str = "01JS9P0Q0THR2X3E4A5B6C7D8G";
const TAG_MISSING: &str = "01JS9P0Q0THR2X3E4A5B6C7D8H";
const SESSION_ALPHA: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X1";
const SESSION_BETA: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X2";
const SESSION_MISSING: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X3";
const NOTE_ALPHA: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5R";
const VERSION_ALPHA: &str = "01JS9P1D6CK9M0N1P2Q3R4S5T6";
const JOB_ALPHA: &str = "01JS8D6E2S3T1J7H9J2Q2N4P63";

#[tokio::test]
async fn tag_endpoints_manage_owner_scoped_case_insensitive_tags() {
    let fixture = MetadataFixture::new().await;

    let created = fixture
        .json_request(
            "POST",
            "/api/v1/tags",
            "alpha-secret",
            Some(json!({"name": "Meeting"})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["name"], "Meeting");
    let tag_id = created.body["id"].as_str().expect("tag id");

    let duplicate = fixture
        .json_request(
            "POST",
            "/api/v1/tags",
            "alpha-secret",
            Some(json!({"name": "meeting"})),
        )
        .await;
    assert_eq!(duplicate.status, StatusCode::OK);
    assert_eq!(duplicate.body["id"], tag_id);
    assert_eq!(duplicate.body["name"], "meeting");

    let fetched = fixture
        .get_json(&format!("/api/v1/tags/{tag_id}"), "alpha-secret")
        .await;
    assert_eq!(fetched["name"], "meeting");

    let renamed = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/tags/{tag_id}"),
            "alpha-secret",
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.body["id"], tag_id);
    assert_eq!(renamed.body["name"], "Planning");

    let hidden = fixture
        .get(&format!("/api/v1/tags/{tag_id}"), "beta-secret")
        .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

    let deleted = fixture
        .request(
            "DELETE",
            &format!("/api/v1/tags/{tag_id}"),
            "alpha-secret",
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let missing = fixture
        .get(&format!("/api/v1/tags/{tag_id}"), "alpha-secret")
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tag_listing_paginates_validates_and_ignores_unknown_parameters() {
    let fixture = MetadataFixture::new().await;
    fixture
        .insert_tag("alpha", TAG_ALPHA, "Old", "2026-04-24T18:00:00.000000000Z")
        .await;
    fixture
        .insert_tag(
            "alpha",
            TAG_GAMMA,
            "Tie High",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;
    fixture
        .insert_tag(
            "alpha",
            TAG_BETA,
            "Tie Low",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;
    fixture
        .insert_tag(
            "beta",
            TAG_MISSING,
            "Hidden",
            "2026-04-24T18:02:00.000000000Z",
        )
        .await;

    let first = fixture
        .get_json("/api/v1/tags?limit=2&unknown=value", "alpha-secret")
        .await;
    assert_eq!(ids(&first), vec![TAG_GAMMA, TAG_BETA]);
    let cursor = first["next_cursor"].as_str().expect("next cursor");

    let second = fixture
        .get_json(
            &format!("/api/v1/tags?limit=2&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(ids(&second), vec![TAG_ALPHA]);
    assert_eq!(second["next_cursor"], Value::Null);

    for path in [
        "/api/v1/tags?cursor=not-a-cursor",
        "/api/v1/tags?limit=0",
        "/api/v1/tags?limit=101",
        "/api/v1/tags?limit=abc",
        "/api/v1/tags?cursor=a&cursor=b",
        "/api/v1/tags?limit=1&limit=2",
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn tag_rename_reports_not_found_before_conflict() {
    let fixture = MetadataFixture::new().await;
    fixture
        .insert_tag(
            "alpha",
            TAG_ALPHA,
            "Notes",
            "2026-04-24T18:00:00.000000000Z",
        )
        .await;
    fixture
        .insert_tag("beta", TAG_BETA, "Other", "2026-04-24T18:00:00.000000000Z")
        .await;

    for tag_id in [TAG_MISSING, TAG_BETA] {
        let response = fixture
            .json_request(
                "PATCH",
                &format!("/api/v1/tags/{tag_id}"),
                "alpha-secret",
                Some(json!({"name": "notes"})),
            )
            .await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
    }

    fixture
        .insert_tag(
            "alpha",
            TAG_GAMMA,
            "Planning",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;
    let conflict = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/tags/{TAG_GAMMA}"),
            "alpha-secret",
            Some(json!({"name": "notes"})),
        )
        .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn tag_rename_visibility_and_delete_cascade_flow_through_voice_note_associations() {
    let fixture = MetadataFixture::new().await;
    fixture
        .insert_tag(
            "alpha",
            TAG_ALPHA,
            "Meeting",
            "2026-04-24T18:00:00.000000000Z",
        )
        .await;
    fixture.insert_voice_note_with_tag("alpha", TAG_ALPHA).await;

    let renamed = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/tags/{TAG_ALPHA}"),
            "alpha-secret",
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    let note = fixture
        .get_json(&format!("/api/v1/voice-notes/{NOTE_ALPHA}"), "alpha-secret")
        .await;
    assert_eq!(note["tags"][0]["name"], "Planning");

    let deleted = fixture
        .request(
            "DELETE",
            &format!("/api/v1/tags/{TAG_ALPHA}"),
            "alpha-secret",
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let note = fixture
        .get_json(&format!("/api/v1/voice-notes/{NOTE_ALPHA}"), "alpha-secret")
        .await;
    assert_eq!(note["tags"], json!([]));
}

#[tokio::test]
async fn session_endpoints_manage_owner_scoped_sessions_without_name_uniqueness() {
    let fixture = MetadataFixture::new().await;

    let created = fixture
        .json_request(
            "POST",
            "/api/v1/sessions",
            "alpha-secret",
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["name"], "Planning");
    let session_id = created.body["id"].as_str().expect("session id");

    let same_name = fixture
        .json_request(
            "POST",
            "/api/v1/sessions",
            "alpha-secret",
            Some(json!({"name": "Planning"})),
        )
        .await;
    assert_eq!(same_name.status, StatusCode::CREATED);
    assert_ne!(same_name.body["id"], session_id);

    let renamed = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/sessions/{session_id}"),
            "alpha-secret",
            Some(json!({"name": "Interviews"})),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.body["name"], "Interviews");

    let hidden = fixture
        .get(&format!("/api/v1/sessions/{session_id}"), "beta-secret")
        .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

    let deleted = fixture
        .request(
            "DELETE",
            &format!("/api/v1/sessions/{session_id}"),
            "alpha-secret",
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn session_listing_paginates_and_validates_like_other_collections() {
    let fixture = MetadataFixture::new().await;
    fixture
        .insert_session(
            "alpha",
            SESSION_ALPHA,
            "Old",
            "2026-04-24T18:00:00.000000000Z",
        )
        .await;
    fixture
        .insert_session(
            "alpha",
            SESSION_BETA,
            "New",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;
    fixture
        .insert_session(
            "beta",
            SESSION_MISSING,
            "Hidden",
            "2026-04-24T18:02:00.000000000Z",
        )
        .await;

    let first = fixture
        .get_json("/api/v1/sessions?limit=1&unknown=value", "alpha-secret")
        .await;
    assert_eq!(ids(&first), vec![SESSION_BETA]);
    let cursor = first["next_cursor"].as_str().expect("next cursor");
    let second = fixture
        .get_json(
            &format!("/api/v1/sessions?limit=1&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(ids(&second), vec![SESSION_ALPHA]);
    assert_eq!(second["next_cursor"], Value::Null);

    for path in [
        "/api/v1/sessions?cursor=not-a-cursor",
        "/api/v1/sessions?limit=0",
        "/api/v1/sessions?limit=101",
        "/api/v1/sessions?limit=abc",
        "/api/v1/sessions?cursor=a&cursor=b",
        "/api/v1/sessions?limit=1&limit=2",
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn session_deletion_nulls_voice_notes_without_mutating_replay_tuple() {
    let fixture = MetadataFixture::new().await;
    fixture
        .insert_session(
            "alpha",
            SESSION_ALPHA,
            "Planning",
            "2026-04-24T18:00:00.000000000Z",
        )
        .await;
    fixture.insert_job_and_voice_note_in_session().await;

    let deleted = fixture
        .request(
            "DELETE",
            &format!("/api/v1/sessions/{SESSION_ALPHA}"),
            "alpha-secret",
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let note = fixture
        .get_json(&format!("/api/v1/voice-notes/{NOTE_ALPHA}"), "alpha-secret")
        .await;
    assert_eq!(note["session_id"], Value::Null);
    let job_session_id: Option<String> = sqlx::query_scalar(
        "SELECT session_id FROM transcription_jobs WHERE api_key_id = 'alpha' AND id = ?",
    )
    .bind(JOB_ALPHA)
    .fetch_one(fixture.storage.pool())
    .await
    .expect("job session lookup");
    assert_eq!(job_session_id.as_deref(), Some(SESSION_ALPHA));
}

#[tokio::test]
async fn metadata_path_validation_precedes_existence_checks() {
    let fixture = MetadataFixture::new().await;

    for (method, path, body, field) in [
        ("GET", "/api/v1/tags/not-a-ulid", None, "tag_id"),
        (
            "PATCH",
            "/api/v1/tags/not-a-ulid",
            Some(json!({"name": "x"})),
            "tag_id",
        ),
        ("DELETE", "/api/v1/tags/not-a-ulid", None, "tag_id"),
        ("GET", "/api/v1/sessions/not-a-ulid", None, "session_id"),
        (
            "PATCH",
            "/api/v1/sessions/not-a-ulid",
            Some(json!({"name": "x"})),
            "session_id",
        ),
        ("DELETE", "/api/v1/sessions/not-a-ulid", None, "session_id"),
    ] {
        let response = fixture
            .request(
                method,
                path,
                "alpha-secret",
                body.map(|value| value.to_string()),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["details"][0]["field"], field);
    }
}

#[tokio::test]
async fn metadata_routes_require_authentication() {
    let fixture = MetadataFixture::new().await;

    for request in [
        Request::builder()
            .uri("/api/v1/tags")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/sessions")
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"name": "Planning"}).to_string()))
            .unwrap(),
        Request::builder()
            .uri("/api/v1/sessions")
            .header("Authorization", "Bearer unknown-secret")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = fixture.app().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

struct JsonResponse {
    status: StatusCode,
    body: Value,
}

struct MetadataFixture {
    _tempdir: TempDir,
    app: axum::Router,
    storage: Storage,
}

impl MetadataFixture {
    async fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let accepted_audio_dir = tempdir.path().join("accepted-audio");
        std::fs::create_dir(&accepted_audio_dir).expect("create accepted audio dir");
        let storage = Storage::connect(&tempdir.path().join("oracy.sqlite"))
            .await
            .expect("connect storage");
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
        .expect("auth config");
        let app = build_router(AppState {
            accepted_audio_dir,
            auth_store: Arc::new(auth_store),
            storage: storage.clone(),
        });

        Self {
            _tempdir: tempdir,
            app,
            storage,
        }
    }

    fn app(&self) -> axum::Router {
        self.app.clone()
    }

    async fn get(&self, path: &str, bearer: &str) -> axum::response::Response {
        self.request("GET", path, bearer, None).await
    }

    async fn get_json(&self, path: &str, bearer: &str) -> Value {
        let response = self.get(path, bearer).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    async fn json_request(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<Value>,
    ) -> JsonResponse {
        let response = self
            .request(method, path, bearer, body.map(|value| value.to_string()))
            .await;
        let status = response.status();
        let body = json_body(response).await;
        JsonResponse { status, body }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<String>,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", format!("Bearer {bearer}"));
        let body = match body {
            Some(body) => {
                builder = builder.header("Content-Type", "application/json");
                Body::from(body)
            }
            None => Body::empty(),
        };
        self.app()
            .oneshot(builder.body(body).unwrap())
            .await
            .expect("response")
    }

    async fn insert_tag(&self, owner: &str, id: &str, name: &str, created_at: &str) {
        sqlx::query(
            r#"
            INSERT INTO tags (id, api_key_id, name, name_folded, created_at)
            VALUES (?, ?, ?, lower(?), ?)
            "#,
        )
        .bind(id)
        .bind(owner)
        .bind(name)
        .bind(name)
        .bind(created_at)
        .execute(self.storage.pool())
        .await
        .expect("insert tag");
    }

    async fn insert_session(&self, owner: &str, id: &str, name: &str, created_at: &str) {
        sqlx::query(
            r#"
            INSERT INTO sessions (id, api_key_id, name, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(owner)
        .bind(name)
        .bind(created_at)
        .execute(self.storage.pool())
        .await
        .expect("insert session");
    }

    async fn insert_voice_note_with_tag(&self, owner: &str, tag_id: &str) {
        self.insert_voice_note(owner, None).await;
        sqlx::query(
            r#"
            INSERT INTO voice_note_tags (api_key_id, voice_note_id, tag_id)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(owner)
        .bind(NOTE_ALPHA)
        .bind(tag_id)
        .execute(self.storage.pool())
        .await
        .expect("insert voice note tag");
    }

    async fn insert_job_and_voice_note_in_session(&self) {
        self.insert_voice_note("alpha", Some(SESSION_ALPHA)).await;
        sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, api_key_id, idempotency_key, audio_sha256_hex,
                audio_content_hash_algorithm, recorded_at, session_id,
                accepted_audio_path, status, created_at, updated_at,
                retry_count, max_retries, voice_note_id
            )
            VALUES (
                ?, 'alpha', 'attempt-a', 'hash', 'sha256:chunk-sha256-v1',
                '2026-04-24T17:59:00.000000000Z', ?, '/tmp/audio',
                'succeeded', '2026-04-24T18:00:00.000000000Z',
                '2026-04-24T18:00:00.000000000Z', 0, 3, ?
            )
            "#,
        )
        .bind(JOB_ALPHA)
        .bind(SESSION_ALPHA)
        .bind(NOTE_ALPHA)
        .execute(self.storage.pool())
        .await
        .expect("insert job");
    }

    async fn insert_voice_note(&self, owner: &str, session_id: Option<&str>) {
        sqlx::query(
            r#"
            INSERT INTO voice_notes (
                id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
                language, model, processing_time_ms, cost_cents,
                created_at, recorded_at, session_id
            )
            VALUES (?, ?, 12.5, 'wav', 401280, 'en', 'gpt-4o-mini-transcribe', 1843, NULL, ?, ?, ?)
            "#,
        )
        .bind(NOTE_ALPHA)
        .bind(owner)
        .bind("2026-04-24T18:00:00.000000000Z")
        .bind("2026-04-24T17:59:00.000000000Z")
        .bind(session_id)
        .execute(self.storage.pool())
        .await
        .expect("insert voice note");

        sqlx::query(
            r#"
            INSERT INTO voice_note_versions (
                id, api_key_id, voice_note_id, version_number, text, created_at
            )
            VALUES (?, ?, ?, 1, 'note text', '2026-04-24T18:00:00.000000000Z')
            "#,
        )
        .bind(VERSION_ALPHA)
        .bind(owner)
        .bind(NOTE_ALPHA)
        .execute(self.storage.pool())
        .await
        .expect("insert voice note version");
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

fn ids(body: &Value) -> Vec<&str> {
    body["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["id"].as_str().expect("id"))
        .collect()
}
