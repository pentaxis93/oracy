use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_OPENAI_EMBEDDING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const OPENAI_EMBEDDING_DIMENSIONS: usize = 1536;
const MAX_CHUNK_BYTES: usize = 6_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingInput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingOutput {
    pub model: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmbeddingFailure {
    #[error("transient embedding failure: {message}")]
    Transient {
        failure_code: String,
        message: String,
        retry_after_seconds: Option<i64>,
    },
    #[error("terminal embedding failure: {failure_code}: {message}")]
    Terminal {
        failure_code: String,
        message: String,
    },
}

#[allow(async_fn_in_trait)]
pub trait EmbeddingEngine {
    async fn embed(&self, input: EmbeddingInput) -> Result<EmbeddingOutput, EmbeddingFailure>;
}

#[derive(Clone)]
pub struct OpenAiEmbeddingEngine {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
    request_timeout: std::time::Duration,
}

impl OpenAiEmbeddingEngine {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self::with_request_timeout(base_url, api_key, DEFAULT_OPENAI_EMBEDDING_TIMEOUT)
    }

    pub fn with_request_timeout(
        base_url: String,
        api_key: String,
        request_timeout: std::time::Duration,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            client: reqwest::Client::new(),
            request_timeout,
        }
    }
}

impl EmbeddingEngine for OpenAiEmbeddingEngine {
    async fn embed(&self, input: EmbeddingInput) -> Result<EmbeddingOutput, EmbeddingFailure> {
        if input.text.trim().is_empty() {
            return Err(EmbeddingFailure::Terminal {
                failure_code: "audio_invalid".to_owned(),
                message: "canonical voice-note text is empty".to_owned(),
            });
        }

        let chunks = text_chunks(&input.text);
        let response = self
            .client
            .post(format!("{}/v1/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&OpenAiEmbeddingRequest {
                model: OPENAI_EMBEDDING_MODEL,
                input: &chunks,
            })
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(|error| {
                let failure_code = if error.is_timeout() {
                    "engine_timeout"
                } else {
                    "engine_error"
                };
                EmbeddingFailure::Transient {
                    failure_code: failure_code.to_owned(),
                    message: error.to_string(),
                    retry_after_seconds: None,
                }
            })?;

        if !response.status().is_success() {
            return Err(classify_openai_embedding_error(response).await);
        }

        let body = response
            .json::<OpenAiEmbeddingResponse>()
            .await
            .map_err(|error| EmbeddingFailure::Transient {
                failure_code: "engine_error".to_owned(),
                message: format!("OpenAI embedding response was invalid: {error}"),
                retry_after_seconds: None,
            })?;
        if body.data.len() != chunks.len() {
            return Err(EmbeddingFailure::Terminal {
                failure_code: "engine_error".to_owned(),
                message: "OpenAI embedding response returned the wrong number of vectors"
                    .to_owned(),
            });
        }

        let mut vectors = vec![Vec::new(); chunks.len()];
        for item in body.data {
            if item.embedding.len() != OPENAI_EMBEDDING_DIMENSIONS {
                return Err(EmbeddingFailure::Terminal {
                    failure_code: "engine_error".to_owned(),
                    message: "OpenAI embedding response returned an unexpected vector dimension"
                        .to_owned(),
                });
            }
            if item.index >= vectors.len() {
                return Err(EmbeddingFailure::Terminal {
                    failure_code: "engine_error".to_owned(),
                    message: "OpenAI embedding response returned an out-of-range vector index"
                        .to_owned(),
                });
            }
            vectors[item.index] = item.embedding;
        }
        if vectors.iter().any(Vec::is_empty) {
            return Err(EmbeddingFailure::Terminal {
                failure_code: "engine_error".to_owned(),
                message: "OpenAI embedding response omitted a vector".to_owned(),
            });
        }

        Ok(EmbeddingOutput {
            model: body.model,
            vector: pooled_normalized_vector(&vectors),
        })
    }
}

#[derive(Debug, Serialize)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    model: String,
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

async fn classify_openai_embedding_error(response: reqwest::Response) -> EmbeddingFailure {
    let status = response.status();
    let retry_after_seconds = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let message = response
        .text()
        .await
        .unwrap_or_else(|_| format!("OpenAI embedding request failed with {status}"));

    match status {
        StatusCode::REQUEST_TIMEOUT => EmbeddingFailure::Transient {
            failure_code: "engine_timeout".to_owned(),
            message,
            retry_after_seconds,
        },
        StatusCode::TOO_MANY_REQUESTS => EmbeddingFailure::Transient {
            failure_code: "engine_rate_limited".to_owned(),
            message,
            retry_after_seconds,
        },
        status if status.is_server_error() => EmbeddingFailure::Transient {
            failure_code: "engine_error".to_owned(),
            message,
            retry_after_seconds,
        },
        _ => EmbeddingFailure::Terminal {
            failure_code: "engine_error".to_owned(),
            message,
        },
    }
}

fn text_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if !current.is_empty() && current.len() + ch.len_utf8() > MAX_CHUNK_BYTES {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn pooled_normalized_vector(vectors: &[Vec<f32>]) -> Vec<f32> {
    let mut pooled = vec![0.0; OPENAI_EMBEDDING_DIMENSIONS];
    for vector in vectors {
        for (index, value) in vector.iter().enumerate() {
            pooled[index] += *value;
        }
    }
    let count = vectors.len() as f32;
    for value in &mut pooled {
        *value /= count;
    }
    let magnitude = pooled.iter().map(|value| value * value).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for value in &mut pooled {
            *value /= magnitude;
        }
    }
    pooled
}
