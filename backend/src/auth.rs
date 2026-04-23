use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use sha2::{Digest, Sha256};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

use crate::config::ApiKeyConfig;
use crate::errors::ApiError;

#[derive(Debug, Clone)]
pub struct AuthStore {
    records: Vec<ApiKeyRecord>,
}

#[derive(Debug, Clone)]
struct ApiKeyRecord {
    api_key_id: String,
    key_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedKey {
    pub api_key_id: String,
}

impl AuthStore {
    pub fn from_configs(configs: &[ApiKeyConfig]) -> Self {
        let records = configs
            .iter()
            .map(|config| ApiKeyRecord {
                api_key_id: config.api_key_id.clone(),
                key_digest: digest_key(&config.key),
            })
            .collect();

        Self { records }
    }

    pub fn authenticate(&self, presented_key: &str) -> Option<AuthenticatedKey> {
        let candidate_digest = digest_key(presented_key);
        let mut matched = Choice::from(0);
        let mut matched_index = 0u64;

        // Keep this loop as a full scan. Returning early on a match would make
        // authentication time depend on key position and hit-vs-miss behavior.
        for (index, record) in self.records.iter().enumerate() {
            let is_match = record.key_digest.ct_eq(&candidate_digest);
            let is_first_match = is_match & !matched;
            matched_index.conditional_assign(&(index as u64), is_first_match);
            matched |= is_match;
        }

        bool::from(matched).then(|| AuthenticatedKey {
            api_key_id: self.records[matched_index as usize].api_key_id.clone(),
        })
    }
}

fn digest_key(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

impl<S> FromRequestParts<S> for AuthenticatedKey
where
    S: Send + Sync,
    Arc<AuthStore>: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_store = Arc::<AuthStore>::from_ref(state);
        let header_value = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(ApiError::unauthorized)?;

        let bearer = header_value
            .strip_prefix("Bearer ")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(ApiError::unauthorized)?;

        auth_store
            .authenticate(bearer)
            .ok_or_else(ApiError::unauthorized)
    }
}
