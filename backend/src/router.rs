use axum::Router;

use crate::errors::ApiError;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .fallback(|| async { Err::<(), _>(ApiError::not_found("Resource not found.")) })
        .with_state(state)
}
