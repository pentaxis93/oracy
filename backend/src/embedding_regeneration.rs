use thiserror::Error;
use time::{Duration, OffsetDateTime};
use ulid::Ulid;

use crate::embedding::{EmbeddingEngine, EmbeddingFailure, EmbeddingInput};
use crate::storage::{
    EmbeddingRegenerationFailure, EmbeddingRegenerationRetryOutcome, NewEmbedding, Storage,
    StorageError, encode_embedding_vector,
};

#[derive(Debug, Clone)]
pub struct EmbeddingRegenerationConfig {
    pub lease_duration: Duration,
    pub idle_sleep: std::time::Duration,
    pub error_sleep: std::time::Duration,
}

impl EmbeddingRegenerationConfig {
    pub fn test() -> Self {
        Self {
            lease_duration: Duration::minutes(5),
            idle_sleep: std::time::Duration::from_millis(10),
            error_sleep: std::time::Duration::from_millis(10),
        }
    }
}

impl Default for EmbeddingRegenerationConfig {
    fn default() -> Self {
        Self {
            lease_duration: Duration::minutes(5),
            idle_sleep: std::time::Duration::from_secs(5),
            error_sleep: std::time::Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingRegenerationOutcome {
    NoJob,
    Replaced { voice_note_id: String },
    Stale { voice_note_id: String },
    RetryWaiting { voice_note_id: String },
    Failed { voice_note_id: String },
}

#[derive(Debug, Error)]
pub enum EmbeddingRegenerationWorkerError {
    #[error("{0}")]
    Storage(#[from] StorageError),
}

pub async fn process_one_embedding_regeneration_job<E>(
    storage: &Storage,
    engine: &E,
    config: EmbeddingRegenerationConfig,
) -> Result<EmbeddingRegenerationOutcome, EmbeddingRegenerationWorkerError>
where
    E: EmbeddingEngine,
{
    let now = OffsetDateTime::now_utc();
    let lease_token = Ulid::new().to_string();
    let Some(job) = storage
        .claim_next_embedding_regeneration_job(&lease_token, now, now + config.lease_duration)
        .await?
    else {
        return Ok(EmbeddingRegenerationOutcome::NoJob);
    };

    if job.text.trim().is_empty() {
        storage
            .fail_embedding_regeneration_job(EmbeddingRegenerationFailure {
                api_key_id: job.api_key_id,
                voice_note_id: job.voice_note_id.clone(),
                lease_token,
                failure_code: "engine_error".to_owned(),
                failure_message: "canonical voice-note text is empty".to_owned(),
                now: OffsetDateTime::now_utc(),
            })
            .await?;
        return Ok(EmbeddingRegenerationOutcome::Failed {
            voice_note_id: job.voice_note_id,
        });
    }

    let embedding = match engine.embed(EmbeddingInput { text: job.text }).await {
        Ok(embedding) => embedding,
        Err(EmbeddingFailure::Transient {
            failure_code,
            message,
            retry_after_seconds,
        }) => {
            let now = OffsetDateTime::now_utc();
            let next_attempt_at = now + Duration::seconds(retry_after_seconds.unwrap_or(60).max(1));
            let outcome = storage
                .record_transient_embedding_regeneration_failure(
                    EmbeddingRegenerationFailure {
                        api_key_id: job.api_key_id,
                        voice_note_id: job.voice_note_id.clone(),
                        lease_token,
                        failure_code,
                        failure_message: message,
                        now,
                    },
                    next_attempt_at,
                )
                .await?;
            return Ok(match outcome {
                EmbeddingRegenerationRetryOutcome::RetryWaiting => {
                    EmbeddingRegenerationOutcome::RetryWaiting {
                        voice_note_id: job.voice_note_id,
                    }
                }
                EmbeddingRegenerationRetryOutcome::Failed => EmbeddingRegenerationOutcome::Failed {
                    voice_note_id: job.voice_note_id,
                },
            });
        }
        Err(EmbeddingFailure::Terminal {
            failure_code,
            message,
        }) => {
            storage
                .fail_embedding_regeneration_job(EmbeddingRegenerationFailure {
                    api_key_id: job.api_key_id,
                    voice_note_id: job.voice_note_id.clone(),
                    lease_token,
                    failure_code,
                    failure_message: message,
                    now: OffsetDateTime::now_utc(),
                })
                .await?;
            return Ok(EmbeddingRegenerationOutcome::Failed {
                voice_note_id: job.voice_note_id,
            });
        }
    };

    let replaced = storage
        .complete_embedding_regeneration_job_if_current(
            &job.api_key_id,
            &job.voice_note_id,
            &job.voice_note_version_id,
            &lease_token,
            NewEmbedding {
                model: embedding.model,
                vector: encode_embedding_vector(&embedding.vector),
                created_at: OffsetDateTime::now_utc(),
            },
        )
        .await?;

    Ok(if replaced {
        EmbeddingRegenerationOutcome::Replaced {
            voice_note_id: job.voice_note_id,
        }
    } else {
        EmbeddingRegenerationOutcome::Stale {
            voice_note_id: job.voice_note_id,
        }
    })
}

pub async fn run_embedding_regeneration_loop<E>(
    storage: Storage,
    engine: E,
    config: EmbeddingRegenerationConfig,
) where
    E: EmbeddingEngine + Sync,
{
    loop {
        match process_one_embedding_regeneration_job(&storage, &engine, config.clone()).await {
            Ok(EmbeddingRegenerationOutcome::NoJob) => {
                tokio::time::sleep(config.idle_sleep).await;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!("embedding regeneration worker iteration failed: {error}");
                tokio::time::sleep(config.error_sleep).await;
            }
        }
    }
}
