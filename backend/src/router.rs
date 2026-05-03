use axum::Router;
use axum::routing::get;

use crate::errors::ApiError;
use crate::metadata::{
    create_session, create_tag, delete_session, delete_tag, get_session, get_tag, list_sessions,
    list_tags, patch_session, patch_tag,
};
use crate::metrics::prometheus_metrics;
use crate::settings::{get_settings, patch_settings};
use crate::state::AppState;
use crate::transcription_jobs;
use crate::voice_notes::{
    delete_voice_note, get_voice_note, list_session_voice_notes, list_voice_note_segments,
    list_voice_note_versions, list_voice_notes, patch_voice_note, put_voice_note_tags,
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .nest("/api/v1/transcription-jobs", transcription_jobs::routes())
        .route("/api/v1/tags", get(list_tags).post(create_tag))
        .route(
            "/api/v1/tags/{tag_id}",
            get(get_tag).patch(patch_tag).delete(delete_tag),
        )
        .route("/api/v1/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/v1/sessions/{session_id}",
            get(get_session).patch(patch_session).delete(delete_session),
        )
        .route("/api/v1/voice-notes", get(list_voice_notes))
        .route(
            "/api/v1/voice-notes/{voice_note_id}",
            get(get_voice_note)
                .patch(patch_voice_note)
                .delete(delete_voice_note),
        )
        .route(
            "/api/v1/voice-notes/{voice_note_id}/versions",
            get(list_voice_note_versions),
        )
        .route(
            "/api/v1/voice-notes/{voice_note_id}/segments",
            get(list_voice_note_segments),
        )
        .route(
            "/api/v1/voice-notes/{voice_note_id}/tags",
            axum::routing::put(put_voice_note_tags),
        )
        .route(
            "/api/v1/sessions/{session_id}/voice-notes",
            get(list_session_voice_notes),
        )
        .fallback(|| async { Err::<(), _>(ApiError::not_found("Resource not found.")) })
        .method_not_allowed_fallback(|| async { Err::<(), _>(ApiError::method_not_allowed()) })
        .with_state(state)
}

pub fn build_operator_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(prometheus_metrics))
        .with_state(state)
}
