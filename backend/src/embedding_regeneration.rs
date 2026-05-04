use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRegenerationRequest {
    pub api_key_id: String,
    pub voice_note_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmbeddingRegenerationTriggerError {
    #[error("{0}")]
    Failed(String),
}

pub trait EmbeddingRegenerationTrigger: Send + Sync {
    fn initiate(
        &self,
        request: EmbeddingRegenerationRequest,
    ) -> Result<(), EmbeddingRegenerationTriggerError>;
}

#[derive(Debug, Clone)]
pub struct NoopEmbeddingRegenerationTrigger;

impl EmbeddingRegenerationTrigger for NoopEmbeddingRegenerationTrigger {
    fn initiate(
        &self,
        _request: EmbeddingRegenerationRequest,
    ) -> Result<(), EmbeddingRegenerationTriggerError> {
        Ok(())
    }
}
