use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use oracy_backend::auth::AuthStore;
use oracy_backend::config::ApiKeyConfig;
use oracy_backend::router::build_router;
use oracy_backend::state::AppState;
use oracy_backend::storage::{Storage, encode_embedding_vector};
use serde_json::{Value, json};
use sqlx::{QueryBuilder, Sqlite};
use tempfile::TempDir;
use time::macros::datetime;
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
const TAG_ACTION: &str = "01JS9P0Q0THR2X3E4A5B6C7D8G";
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
async fn patch_voice_note_text_appends_version_updates_reads_and_initiates_embedding_regeneration()
{
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
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;
    fixture
        .insert_segment("alpha", NOTE_OLD, SEGMENT_FIRST, 0, "original segment")
        .await;

    let response = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/voice-notes/{NOTE_OLD}"),
            "alpha-secret",
            Some(json!({"text": "edited text"})),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["id"], NOTE_OLD);
    assert_eq!(response.body["text"], "edited text");
    assert_ne!(response.body["current_version_id"], VERSION_OLD);

    let versions = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/versions"),
            "alpha-secret",
        )
        .await;
    assert_eq!(versions["items"].as_array().expect("versions").len(), 2);
    assert_eq!(versions["items"][0]["text"], "edited text");
    assert_eq!(versions["items"][1]["id"], VERSION_OLD);

    let detail = fixture
        .get_json(&format!("/api/v1/voice-notes/{NOTE_OLD}"), "alpha-secret")
        .await;
    assert_eq!(detail["text"], "edited text");
    assert_eq!(
        detail["current_version_id"],
        response.body["current_version_id"]
    );
    assert_eq!(detail["tags"][0]["id"], TAG_MEETING);

    let segments = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/segments"),
            "alpha-secret",
        )
        .await;
    assert_eq!(segments["items"][0]["text"], "original segment");

    let regeneration_job = fixture
        .storage
        .claim_next_embedding_regeneration_job(
            "regen-lease",
            datetime!(2026-04-24 18:10:00 UTC),
            datetime!(2026-04-24 18:15:00 UTC),
        )
        .await
        .expect("claim regeneration job")
        .expect("regeneration job exists");
    assert_eq!(regeneration_job.api_key_id, "alpha");
    assert_eq!(regeneration_job.voice_note_id, NOTE_OLD);
    assert_eq!(
        regeneration_job.voice_note_version_id,
        response.body["current_version_id"]
    );
    assert_eq!(regeneration_job.text, "edited text");
}

#[tokio::test]
async fn patch_voice_note_text_reports_contract_errors_for_invalid_requests() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: NOTE_OTHER_OWNER,
            version_id: NOTE_OTHER_OWNER,
            text: "hidden note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let malformed_path = fixture
        .json_request(
            "PATCH",
            "/api/v1/voice-notes/not-a-ulid",
            "alpha-secret",
            Some(json!({"text": "edited"})),
        )
        .await;
    assert_eq!(malformed_path.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed_path.body["details"][0]["field"], "voice_note_id");

    for voice_note_id in [NOTE_MISSING, NOTE_OTHER_OWNER] {
        let response = fixture
            .json_request(
                "PATCH",
                &format!("/api/v1/voice-notes/{voice_note_id}"),
                "alpha-secret",
                Some(json!({"text": "edited"})),
            )
            .await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert_eq!(response.body["error_code"], "not_found");
    }

    let malformed_json = fixture
        .request(
            "PATCH",
            &format!("/api/v1/voice-notes/{NOTE_MISSING}"),
            "alpha-secret",
            Some("{".to_owned()),
        )
        .await;
    assert_eq!(malformed_json.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(malformed_json).await["error_code"],
        "malformed_json"
    );

    let invalid_shape = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/voice-notes/{NOTE_MISSING}"),
            "alpha-secret",
            Some(json!({"text": 1})),
        )
        .await;
    assert_eq!(invalid_shape.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid_shape.body["error_code"], "invalid_request_shape");

    let blank_text = fixture
        .json_request(
            "PATCH",
            &format!("/api/v1/voice-notes/{NOTE_MISSING}"),
            "alpha-secret",
            Some(json!({"text": "   \n\t"})),
        )
        .await;
    assert_eq!(blank_text.status, StatusCode::BAD_REQUEST);
    assert_eq!(blank_text.body["details"][0]["field"], "text");

    let unauthorized = fixture
        .app()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/voice-notes/{NOTE_MISSING}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({"text": "edited"}).to_string()))
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
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
async fn put_voice_note_tags_replaces_the_full_tag_set_without_creating_a_version() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "tagged note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;
    fixture
        .insert_tag(
            "alpha",
            TAG_ACTION,
            "Action",
            "2026-04-24T18:01:40.000000000Z",
        )
        .await;
    fixture
        .insert_tag(
            "beta",
            TAG_MISSING,
            "Hidden",
            "2026-04-24T18:01:50.000000000Z",
        )
        .await;

    let response = fixture
        .json_request(
            "PUT",
            &format!("/api/v1/voice-notes/{NOTE_OLD}/tags"),
            "alpha-secret",
            Some(json!({"tag_ids": [TAG_ACTION]})),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["tags"][0]["id"], TAG_ACTION);
    assert_eq!(response.body["current_version_id"], VERSION_OLD);

    let versions = fixture
        .get_json(
            &format!("/api/v1/voice-notes/{NOTE_OLD}/versions"),
            "alpha-secret",
        )
        .await;
    assert_eq!(versions["items"].as_array().expect("versions").len(), 1);

    let duplicate = fixture
        .json_request(
            "PUT",
            &format!("/api/v1/voice-notes/{NOTE_OLD}/tags"),
            "alpha-secret",
            Some(json!({"tag_ids": [TAG_ACTION, TAG_ACTION]})),
        )
        .await;
    assert_eq!(duplicate.status, StatusCode::BAD_REQUEST);
    assert_eq!(duplicate.body["error_code"], "validation_error");
    assert_eq!(duplicate.body["details"][0]["field"], "tag_ids");

    for tag_id in [TAG_MISSING, TAG_ACTION] {
        let bearer = if tag_id == TAG_MISSING {
            "alpha-secret"
        } else {
            "beta-secret"
        };
        let response = fixture
            .json_request(
                "PUT",
                &format!("/api/v1/voice-notes/{NOTE_OLD}/tags"),
                bearer,
                Some(json!({"tag_ids": [tag_id]})),
            )
            .await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
    }

    let malformed_tag_id = fixture
        .json_request(
            "PUT",
            &format!("/api/v1/voice-notes/{NOTE_OLD}/tags"),
            "alpha-secret",
            Some(json!({"tag_ids": ["not-a-ulid"]})),
        )
        .await;
    assert_eq!(malformed_tag_id.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed_tag_id.body["details"][0]["field"], "tag_ids");
}

#[tokio::test]
async fn delete_voice_note_hard_deletes_children_and_preserves_the_succeeded_job_replay_record() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "doomed note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
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
    fixture
        .insert_segment("alpha", NOTE_OLD, SEGMENT_FIRST, 0, "segment text")
        .await;
    fixture.insert_embedding("alpha", NOTE_OLD).await;
    fixture
        .insert_succeeded_job("alpha", JOB_ALPHA, NOTE_OLD)
        .await;

    let response = fixture
        .request(
            "DELETE",
            &format!("/api/v1/voice-notes/{NOTE_OLD}"),
            "alpha-secret",
            None,
        )
        .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        fixture
            .storage
            .get_voice_note("alpha", NOTE_OLD)
            .await
            .expect("voice note lookup")
            .is_none()
    );
    assert!(
        fixture
            .storage
            .list_voice_note_versions("alpha", NOTE_OLD, None, 10)
            .await
            .expect("versions")
            .is_empty()
    );
    assert!(
        fixture
            .storage
            .list_segments("alpha", NOTE_OLD)
            .await
            .expect("segments")
            .is_empty()
    );
    assert!(
        fixture
            .storage
            .get_current_embedding("alpha", NOTE_OLD)
            .await
            .expect("embedding lookup")
            .is_none()
    );
    assert!(
        fixture
            .storage
            .list_voice_note_tags("alpha", NOTE_OLD)
            .await
            .expect("tags")
            .is_empty()
    );
    let job = fixture
        .storage
        .get_job("alpha", JOB_ALPHA)
        .await
        .expect("job lookup")
        .expect("job survives");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.voice_note_id, None);

    let missing = fixture
        .request(
            "DELETE",
            &format!("/api/v1/voice-notes/{NOTE_OLD}"),
            "alpha-secret",
            None,
        )
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let malformed = fixture
        .request(
            "DELETE",
            "/api/v1/voice-notes/not-a-ulid",
            "alpha-secret",
            None,
        )
        .await;
    assert_validation_error(malformed, "voice_note_id").await;
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
async fn root_collection_filters_compose_by_tag_session_time_and_cursor() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "older tagged session note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: Some(SESSION_A),
            tags: vec![
                TagSeed {
                    id: TAG_MEETING,
                    name: "Meeting",
                    created_at: "2026-04-24T18:01:30.000000000Z",
                },
                TagSeed {
                    id: TAG_ACTION,
                    name: "Action",
                    created_at: "2026-04-24T18:01:40.000000000Z",
                },
            ],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_NEW,
            version_id: VERSION_NEW,
            text: "newer meeting note",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:00.000000000Z",
            session_id: Some(SESSION_BETA),
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OUTSIDE_SESSION,
            version_id: NOTE_OUTSIDE_SESSION,
            text: "newest action note",
            created_at: "2026-04-24T18:02:00.000000000Z",
            recorded_at: "2026-04-24T18:01:00.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_ACTION,
                name: "Action",
                created_at: "2026-04-24T18:01:40.000000000Z",
            }],
        })
        .await;

    assert_collection_ids(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?tag_id={TAG_MEETING}"),
                "alpha-secret",
            )
            .await,
        &[NOTE_NEW, NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?tag_id={TAG_MEETING}&tag_id={TAG_ACTION}"),
                "alpha-secret",
            )
            .await,
        &[NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?session_id={SESSION_A}"),
                "alpha-secret",
            )
            .await,
        &[NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?recorded_after=2026-04-24T18%3A00%3A00Z",
                "alpha-secret",
            )
            .await,
        &[NOTE_OUTSIDE_SESSION],
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?recorded_before=2026-04-24T18%3A00%3A00Z",
                "alpha-secret",
            )
            .await,
        &[NOTE_NEW, NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?created_after=2026-04-24T18%3A00%3A00Z",
                "alpha-secret",
            )
            .await,
        &[NOTE_OUTSIDE_SESSION, NOTE_NEW],
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?created_before=2026-04-24T18%3A01%3A00Z",
                "alpha-secret",
            )
            .await,
        &[NOTE_NEW, NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?created_after=2026-04-24T18%3A00%3A00Z&created_before=2026-04-24T18%3A01%3A00Z",
                "alpha-secret",
            )
            .await,
        &[NOTE_NEW],
    );
    assert_collection_ids(
        fixture
            .get_json(
                &format!(
                    "/api/v1/voice-notes?tag_id={TAG_ACTION}&recorded_before=2026-04-24T18%3A00%3A00Z"
                ),
                "alpha-secret",
            )
            .await,
        &[NOTE_OLD],
    );

    let first = fixture
        .get_json(
            &format!("/api/v1/voice-notes?tag_id={TAG_MEETING}&limit=1"),
            "alpha-secret",
        )
        .await;
    assert_collection_ids(first.clone(), &[NOTE_NEW]);
    let cursor = first["next_cursor"].as_str().expect("next cursor");
    let second = fixture
        .get_json(
            &format!("/api/v1/voice-notes?tag_id={TAG_MEETING}&limit=1&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_collection_ids(second.clone(), &[NOTE_OLD]);
    assert_eq!(second["next_cursor"], Value::Null);
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
async fn tag_filters_match_only_owned_existing_tags() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "alpha note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "beta",
            id: NOTE_OTHER_OWNER,
            version_id: NOTE_OTHER_OWNER,
            text: "beta tagged note",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:00.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;

    assert_empty_collection(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?tag_id={TAG_MEETING}"),
                "alpha-secret",
            )
            .await,
    );
    assert_empty_collection(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?tag_id={TAG_MISSING}"),
                "alpha-secret",
            )
            .await,
    );
}

#[tokio::test]
async fn session_collection_filters_apply_inside_path_session() {
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
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OUTSIDE_SESSION,
            version_id: VERSION_NEW,
            text: "outside session",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:00.000000000Z",
            session_id: None,
            tags: vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        })
        .await;

    assert_collection_ids(
        fixture
            .get_json(
                &format!("/api/v1/sessions/{SESSION_A}/voice-notes?tag_id={TAG_MEETING}"),
                "alpha-secret",
            )
            .await,
        &[NOTE_IN_SESSION],
    );
    assert_collection_ids(
        fixture
            .get_json(
                &format!(
                    "/api/v1/sessions/{SESSION_A}/voice-notes?recorded_after=2026-04-24T17%3A58%3A00Z&created_before=2026-04-24T18%3A00%3A00Z"
                ),
                "alpha-secret",
            )
            .await,
        &[NOTE_IN_SESSION],
    );
}

#[tokio::test]
async fn invalid_collection_query_values_return_validation_errors() {
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
async fn keyword_search_returns_parent_voice_notes_for_current_and_historical_version_matches() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "apollo retrospective",
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
            NOTE_PAGE_A,
            2,
            "renamed historical note",
            "2026-04-24T18:02:00.000000000Z",
        )
        .await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_NEW,
            version_id: VERSION_NEW,
            text: "apollo launch checklist",
            created_at: "2026-04-24T18:01:00.000000000Z",
            recorded_at: "2026-04-24T18:00:30.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    let body = fixture
        .get_json(
            "/api/v1/voice-notes?q=apollo&search_mode=keyword",
            "alpha-secret",
        )
        .await;

    assert_collection_ids(body.clone(), &[NOTE_OLD, NOTE_NEW]);
    assert_eq!(body["items"][0]["current_version_id"], NOTE_PAGE_A);
    assert_eq!(body["items"][0]["text"], "renamed historical note");
}

#[tokio::test]
async fn keyword_search_treats_fts_syntax_shaped_query_text_as_literal_terms() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "Rock AND OR NOT NEAR Roll",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    for q in [
        "AND",
        "OR",
        "NOT",
        "NEAR",
        "rock*",
        "^rock",
        "(rock)",
        "rock:roll",
        "Rock AND Roll",
        "AND OR *",
    ] {
        let body = fixture
            .get_json(&voice_note_search_path(q, "keyword"), "alpha-secret")
            .await;

        assert_collection_ids(body, &[NOTE_OLD]);
    }
}

#[tokio::test]
async fn semantic_search_orders_current_embeddings_by_cosine_similarity() {
    let openai_base_url = spawn_fake_embedding_server().await;
    let fixture = VoiceNoteFixture::new_with_openai_base_url(openai_base_url).await;
    for (id, version_id, text, created_at, vector) in [
        (
            NOTE_OLD,
            VERSION_OLD,
            "least similar",
            "2026-04-24T18:00:00.000000000Z",
            [0.0, 1.0, 0.0],
        ),
        (
            NOTE_PAGE_C,
            NOTE_PAGE_C,
            "middle similar",
            "2026-04-24T18:01:00.000000000Z",
            [0.5, 0.5, 0.0],
        ),
        (
            NOTE_NEW,
            VERSION_NEW,
            "most similar",
            "2026-04-24T18:02:00.000000000Z",
            [1.0, 0.0, 0.0],
        ),
    ] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id,
                text,
                created_at,
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id: None,
                tags: vec![],
            })
            .await;
        fixture.insert_embedding_vector("alpha", id, vector).await;
    }

    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?q=query&search_mode=semantic",
                "alpha-secret",
            )
            .await,
        &[NOTE_NEW, NOTE_PAGE_C, NOTE_OLD],
    );
}

#[tokio::test]
async fn semantic_search_returns_empty_without_provider_when_filtered_candidates_are_empty() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "unmatched note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    assert_empty_collection(
        fixture
            .get_json(
                &format!("/api/v1/voice-notes?q=query&search_mode=semantic&tag_id={TAG_MISSING}"),
                "alpha-secret",
            )
            .await,
    );
}

#[tokio::test]
async fn search_modes_gate_keyword_terms_and_semantic_query_text_independently() {
    let openai_base_url = spawn_fake_embedding_server().await;
    let fixture = VoiceNoteFixture::new_with_openai_base_url(openai_base_url).await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "plain searchable note",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;
    fixture
        .insert_embedding_vector("alpha", NOTE_OLD, [1.0, 0.0, 0.0])
        .await;

    assert_empty_collection(
        fixture
            .get_json(&voice_note_search_path("😀", "keyword"), "alpha-secret")
            .await,
    );
    assert_collection_ids(
        fixture
            .get_json(&voice_note_search_path("😀", "semantic"), "alpha-secret")
            .await,
        &[NOTE_OLD],
    );
    assert_collection_ids(
        fixture
            .get_json(&voice_note_search_path("😀", "hybrid"), "alpha-secret")
            .await,
        &[NOTE_OLD],
    );
    assert_empty_collection(
        fixture
            .get_json(&voice_note_search_path("   ", "semantic"), "alpha-secret")
            .await,
    );
}

#[tokio::test]
async fn hybrid_search_uses_reciprocal_rank_fusion_for_keyword_and_semantic_results() {
    let openai_base_url = spawn_fake_embedding_server().await;
    let fixture = VoiceNoteFixture::new_with_openai_base_url(openai_base_url).await;
    for (id, version_id, text, created_at, vector) in [
        (
            NOTE_OLD,
            VERSION_OLD,
            "apollo shared result",
            "2026-04-24T18:00:00.000000000Z",
            [1.0, 0.0, 0.0],
        ),
        (
            NOTE_NEW,
            VERSION_NEW,
            "apollo keyword only",
            "2026-04-24T18:01:00.000000000Z",
            [0.0, 1.0, 0.0],
        ),
        (
            NOTE_PAGE_C,
            NOTE_PAGE_C,
            "semantic only",
            "2026-04-24T18:02:00.000000000Z",
            [0.9, 0.1, 0.0],
        ),
    ] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id,
                text,
                created_at,
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id: None,
                tags: vec![],
            })
            .await;
        fixture.insert_embedding_vector("alpha", id, vector).await;
    }

    let body = fixture
        .get_json("/api/v1/voice-notes?q=apollo", "alpha-secret")
        .await;

    assert_eq!(body["items"][0]["id"], NOTE_OLD);
    assert_collection_ids(body, &[NOTE_OLD, NOTE_NEW, NOTE_PAGE_C]);
}

#[tokio::test]
async fn hybrid_search_keeps_keyword_results_without_provider_when_semantic_candidates_are_empty() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_voice_note(VoiceNoteSeed {
            owner: "alpha",
            id: NOTE_OLD,
            version_id: VERSION_OLD,
            text: "apollo keyword match",
            created_at: "2026-04-24T18:00:00.000000000Z",
            recorded_at: "2026-04-24T17:59:00.000000000Z",
            session_id: None,
            tags: vec![],
        })
        .await;

    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?q=apollo&search_mode=hybrid",
                "alpha-secret",
            )
            .await,
        &[NOTE_OLD],
    );
}

#[tokio::test]
async fn semantic_and_hybrid_search_skip_provider_when_large_candidate_set_has_no_embeddings() {
    let fixture = VoiceNoteFixture::new().await;
    fixture
        .insert_large_unembedded_search_candidate_set("alpha")
        .await;

    assert_empty_collection(
        fixture
            .get_json(
                "/api/v1/voice-notes?q=query&search_mode=semantic",
                "alpha-secret",
            )
            .await,
    );
    assert_collection_ids(
        fixture
            .get_json(
                "/api/v1/voice-notes?q=apollo&search_mode=hybrid",
                "alpha-secret",
            )
            .await,
        &["bulk-note-00000"],
    );
}

#[tokio::test]
async fn search_filters_and_cursors_apply_to_relevance_ordered_results() {
    let fixture = VoiceNoteFixture::new().await;
    fixture.insert_session("alpha", SESSION_A).await;
    for (id, version_id, text, created_at, session_id, tags) in [
        (
            NOTE_OLD,
            VERSION_OLD,
            "apollo exact",
            "2026-04-24T18:00:00.000000000Z",
            Some(SESSION_A),
            vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        ),
        (
            NOTE_NEW,
            VERSION_NEW,
            "apollo exact",
            "2026-04-24T18:01:00.000000000Z",
            Some(SESSION_A),
            vec![TagSeed {
                id: TAG_MEETING,
                name: "Meeting",
                created_at: "2026-04-24T18:01:30.000000000Z",
            }],
        ),
        (
            NOTE_OUTSIDE_SESSION,
            NOTE_OUTSIDE_SESSION,
            "apollo exact",
            "2026-04-24T18:02:00.000000000Z",
            None,
            vec![TagSeed {
                id: TAG_ACTION,
                name: "Action",
                created_at: "2026-04-24T18:01:40.000000000Z",
            }],
        ),
    ] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id,
                text,
                created_at,
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id,
                tags,
            })
            .await;
    }

    let first = fixture
        .get_json(
            &format!(
                "/api/v1/sessions/{SESSION_A}/voice-notes?q=apollo&search_mode=keyword&tag_id={TAG_MEETING}&limit=1"
            ),
            "alpha-secret",
        )
        .await;
    assert_collection_ids(first.clone(), &[NOTE_NEW]);
    let cursor = first["next_cursor"].as_str().expect("next cursor");
    let second = fixture
        .get_json(
            &format!(
                "/api/v1/sessions/{SESSION_A}/voice-notes?q=apollo&search_mode=keyword&tag_id={TAG_MEETING}&limit=1&cursor={cursor}"
            ),
            "alpha-secret",
        )
        .await;
    assert_collection_ids(second.clone(), &[NOTE_OLD]);
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn search_uses_distinct_cursors_and_blank_queries_return_empty_search_results() {
    let fixture = VoiceNoteFixture::new().await;
    for (id, version_id, created_at) in [
        (NOTE_OLD, VERSION_OLD, "2026-04-24T18:00:00.000000000Z"),
        (NOTE_NEW, VERSION_NEW, "2026-04-24T18:01:00.000000000Z"),
        (NOTE_PAGE_C, NOTE_PAGE_C, "2026-04-24T18:02:00.000000000Z"),
    ] {
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id,
                version_id,
                text: "apollo exact",
                created_at,
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id: None,
                tags: vec![],
            })
            .await;
    }

    assert_empty_collection(
        fixture
            .get_json(
                "/api/v1/voice-notes?q=...&search_mode=keyword",
                "alpha-secret",
            )
            .await,
    );

    let history = fixture
        .get_json("/api/v1/voice-notes?limit=1", "alpha-secret")
        .await;
    let history_cursor = history["next_cursor"].as_str().expect("history cursor");
    assert_validation_error(
        fixture
            .get(
                &format!(
                    "/api/v1/voice-notes?q=apollo&search_mode=keyword&cursor={history_cursor}"
                ),
                "alpha-secret",
            )
            .await,
        "cursor",
    )
    .await;

    let search = fixture
        .get_json(
            "/api/v1/voice-notes?q=apollo&search_mode=keyword&limit=1",
            "alpha-secret",
        )
        .await;
    let search_cursor = search["next_cursor"].as_str().expect("search cursor");
    assert_validation_error(
        fixture
            .get(
                &format!("/api/v1/voice-notes?limit=1&cursor={search_cursor}"),
                "alpha-secret",
            )
            .await,
        "cursor",
    )
    .await;
}

#[tokio::test]
async fn keyword_only_search_paginates_many_matches_in_relevance_order() {
    let fixture = VoiceNoteFixture::new().await;
    let mut inserted_ids = Vec::new();
    for index in 0..8 {
        let id = format!("01JS8D6E2S3T1J7H9J2Q2N4P{index:02}");
        let version_id = format!("01JS9P1D6CK9M0N1P2Q3R4S{index:02}");
        let created_at = format!("2026-04-24T18:00:0{index}.000000000Z");
        fixture
            .insert_voice_note(VoiceNoteSeed {
                owner: "alpha",
                id: &id,
                version_id: &version_id,
                text: "apollo exact",
                created_at: &created_at,
                recorded_at: "2026-04-24T17:59:00.000000000Z",
                session_id: None,
                tags: vec![],
            })
            .await;
        inserted_ids.push(id);
    }

    let first = fixture
        .get_json(
            "/api/v1/voice-notes?q=apollo&search_mode=keyword&limit=1",
            "alpha-secret",
        )
        .await;
    assert_collection_ids(first.clone(), &[inserted_ids[7].as_str()]);
    let cursor = first["next_cursor"].as_str().expect("next cursor");

    let second = fixture
        .get_json(
            &format!("/api/v1/voice-notes?q=apollo&search_mode=keyword&limit=1&cursor={cursor}"),
            "alpha-secret",
        )
        .await;
    assert_collection_ids(second, &[inserted_ids[6].as_str()]);
}

#[tokio::test]
async fn repeated_singular_query_parameters_return_repeated_parameter_errors() {
    let fixture = VoiceNoteFixture::new().await;

    for (path, field) in [
        ("/api/v1/voice-notes?cursor=a&cursor=b", "cursor"),
        ("/api/v1/voice-notes?limit=1&limit=2", "limit"),
        ("/api/v1/voice-notes?q=hello&q=again", "q"),
        (
            "/api/v1/voice-notes?q=hello&search_mode=keyword&search_mode=hybrid",
            "search_mode",
        ),
        (
            &format!("/api/v1/voice-notes?session_id={SESSION_A}&session_id={SESSION_BETA}"),
            "session_id",
        ),
        (
            "/api/v1/voice-notes?recorded_after=2026-04-24T18%3A00%3A00Z&recorded_after=2026-04-24T18%3A01%3A00Z",
            "recorded_after",
        ),
        (
            "/api/v1/voice-notes?recorded_before=2026-04-24T18%3A00%3A00Z&recorded_before=2026-04-24T18%3A01%3A00Z",
            "recorded_before",
        ),
        (
            "/api/v1/voice-notes?created_after=2026-04-24T18%3A00%3A00Z&created_after=2026-04-24T18%3A01%3A00Z",
            "created_after",
        ),
        (
            "/api/v1/voice-notes?created_before=2026-04-24T18%3A00%3A00Z&created_before=2026-04-24T18%3A01%3A00Z",
            "created_before",
        ),
    ] {
        let response = fixture.get(path, "alpha-secret").await;
        assert_repeated_singular_parameter_error(response, field).await;
    }

    let ignored = fixture
        .get(
            &format!("/api/v1/sessions/{SESSION_A}/voice-notes?session_id={SESSION_A}&session_id={SESSION_BETA}"),
            "alpha-secret",
        )
        .await;
    assert_eq!(ignored.status(), StatusCode::NOT_FOUND);
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

struct JsonResponse {
    status: StatusCode,
    body: Value,
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
        Self::new_with_openai_base_url("http://127.0.0.1".to_owned()).await
    }

    async fn new_with_openai_base_url(openai_base_url: String) -> Self {
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
            metrics: oracy_backend::metrics::Metrics::new(),
            operator_listen_addr: "127.0.0.1:9090".parse().expect("operator listen addr"),
            openai_api_key: "test-openai-key".to_owned(),
            openai_base_url,
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

    async fn insert_tag(&self, owner: &str, id: &str, name: &str, created_at: &str) {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO tags (id, api_key_id, name, name_folded, created_at)
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

    async fn insert_voice_note(&self, seed: VoiceNoteSeed<'_>) {
        if let Some(session_id) = seed.session_id {
            self.insert_session(seed.owner, session_id).await;
        }

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
        .bind(seed.id)
        .bind(seed.owner)
        .bind(seed.created_at)
        .bind(seed.recorded_at)
        .bind(seed.session_id)
        .execute(self.storage.pool())
        .await
        .expect("insert voice note");

        sqlx::query(
            r#"
            INSERT INTO voice_note_versions (
                id, api_key_id, voice_note_id, version_number, text, created_at
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
        .expect("insert voice note version");

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
                INSERT INTO voice_note_tags (api_key_id, voice_note_id, tag_id)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(seed.owner)
            .bind(seed.id)
            .bind(tag.id)
            .execute(self.storage.pool())
            .await
            .expect("insert voice note tag");
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
            INSERT INTO voice_note_versions (
                id, api_key_id, voice_note_id, version_number, text, created_at
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
        .expect("insert voice note version");
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
                id, api_key_id, voice_note_id, position, start_ms, end_ms, text
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

    async fn insert_large_unembedded_search_candidate_set(&self, owner: &str) {
        const CANDIDATE_COUNT: usize = 33_000;
        const INSERT_CHUNK_SIZE: usize = 2_500;

        for start in (0..CANDIDATE_COUNT).step_by(INSERT_CHUNK_SIZE) {
            let end = (start + INSERT_CHUNK_SIZE).min(CANDIDATE_COUNT);
            let mut query = QueryBuilder::<Sqlite>::new(
                r#"
                INSERT INTO voice_notes (
                    id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
                    language, model, processing_time_ms, cost_cents,
                    created_at, recorded_at, session_id
                )
                "#,
            );
            query.push_values(start..end, |mut row, index| {
                row.push_bind(format!("bulk-note-{index:05}"))
                    .push_bind(owner)
                    .push_bind(12.5_f64)
                    .push_bind("wav")
                    .push_bind(401_280_i64)
                    .push_bind("en")
                    .push_bind("gpt-4o-mini-transcribe")
                    .push_bind(1_843_i64)
                    .push_bind(Option::<i64>::None)
                    .push_bind("2026-04-24T18:00:00.000000000Z")
                    .push_bind("2026-04-24T17:59:00.000000000Z")
                    .push_bind(Option::<&str>::None);
            });
            query
                .build()
                .execute(self.storage.pool())
                .await
                .expect("insert large voice note candidate set");
        }

        for start in (0..CANDIDATE_COUNT).step_by(INSERT_CHUNK_SIZE) {
            let end = (start + INSERT_CHUNK_SIZE).min(CANDIDATE_COUNT);
            let mut query = QueryBuilder::<Sqlite>::new(
                r#"
                INSERT INTO voice_note_versions (
                    id, api_key_id, voice_note_id, version_number, text, created_at
                )
                "#,
            );
            query.push_values(start..end, |mut row, index| {
                let text = if index == 0 {
                    "apollo keyword match"
                } else {
                    "quiet note"
                };
                row.push_bind(format!("bulk-version-{index:05}"))
                    .push_bind(owner)
                    .push_bind(format!("bulk-note-{index:05}"))
                    .push_bind(1_i64)
                    .push_bind(text)
                    .push_bind("2026-04-24T18:00:00.000000000Z");
            });
            query
                .build()
                .execute(self.storage.pool())
                .await
                .expect("insert large voice note version candidate set");
        }
    }

    async fn insert_embedding(&self, owner: &str, voice_note_id: &str) {
        sqlx::query(
            r#"
            INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
            VALUES (?, ?, 'embedding-v1', x'010203', '2026-04-24T18:02:00.000000000Z')
            "#,
        )
        .bind(voice_note_id)
        .bind(owner)
        .execute(self.storage.pool())
        .await
        .expect("insert embedding");
    }

    async fn insert_embedding_vector(&self, owner: &str, voice_note_id: &str, prefix: [f32; 3]) {
        let mut vector = vec![0.0; 1536];
        vector[0] = prefix[0];
        vector[1] = prefix[1];
        vector[2] = prefix[2];
        sqlx::query(
            r#"
            INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
            VALUES (?, ?, 'embedding-v1', ?, '2026-04-24T18:02:00.000000000Z')
            "#,
        )
        .bind(voice_note_id)
        .bind(owner)
        .bind(encode_embedding_vector(&vector))
        .execute(self.storage.pool())
        .await
        .expect("insert embedding");
    }

    async fn insert_succeeded_job(&self, owner: &str, job_id: &str, voice_note_id: &str) {
        sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, api_key_id, idempotency_key, audio_sha256_hex,
                audio_content_hash_algorithm, recorded_at, accepted_audio_path,
                status, created_at, updated_at, retry_count, max_retries, voice_note_id
            )
            VALUES (
                ?, ?, ?, 'hash', 'sha256:chunk-sha256-v1',
                '2026-04-24T17:59:00.000000000Z', '/tmp/audio',
                'succeeded', '2026-04-24T18:00:00.000000000Z',
                '2026-04-24T18:00:00.000000000Z', 0, 3, ?
            )
            "#,
        )
        .bind(job_id)
        .bind(owner)
        .bind(format!("{job_id}-attempt"))
        .bind(voice_note_id)
        .execute(self.storage.pool())
        .await
        .expect("insert succeeded job");
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

async fn spawn_fake_embedding_server() -> String {
    let app = Router::new().route("/v1/embeddings", post(fake_embedding));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake embedding server");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake embedding server");
    });
    format!("http://{addr}")
}

async fn fake_embedding(Json(body): Json<Value>) -> Json<Value> {
    let input = body["input"].as_array().expect("input array");
    let data = input
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut embedding = vec![0.0; 1536];
            embedding[0] = 1.0;
            json!({
                "index": index,
                "embedding": embedding,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "model": "text-embedding-3-small",
        "data": data,
    }))
}

async fn assert_validation_error(response: axum::response::Response, field: &str) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error_code"], "validation_error");
    assert_eq!(body["details"][0]["field"], field);
}

async fn assert_repeated_singular_parameter_error(response: axum::response::Response, field: &str) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error_code"], "repeated_singular_parameter");
    assert_eq!(body["details"][0]["field"], field);
}

fn voice_note_search_path(q: &str, search_mode: &str) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("q", q)
        .append_pair("search_mode", search_mode)
        .finish();
    format!("/api/v1/voice-notes?{query}")
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

fn assert_collection_ids(body: Value, expected_ids: &[&str]) {
    assert_eq!(
        body["items"]
            .as_array()
            .expect("items array")
            .iter()
            .map(|item| item["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        expected_ids
    );
}
