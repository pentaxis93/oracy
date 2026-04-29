use axum::Json;
use axum::extract::State;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

use crate::auth::AuthenticatedKey;
use crate::errors::{ApiError, ErrorDetail};
use crate::json::JsonBody;
use crate::state::AppState;
use crate::storage::{SettingsPatch, SettingsRecord};

pub const DEFAULT_TRANSCRIPTION_MODEL: &str = "gpt-4o-mini-transcribe";
pub const SUPPORTED_TRANSCRIPTION_MODELS: [&str; 2] =
    ["gpt-4o-mini-transcribe", "gpt-4o-transcribe"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsResource {
    pub transcription_model: String,
}

pub async fn get_settings(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
) -> Result<Json<SettingsResource>, ApiError> {
    let settings = state
        .storage
        .get_settings(authenticated_key.api_key_id.as_str())
        .await
        .map_err(|_| ApiError::internal("Failed to load settings."))?;

    Ok(Json(SettingsResource::from(settings)))
}

pub async fn patch_settings(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    JsonBody(body): JsonBody<Value>,
) -> Result<Json<SettingsResource>, ApiError> {
    let patch = parse_patch(body)?;
    let settings = state
        .storage
        .update_settings(
            authenticated_key.api_key_id.as_str(),
            patch,
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(|_| ApiError::internal("Failed to update settings."))?;

    Ok(Json(SettingsResource::from(settings)))
}

impl From<SettingsRecord> for SettingsResource {
    fn from(settings: SettingsRecord) -> Self {
        Self {
            transcription_model: settings.transcription_model,
        }
    }
}

fn parse_patch(body: Value) -> Result<SettingsPatch, ApiError> {
    let Value::Object(fields) = body else {
        return Err(validation_error("", "Request body must be a JSON object."));
    };

    let mut patch = SettingsPatch {
        transcription_model: None,
    };

    for (field, value) in fields {
        match field.as_str() {
            "transcription_model" => {
                let Some(model) = value.as_str() else {
                    return Err(validation_error(
                        "transcription_model",
                        "Must be a supported transcription model identifier.",
                    ));
                };
                if !SUPPORTED_TRANSCRIPTION_MODELS.contains(&model) {
                    return Err(validation_error(
                        "transcription_model",
                        "Must be a supported transcription model identifier.",
                    ));
                }
                patch.transcription_model = Some(model.to_owned());
            }
            _ => {
                return Err(validation_error(&field, "Unknown settings field."));
            }
        }
    }

    Ok(patch)
}

fn validation_error(field: &str, message: &str) -> ApiError {
    ApiError::validation(
        "One or more request fields are invalid.",
        Some(vec![ErrorDetail {
            field: field.to_owned(),
            message: message.to_owned(),
        }]),
    )
}
