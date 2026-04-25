use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::FromRef;

use crate::auth::AuthStore;
use crate::storage::Storage;

#[derive(Clone, Debug)]
pub struct AppState {
    pub accepted_audio_dir: PathBuf,
    pub auth_store: Arc<AuthStore>,
    pub storage: Storage,
}

impl FromRef<AppState> for Arc<AuthStore> {
    fn from_ref(input: &AppState) -> Self {
        input.auth_store.clone()
    }
}
