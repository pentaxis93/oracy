use std::path::PathBuf;
use std::sync::Arc;
use std::{fmt, fmt::Debug};

use axum::extract::FromRef;

use crate::auth::AuthStore;
use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub accepted_audio_dir: PathBuf,
    pub auth_store: Arc<AuthStore>,
    pub openai_api_key: String,
    pub storage: Storage,
}

impl Debug for AppState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("accepted_audio_dir", &self.accepted_audio_dir)
            .field("auth_store", &self.auth_store)
            .field("openai_api_key", &"<redacted>")
            .field("storage", &self.storage)
            .finish()
    }
}

impl FromRef<AppState> for Arc<AuthStore> {
    fn from_ref(input: &AppState) -> Self {
        input.auth_store.clone()
    }
}
