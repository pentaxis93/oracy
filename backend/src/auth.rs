use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::HeaderValue;
use axum::http::request::Parts;
use sha2::{Digest, Sha256};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

use crate::config::ApiKeyConfig;
use crate::errors::ApiError;

#[derive(Clone)]
pub struct ApiKey(String);

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ApiKeyId(String);

#[derive(Clone, PartialEq, Eq)]
pub struct KeyValidationError {
    target: ValidationTarget,
    violation: KeyValidationViolation,
    subject: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuthStore {
    records: Vec<ConfiguredKey>,
}

#[derive(Clone, Debug)]
struct ConfiguredKey {
    id: ApiKeyId,
    digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedKey {
    pub api_key_id: ApiKeyId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValidationTarget {
    ApiKey,
    ApiKeyId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyValidationViolation {
    Blank,
    SurroundingWhitespace,
    NonVisibleAscii,
    InvalidHeaderValue,
    DuplicateApiKeyId,
    DuplicateApiKeyMaterial,
}

impl ApiKey {
    pub fn try_from_config(raw: &str) -> Result<Self, KeyValidationError> {
        validate_api_key(raw).map(|()| Self(raw.to_owned()))
    }

    pub fn try_from_presented_bearer(raw: &str) -> Result<Self, KeyValidationError> {
        validate_api_key(raw).map(|()| Self(raw.to_owned()))
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey(_)")
    }
}

impl ApiKeyId {
    pub fn try_from_config(raw: &str) -> Result<Self, KeyValidationError> {
        validate_api_key_id(raw).map(|()| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKeyId(_)")
    }
}

impl fmt::Display for ApiKeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Debug for KeyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyValidationError")
            .field("target", &self.target)
            .field("violation", &self.violation)
            .finish()
    }
}

impl fmt::Display for KeyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.target, self.violation) {
            (ValidationTarget::ApiKey, KeyValidationViolation::Blank) => {
                if let Some(api_key_id) = &self.subject {
                    write!(
                        formatter,
                        "api key material for api_key_id '{api_key_id}' must not be blank"
                    )
                } else {
                    formatter.write_str("api key material must not be blank")
                }
            }
            (ValidationTarget::ApiKey, KeyValidationViolation::SurroundingWhitespace) => {
                if let Some(api_key_id) = &self.subject {
                    write!(
                        formatter,
                        "api key material for api_key_id '{api_key_id}' has surrounding whitespace"
                    )
                } else {
                    formatter.write_str("api key material has surrounding whitespace")
                }
            }
            (ValidationTarget::ApiKey, KeyValidationViolation::NonVisibleAscii) => {
                if let Some(api_key_id) = &self.subject {
                    write!(
                        formatter,
                        "api key material for api_key_id '{api_key_id}' must contain only visible ASCII characters"
                    )
                } else {
                    formatter
                        .write_str("api key material must contain only visible ASCII characters")
                }
            }
            (ValidationTarget::ApiKey, KeyValidationViolation::InvalidHeaderValue) => {
                if let Some(api_key_id) = &self.subject {
                    write!(
                        formatter,
                        "api key material for api_key_id '{api_key_id}' must be usable as a Bearer authorization header value"
                    )
                } else {
                    formatter.write_str(
                        "api key material must be usable as a Bearer authorization header value",
                    )
                }
            }
            (ValidationTarget::ApiKey, KeyValidationViolation::DuplicateApiKeyMaterial) => {
                if let Some(api_key_id) = &self.subject {
                    write!(
                        formatter,
                        "duplicate api key material for api_key_id '{api_key_id}'"
                    )
                } else {
                    formatter.write_str("duplicate api key material")
                }
            }
            (ValidationTarget::ApiKey, KeyValidationViolation::DuplicateApiKeyId) => {
                unreachable!("duplicate api_key_id errors target ApiKeyId")
            }
            (ValidationTarget::ApiKeyId, KeyValidationViolation::Blank) => {
                formatter.write_str("api_key_id must not be blank")
            }
            (ValidationTarget::ApiKeyId, KeyValidationViolation::SurroundingWhitespace) => {
                let subject = self
                    .subject
                    .as_deref()
                    .expect("api_key_id whitespace errors carry the offending value");
                write!(
                    formatter,
                    "api_key_id '{subject}' has surrounding whitespace"
                )
            }
            (ValidationTarget::ApiKeyId, KeyValidationViolation::NonVisibleAscii) => {
                let subject = self
                    .subject
                    .as_deref()
                    .expect("api_key_id ascii errors carry the offending value");
                write!(
                    formatter,
                    "api_key_id '{subject}' must contain only visible ASCII characters"
                )
            }
            (ValidationTarget::ApiKeyId, KeyValidationViolation::DuplicateApiKeyId) => {
                let subject = self
                    .subject
                    .as_deref()
                    .expect("duplicate api_key_id errors carry the offending value");
                write!(formatter, "duplicate api_key_id: {subject}")
            }
            (ValidationTarget::ApiKeyId, KeyValidationViolation::InvalidHeaderValue)
            | (ValidationTarget::ApiKeyId, KeyValidationViolation::DuplicateApiKeyMaterial) => {
                unreachable!("api_key_id validation does not use these violations")
            }
        }
    }
}

impl std::error::Error for KeyValidationError {}

impl AuthStore {
    pub fn try_from_configs(configs: &[ApiKeyConfig]) -> Result<Self, KeyValidationError> {
        let mut records = Vec::with_capacity(configs.len());
        let mut seen_ids = HashSet::with_capacity(configs.len());
        let mut seen_digests = HashSet::with_capacity(configs.len());

        for config in configs {
            let id = ApiKeyId::try_from_config(&config.api_key_id)?;
            let key = ApiKey::try_from_config(&config.key)
                .map_err(|error| error.with_subject(id.as_str().to_owned()))?;
            let digest = key.digest();

            if !seen_ids.insert(id.clone()) {
                return Err(KeyValidationError::duplicate_api_key_id(id.as_str()));
            }

            if !seen_digests.insert(digest) {
                return Err(KeyValidationError::duplicate_api_key_material(id.as_str()));
            }

            records.push(ConfiguredKey { id, digest });
        }

        Ok(Self { records })
    }

    pub fn authenticate(&self, presented_key: &ApiKey) -> Option<AuthenticatedKey> {
        let candidate_digest = presented_key.digest();
        let mut matched = Choice::from(0);
        let mut matched_index = 0u64;

        // Keep this loop as a full scan. Returning early on a match would make
        // authentication time depend on key position and hit-vs-miss behavior.
        for (index, record) in self.records.iter().enumerate() {
            let is_match = record.digest.ct_eq(&candidate_digest);
            let is_first_match = is_match & !matched;
            matched_index.conditional_assign(&(index as u64), is_first_match);
            matched |= is_match;
        }

        bool::from(matched).then(|| AuthenticatedKey {
            api_key_id: self.records[matched_index as usize].id.clone(),
        })
    }
}

fn validate_api_key(raw: &str) -> Result<(), KeyValidationError> {
    validate_raw(raw, ValidationTarget::ApiKey, true, None)
}

fn validate_api_key_id(raw: &str) -> Result<(), KeyValidationError> {
    validate_raw(
        raw,
        ValidationTarget::ApiKeyId,
        false,
        key_validation_subject(raw),
    )
}

fn validate_raw(
    raw: &str,
    target: ValidationTarget,
    require_header_round_trip: bool,
    subject: Option<String>,
) -> Result<(), KeyValidationError> {
    if raw.trim().is_empty() {
        return Err(KeyValidationError::new(
            target,
            KeyValidationViolation::Blank,
            blank_subject(target, subject),
        ));
    }

    if raw != raw.trim() {
        return Err(KeyValidationError::new(
            target,
            KeyValidationViolation::SurroundingWhitespace,
            subject,
        ));
    }

    if !raw.bytes().all(|byte| (0x21..=0x7E).contains(&byte)) {
        return Err(KeyValidationError::new(
            target,
            KeyValidationViolation::NonVisibleAscii,
            subject,
        ));
    }

    if require_header_round_trip && HeaderValue::from_str(&format!("Bearer {raw}")).is_err() {
        return Err(KeyValidationError::new(
            target,
            KeyValidationViolation::InvalidHeaderValue,
            subject,
        ));
    }

    Ok(())
}

fn blank_subject(target: ValidationTarget, subject: Option<String>) -> Option<String> {
    match target {
        ValidationTarget::ApiKey => subject,
        ValidationTarget::ApiKeyId => None,
    }
}

fn key_validation_subject(raw: &str) -> Option<String> {
    (!raw.trim().is_empty()).then(|| raw.to_owned())
}

fn extract_bearer_credential(header_value: &str) -> Option<ApiKey> {
    let (scheme, remainder) = header_value.split_once(|char: char| char.is_ascii_whitespace())?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }

    ApiKey::try_from_presented_bearer(remainder.trim()).ok()
}

impl KeyValidationError {
    fn new(
        target: ValidationTarget,
        violation: KeyValidationViolation,
        subject: Option<String>,
    ) -> Self {
        Self {
            target,
            violation,
            subject,
        }
    }

    fn with_subject(mut self, subject: String) -> Self {
        self.subject = Some(subject);
        self
    }

    fn duplicate_api_key_id(api_key_id: &str) -> Self {
        Self::new(
            ValidationTarget::ApiKeyId,
            KeyValidationViolation::DuplicateApiKeyId,
            Some(api_key_id.to_owned()),
        )
    }

    fn duplicate_api_key_material(api_key_id: &str) -> Self {
        Self::new(
            ValidationTarget::ApiKey,
            KeyValidationViolation::DuplicateApiKeyMaterial,
            Some(api_key_id.to_owned()),
        )
    }
}

impl fmt::Debug for ValidationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey => formatter.write_str("ApiKey"),
            Self::ApiKeyId => formatter.write_str("ApiKeyId"),
        }
    }
}

impl fmt::Debug for KeyValidationViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => formatter.write_str("Blank"),
            Self::SurroundingWhitespace => formatter.write_str("SurroundingWhitespace"),
            Self::NonVisibleAscii => formatter.write_str("NonVisibleAscii"),
            Self::InvalidHeaderValue => formatter.write_str("InvalidHeaderValue"),
            Self::DuplicateApiKeyId => formatter.write_str("DuplicateApiKeyId"),
            Self::DuplicateApiKeyMaterial => formatter.write_str("DuplicateApiKeyMaterial"),
        }
    }
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

        let credential =
            extract_bearer_credential(header_value).ok_or_else(ApiError::unauthorized)?;

        auth_store
            .authenticate(&credential)
            .ok_or_else(ApiError::unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::ApiKey;

    #[test]
    fn try_from_config_and_try_from_presented_bearer_reject_same_inputs() {
        let cases = [
            ("", false),
            ("   ", false),
            (" secret", false),
            ("secret ", false),
            ("sëcret", false),
            ("bad\u{1}key", false),
            ("visible-ascii-secret", true),
        ];

        for (raw, expected_ok) in cases {
            let from_config = ApiKey::try_from_config(raw);
            let from_presented = ApiKey::try_from_presented_bearer(raw);

            assert_eq!(
                from_config.is_ok(),
                from_presented.is_ok(),
                "constructors diverged for {raw:?}"
            );
            assert_eq!(
                from_config.is_ok(),
                expected_ok,
                "unexpected constructor result for {raw:?}"
            );
        }
    }

    #[test]
    fn digest_is_stable_across_construction_paths() {
        let from_config = ApiKey::try_from_config("visible-ascii-secret").expect("valid key");
        let from_presented =
            ApiKey::try_from_presented_bearer("visible-ascii-secret").expect("valid key");

        assert_eq!(from_config.digest(), from_presented.digest());
    }

    #[test]
    fn try_from_presented_bearer_rejects_bytes_that_cannot_appear_in_headers() {
        for raw in ["sëcret", "bad\u{1}key", "line\nbreak"] {
            assert!(ApiKey::try_from_presented_bearer(raw).is_err(), "{raw:?}");
            let parsed = HeaderValue::from_bytes(format!("Bearer {raw}").as_bytes());
            assert!(
                parsed.is_err() || parsed.expect("header value").to_str().is_err(),
                "{raw:?}"
            );
        }

        assert!(ApiKey::try_from_presented_bearer("visible-ascii-secret").is_ok());
        assert!(HeaderValue::from_str("Bearer visible-ascii-secret").is_ok());
    }
}
