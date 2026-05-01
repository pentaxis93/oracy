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

const NOTE_OLD: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5R";
const NOTE_NEW: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5S";
const NOTE_OTHER_OWNER: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5T";
const NOTE_PAGE_A: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5A";
const NOTE_PAGE_B: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5B";
const NOTE_PAGE_C: &str = "01JS8D6E2S3T1J7H9J2Q2N4P5C";
const NOTE_IN_SESSION: &str = "01JS8D6E2S3T1J7H9J2Q2N4P60";
const NOTE_OUTSIDE_SESSION: &str = "01JS8D6E2S3T1J7H9J2Q2N4P61";
const NOTE_MISSING: &str = "01JS8D6E2S3T1J7H9J2Q2N4P62";
const VERSION_OLD: &str = "01JS9P1D6CK9M0N1P2Q3R4S5T6";
const VERSION_NEW: &str = "01JS9P1D6CK9M0N1P2Q3R4S5T7";
const SEGMENT_FIRST: &str = "01JS9P1K2AQ3B4C5D6E7F8G9H0";
const SEGMENT_SECOND: &str = "01JS9P1K2AQ3B4C5D6E7F8G9H1";
const TAG_MEETING: &str = "01JS9P0Q0THR2X3E4A5B6C7D8E";
const TAG_MISSING: &str = "01JS9P0Q0THR2X3E4A5B6C7D8F";
const SESSION_A: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X1";
const SESSION_BETA: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X2";
const SESSION_MISSING: &str = "01JS9P0X3NM4Q5R6S7T8V9W0X3";
const JOB_ALPHA: &str = "01JS8D6E2S3T1J7H9J2Q2N4P63";

#[tokio::test]
async fn voice_note_history_returns_owner_scoped_notes_newest_first() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
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
            id: NOTE_NEW,
            version_id: VERSION_NEW,
            text: "newer note",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:30.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: NOTE_OTHER_OWNER,
            version_id: NOTE_OTHER_OWNER,
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
                    "id": NOTE_NEW,
                    "current_version_id": VERSION_NEW,
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
                            "id": TAG_MEETING,
                            "name": "Meeting",
                            "created_at": "2026-04-24T18:01:30Z"
                        }
                    ]
                },
                {
                    "id": NOTE_OLD,
                    "current_version_id": VERSION_OLD,
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
    for id in [NOTE_PAGE_A, NOTE_PAGE_C, NOTE_PAGE_B] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id: id,
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
        vec![NOTE_PAGE_C, NOTE_PAGE_B]
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
        vec![NOTE_PAGE_A]
    );
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn voice_note_detail_returns_not_found_for_missing_other_owner_and_job_ids() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: NOTE_OTHER_OWNER,
            version_id: VERSION_OLD,
            text: "other owner",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture.insert_job("alpha", JOB_ALPHA).await;

    for voice_note_id in [NOTE_MISSING, NOTE_OTHER_OWNER, JOB_ALPHA] {
        let response = fixture
            .get(
                &format!("/api/v1/voice-notes/{voice_note_id}"),
                "alpha-secret",
            )
            .await;
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
            id: NOTE_OLD,
            version_id: VERSION_OLD,
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
            NOTE_OLD,
            VERSION_NEW,
            2,
            "edited text",
            "2026-04-24T18:01:00.000000000Z",
        )
        .await;

    let body = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/versions"),
            "alpha-secret",
        )
        .await;

    assert_eq!(
        body,
        json!({
            "items": [
                {
                    "id": VERSION_NEW,
                    "voice_note_id": NOTE_OLD,
                    "text": "edited text",
                    "created_at": "2026-04-24T18:01:00Z"
                },
                {
                    "id": VERSION_OLD,
                    "voice_note_id": NOTE_OLD,
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
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "initial text",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_segment("alpha", NOTE_OLD, SEGMENT_SECOND, 1, "second")
        .await;
    fixture
        .insert_segment("alpha", NOTE_OLD, SEGMENT_FIRST, 0, "first")
        .await;

    let first = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/segments?limit=1"),
            "alpha-secret",
        )
        .await;
    assert_eq!(first["items"][0]["id"], SEGMENT_FIRST);
    let cursor = first["next_cursor"].as_str().expect("next cursor");

    let second = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/segments?limit=1&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(second["items"][0]["id"], SEGMENT_SECOND);
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn session_voice_note_history_requires_owned_session_and_lists_only_session_notes() {
    let fixture = VoiceNoteFixture::new().await;
    fixture.insert_session("alpha", SESSION_A).await;
    fixture.insert_session("beta", SESSION_BETA).await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_IN_SESSION,
            version_id: VERSION_OLD,
            text: "in session",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: Some(SESSION_A),
            tags: vec![],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OUTSIDE_SESSION,
            version_id: VERSION_NEW,
            text: "outside session",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:30.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let body = fixture
        .get_json(
            &format!("/api/v1/sessions/{SESSION_A}/voice-notes"),
            "alpha-secret",
        )
        .await;
    assert_eq!(body["items"][0]["id"], NOTE_IN_SESSION);
    assert_eq!(body["items"].as_array().expect("items").len(), 1);

    let body = fixture
        .get_json(
            &format!("/api/v1/sessions/{SESSION_A}/voice-notes?session_id=not-a-ulid"),
            "alpha-secret",
        )
        .await;
    assert_eq!(body["items"][0]["id"], NOTE_IN_SESSION);

    for session_id in [SESSION_MISSING, SESSION_BETA] {
        let response = fixture
            .get(
                &format!("/api/v1/sessions/{session_id}/voice-notes"),
                "alpha-secret",
            )
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn root_collection_deferred_filters_return_empty_results_without_false_positives() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "history result",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    for path in [
        "/api/v1/voice-notes?q=hello".to_owned(),
        "/api/v1/voice-notes?q=hello&search_mode=keyword".to_owned(),
        format!("/api/v1/voice-notes?tag_id={TAG_MEETING}"),
        format!("/api/v1/voice-notes?tag_id={TAG_MISSING}&tag_id={TAG_MEETING}"),
        format!("/api/v1/voice-notes?session_id={SESSION_MISSING}"),
        "/api/v1/voice-notes?recorded_after=2026-01-01T00%3A00%3A00Z".to_owned(),
        "/api/v1/voice-notes?recorded_before=2026-01-02T00%3A00%3A00Z".to_owned(),
        "/api/v1/voice-notes?created_after=2026-01-01T00%3A00%3A00Z".to_owned(),
        "/api/v1/voice-notes?created_before=2026-01-02T00%3A00%3A00Z".to_owned(),
    ] {
        assert_empty_collection(fixture.get_json(&path, "alpha-secret").await);
    }
}

#[tokio::test]
async fn root_collection_ignores_unknown_parameters_without_deferring_history() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "history result",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let body = fixture
        .get_json("/api/v1/voice-notes?unknown=value", "alpha-secret")
        .await;
    assert_eq!(body["items"][0]["id"], NOTE_OLD);
}

#[tokio::test]
async fn session_collection_deferred_filters_return_empty_results_for_owned_session() {
    let fixture = VoiceNoteFixture::new().await;
    fixture.insert_session("alpha", SESSION_A).await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_IN_SESSION,
            version_id: VERSION_OLD,
            text: "in session",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: Some(SESSION_A),
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;

    for path in [
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?q=hello"),
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?q=hello&search_mode=keyword"),
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?tag_id={TAG_MEETING}"),
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?recorded_after=2026-01-01T00%3A00%3A00Z"),
        format!(
            "/api/v1/sessions/{SESSION_A}/voice-notes?recorded_before=2026-01-02T00%3A00%3A00Z"
        ),
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?created_after=2026-01-01T00%3A00%3A00Z"),
        format!("/api/v1/sessions/{SESSION_A}/voice-notes?created_before=2026-01-02T00%3A00%3A00Z"),
    ] {
        assert_empty_collection(fixture.get_json(&path, "alpha-secret").await);
    }
}

#[tokio::test]
async fn invalid_deferred_collection_query_values_return_validation_errors() {
    let fixture = VoiceNoteFixture::new().await;

    for (path, field) in [
        (
            "/api/v1/voice-notes?q=hello&search_mode=typo".to_owned(),
            "search_mode",
        ),
        ("/api/v1/voice-notes?tag_id=not-a-ulid".to_owned(), "tag_id"),
        (
            format!("/api/v1/voice-notes?tag_id={TAG_MISSING}&tag_id=not-a-ulid"),
            "tag_id",
        ),
        (
            "/api/v1/voice-notes?session_id=not-a-ulid".to_owned(),
            "session_id",
        ),
        (
            "/api/v1/voice-notes?recorded_after=not-a-time".to_owned(),
            "recorded_after",
        ),
        (
            "/api/v1/voice-notes?recorded_after=2026-01-01T00%3A00%3A00%2B01%3A00".to_owned(),
            "recorded_after",
        ),
        (
            "/api/v1/voice-notes?recorded_before=not-a-time".to_owned(),
            "recorded_before",
        ),
        (
            "/api/v1/voice-notes?created_after=not-a-time".to_owned(),
            "created_after",
        ),
        (
            "/api/v1/voice-notes?created_before=not-a-time".to_owned(),
            "created_before",
        ),
        (
            "/api/v1/voice-notes?search_mode=keyword".to_owned(),
            "search_mode",
        ),
    ] {
        let response = fixture.get(&path, "alpha-secret").await;
        assert_validation_error(response, field).await;
    }
}

#[tokio::test]
async fn each_valid_search_mode_is_accepted_while_search_is_deferred() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "history result",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    for mode in ["keyword", "semantic", "hybrid"] {
        let accepted = fixture
            .get_json(
                &format!("/api/v1/voice-notes?q=hello&search_mode={mode}"),
                "alpha-secret",
            )
            .await;
        assert_empty_collection(accepted);
    }
}

#[tokio::test]
async fn malformed_path_ids_return_validation_errors_before_existence_checks() {
    let fixture = VoiceNoteFixture::new().await;
    let lowercase_ulid = NOTE_MISSING.to_ascii_lowercase();

    for (path, field) in [
        ("/api/v1/voice-notes/not-a-ulid".to_owned(), "voice_note_id"),
        (
            format!("/api/v1/voice-notes/{lowercase_ulid}"),
            "voice_note_id",
        ),
        (
            "/api/v1/voice-notes/not-a-ulid/versions".to_owned(),
            "voice_note_id",
        ),
        (
            "/api/v1/voice-notes/not-a-ulid/segments".to_owned(),
            "voice_note_id",
        ),
        (
            "/api/v1/sessions/not-a-ulid/voice-notes".to_owned(),
            "session_id",
        ),
    ] {
        let response = fixture.get(&path, "alpha-secret").await;
        assert_validation_error(response, field).await;
    }
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

#[tokio::test]
async fn collection_query_validation_precedes_existence_checks() {
    let fixture = VoiceNoteFixture::new().await;

    for (path, field) in [
        (
            format!("/api/v1/sessions/{SESSION_MISSING}/voice-notes?limit=0"),
            "limit",
        ),
        (
            format!("/api/v1/sessions/{SESSION_MISSING}/voice-notes?recorded_after=not-a-time"),
            "recorded_after",
        ),
        (
            format!("/api/v1/voice-notes/{NOTE_MISSING}/versions?cursor=not-a-cursor"),
            "cursor",
        ),
        (
            format!("/api/v1/voice-notes/{NOTE_MISSING}/segments?limit=abc"),
            "limit",
        ),
    ] {
        let response = fixture.get(&path, "alpha-secret").await;
        assert_validation_error(response, field).await;
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

async fn assert_validation_error(response: axum::response::Response, field: &str) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error_code"], "validation_error");
    assert_eq!(body["details"][0]["field"], field);
}

fn assert_empty_collection(body: Value) {
    assert_eq!(
        body,
        json!({
            "items": [],
            "next_cursor": null
        })
    );
}
