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

#[tokio::test]
async fn voice_note_history_returns_owner_scoped_notes_newest_first() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-old",
            version_id: "version-old",
            text: "older note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-new",
            version_id: "version-new",
            text: "newer note",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:30.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: "tag-meeting",
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: "note-other-owner",
            version_id: "version-other-owner",
            text: "hidden note",
            created_at: "2026-04-24T18:02:00.000000000Z",
            recorded_at: "2026-04-24T18:01:30.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let response = fixture
        .app()
        .oneshot(
            Request::builder()
                .uri("/api/v1/voice-notes")
                .header("Authorization", "Bearer alpha-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "items": [
                {
                    "id": "note-new",
                    "current_version_id": "version-new",
                    "text": "newer note",
                    "audio_duration_seconds": 12.5,
                    "audio_format": "wav",
                    "audio_size_bytes": 401280,
                    "language": "en",
                    "model": "gpt-4o-mini-transcribe",
                    "processing_time_ms": 1843,
                    "cost_cents": null,
                    "created_at": "2026-04-24T18:01:00Z",
                    "recorded_at": "2026-04-24T18:00:30Z",
                    "session_id": null,
                    "tags": [
                        {
                            "id": "tag-meeting",
                            "name": "Meeting",
                            "created_at": "2026-04-24T18:01:30Z"
                        }
                    ]
                },
                {
                    "id": "note-old",
                    "current_version_id": "version-old",
                    "text": "older note",
                    "audio_duration_seconds": 12.5,
                    "audio_format": "wav",
                    "audio_size_bytes": 401280,
                    "language": "en",
                    "model": "gpt-4o-mini-transcribe",
                    "processing_time_ms": 1843,
                    "cost_cents": null,
                    "created_at": "2026-04-24T18:00:00Z",
                    "recorded_at": "2026-04-24T17:59:00Z",
                    "session_id": null,
                    "tags": []
                }
            ],
            "next_cursor": null
        })
    );
}

#[tokio::test]
async fn voice_note_history_paginates_with_descending_id_tiebreaker() {
    let fixture = VoiceNoteFixture::new().await;
    for id in ["note-a", "note-c", "note-b"] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id: &format!("{id}-version"),
                text: id,
                created_at: "2026-04-24T18:00:00.000000000Z",
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id: None,
                tags: vec![],
            })
            .await;
    }

    let first = fixture
        .get_json("/api/v1/voice-notes?limit=2", "alpha-secret")
        .await;
    assert_eq!(
        first["items"]
            .as_array()
            .expect("items array")
            .iter()
            .map(|item| item["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        vec!["note-c", "note-b"]
    );
    let cursor = first["next_cursor"].as_str().expect("next cursor");

    let second = fixture
        .get_json(
            &format!("/api/v1/voice-notes?limit=2&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(
        second["items"]
            .as_array()
            .expect("items array")
            .iter()
            .map(|item| item["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        vec!["note-a"]
    );
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn voice_note_detail_returns_not_found_for_missing_other_owner_and_job_ids() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: "note-beta",
            version_id: "version-beta",
            text: "other owner",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture.insert_job("alpha", "job-alpha").await;

    for path in [
        "/api/v1/voice-notes/note-missing",
        "/api/v1/voice-notes/note-beta",
        "/api/v1/voice-notes/job-alpha",
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(response).await["error_code"], "not_found");
    }
}

#[tokio::test]
async fn version_history_returns_versions_newest_first_in_shared_envelope() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-a",
            version_id: "version-1",
            text: "initial text",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_version(
            "alpha",
            "note-a",
            "version-2",
            2,
            "edited text",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;

    let body = fixture
        .get_json("/api/v1/voice-notes/note-a/versions", "alpha-secret")
        .await;

    assert_eq!(
        body,
        json!({
            "items": [
                {
                    "id": "version-2",
                    "voice_note_id": "note-a",
                    "text": "edited text",
                    "created_at": "2026-04-24T18:01:00Z"
                },
                {
                    "id": "version-1",
                    "voice_note_id": "note-a",
                    "text": "initial text",
                    "created_at": "2026-04-24T18:00:00Z"
                }
            ],
            "next_cursor": null
        })
    );
}

#[tokio::test]
async fn segments_return_in_position_order_with_pagination() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-a",
            version_id: "version-1",
            text: "initial text",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_segment("alpha", "note-a", "segment-2", 1, "second")
        .await;
    fixture
        .insert_segment("alpha", "note-a", "segment-1", 0, "first")
        .await;

    let first = fixture
        .get_json(
            "/api/v1/voice-notes/note-a/segments?limit=1",
            "alpha-secret",
        )
        .await;
    assert_eq!(first["items"][0]["id"], "segment-1");
    let cursor = first["next_cursor"].as_str().expect("next cursor");

    let second = fixture
        .get_json(
            &format!("/api/v1/voice-notes/note-a/segments?limit=1&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(second["items"][0]["id"], "segment-2");
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn session_voice_note_history_requires_owned_session_and_lists_only_session_notes() {
    let fixture = VoiceNoteFixture::new().await;
    fixture.insert_session("alpha", "session-a").await;
    fixture.insert_session("beta", "session-beta").await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-in-session",
            version_id: "version-in-session",
            text: "in session",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: Some("session-a"),
            tags: vec![],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-outside-session",
            version_id: "version-outside-session",
            text: "outside session",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:30.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let body = fixture
        .get_json("/api/v1/sessions/session-a/voice-notes", "alpha-secret")
        .await;
    assert_eq!(body["items"][0]["id"], "note-in-session");
    assert_eq!(body["items"].as_array().expect("items").len(), 1);

    for path in [
        "/api/v1/sessions/session-missing/voice-notes",
        "/api/v1/sessions/session-beta/voice-notes",
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn collection_queries_accept_deferred_and_unknown_parameters_but_reject_search_mode_without_q()
 {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: "note-a",
            version_id: "version-a",
            text: "history result",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let accepted = fixture
        .get_json(
            "/api/v1/voice-notes?q=hello&search_mode=keyword&tag_id=tag-missing&session_id=session-missing&recorded_after=2026-01-01T00%3A00%3A00Z&unknown=value",
            "alpha-secret",
        )
        .await;
    assert_eq!(accepted["items"][0]["id"], "note-a");

    let rejected = fixture
        .get("/api/v1/voice-notes?search_mode=keyword", "alpha-secret")
        .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(rejected).await["error_code"], "validation_error");
}

#[tokio::test]
async fn malformed_cursor_and_invalid_limit_return_validation_errors() {
    let fixture = VoiceNoteFixture::new().await;

    for path in [
        "/api/v1/voice-notes?cursor=not-a-cursor",
        "/api/v1/voice-notes?limit=0",
        "/api/v1/voice-notes?limit=101",
        "/api/v1/voice-notes?limit=abc",
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error_code"], "validation_error");
    }
}

struct VoiceNoteFixture {
    _tempdir: TempDir,
    app: axum::Router,
    storage: Storage,
}

struct VoiceNoteSeed<'a> {
    owner: &'a str,
    id: &'a str,
    version_id: &'a str,
    text: &'a str,
    created_at: &'a str,
    recorded_at: &'a str,
    session_id: Option<&'a str>,
    tags: Vec<TagSeed<'a>>,
}

struct TagSeed<'a> {
    id: &'a str,
    name: &'a str,
    created_at: &'a str,
}

impl VoiceNoteFixture {
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
        self.app()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("Authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response")
    }

    async fn get_json(&self, path: &str, bearer: &str) -> Value {
        let response = self.get(path, bearer).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    async fn insert_session(&self, owner: &str, session_id: &str) {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO sessions (id, api_key_id, name, created_at)
            VALUES (?, ?, 'Session', '2026-04-24T17:58:00.000000000Z')
            "#,
        )
        .bind(session_id)
        .bind(owner)
        .execute(self.storage.pool())
        .await
        .expect("insert session");
    }

    async fn insert_voice_note(&self, seed: VoiceNoteSeed<'_>) {
        if let Some(session_id) = seed.session_id {
            self.insert_session(seed.owner, session_id).await;
        }

        sqlx::query(
            r#"
            INSERT INTO transcripts (
                id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
                transcript_language, model, processing_time_ms, cost_cents,
                created_at, recorded_at, session_id
            )
            VALUES (?, ?, 12.5, 'wav', 401280, 'en', 'gpt-4o-mini-transcribe', 1843, NULL, ?, ?, ?)
            "#,
        )
        .bind(seed.id)
        .bind(seed.owner)
        .bind(seed.created_at)
        .bind(seed.recorded_at)
        .bind(seed.session_id)
        .execute(self.storage.pool())
        .await
        .expect("insert transcript");

        sqlx::query(
            r#"
            INSERT INTO transcript_versions (
                id, api_key_id, transcript_id, version_number, transcript, created_at
            )
            VALUES (?, ?, ?, 1, ?, ?)
            "#,
        )
        .bind(seed.version_id)
        .bind(seed.owner)
        .bind(seed.id)
        .bind(seed.text)
        .bind(seed.created_at)
        .execute(self.storage.pool())
        .await
        .expect("insert transcript version");

        for tag in seed.tags {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO tags (id, api_key_id, name, name_folded, created_at)
                VALUES (?, ?, ?, lower(?), ?)
                "#,
            )
            .bind(tag.id)
            .bind(seed.owner)
            .bind(tag.name)
            .bind(tag.name)
            .bind(tag.created_at)
            .execute(self.storage.pool())
            .await
            .expect("insert tag");

            sqlx::query(
                r#"
                INSERT INTO transcript_tags (api_key_id, transcript_id, tag_id)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(seed.owner)
            .bind(seed.id)
            .bind(tag.id)
            .execute(self.storage.pool())
            .await
            .expect("insert transcript tag");
        }
    }

    async fn insert_version(
        &self,
        owner: &str,
        voice_note_id: &str,
        version_id: &str,
        version_number: i64,
        text: &str,
        created_at: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO transcript_versions (
                id, api_key_id, transcript_id, version_number, transcript, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(version_id)
        .bind(owner)
        .bind(voice_note_id)
        .bind(version_number)
        .bind(text)
        .bind(created_at)
        .execute(self.storage.pool())
        .await
        .expect("insert transcript version");
    }

    async fn insert_segment(
        &self,
        owner: &str,
        voice_note_id: &str,
        segment_id: &str,
        position: i64,
        text: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO segments (
                id, api_key_id, transcript_id, position, start_ms, end_ms, text
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(segment_id)
        .bind(owner)
        .bind(voice_note_id)
        .bind(position)
        .bind(position * 1000)
        .bind((position + 1) * 1000)
        .bind(text)
        .execute(self.storage.pool())
        .await
        .expect("insert segment");
    }

    async fn insert_job(&self, owner: &str, job_id: &str) {
        sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, api_key_id, idempotency_key, audio_sha256_hex,
                audio_content_hash_algorithm, recorded_at, accepted_audio_path,
                status, created_at, updated_at, retry_count, max_retries
            )
            VALUES (
                ?, ?, ?, 'hash', 'sha256:chunk-sha256-v1',
                '2026-04-24T17:59:00.000000000Z', '/tmp/audio',
                'queued', '2026-04-24T18:00:00.000000000Z',
                '2026-04-24T18:00:00.000000000Z', 0, 3
            )
            "#,
        )
        .bind(job_id)
        .bind(owner)
        .bind(format!("{job_id}-attempt"))
        .execute(self.storage.pool())
        .await
        .expect("insert job");
    }
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("valid json")
}
