use axum::Router;
use axum::routing::get;

use crate::errors::ApiError;
use crate::settings::{get_settings, patch_settings};
use crate::state::AppState;
use crate::voice_notes::{
    get_voice_note, list_session_voice_notes, list_voice_note_segments, list_voice_note_versions,
    list_voice_notes,
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .route("/api/v1/voice-notes", get(list_voice_notes))
        .route("/api/v1/voice-notes/{voice_note_id}", get(get_voice_note))
        .route(
            "/api/v1/voice-notes/{voice_note_id}/versions",
            get(list_voice_note_versions),
        )
        .route(
            "/api/v1/voice-notes/{voice_note_id}/segments",
            get(list_voice_note_segments),
        )
        .route(
            "/api/v1/sessions/{session_id}/voice-notes",
            get(list_session_voice_notes),
        )
        .fallback(|| async { Err::<(), _>(ApiError::not_found("Resource not found.")) })
        .method_not_allowed_fallback(|| async { Err::<(), _>(ApiError::method_not_allowed()) })
        .with_state(state)
}
