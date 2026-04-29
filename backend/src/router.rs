use axum::Router;
use axum::routing::get;

use crate::errors::ApiError;
use crate::settings::{get_settings, patch_settings};
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .fallback(|| async { Err::<(), _>(ApiError::not_found("Resource not found.")) })
        .method_not_allowed_fallback(|| async { Err::<(), _>(ApiError::method_not_allowed()) })
        .with_state(state)
}
