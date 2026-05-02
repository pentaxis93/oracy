use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tokio::process::Command;
use ulid::Ulid;

use crate::storage::{
    NewSegment, NewVoiceNote, NewVoiceNoteVersion, RetryOutcome, Storage, StorageError,
    TerminalJobFailure, TransientJobFailure, VoiceNoteMaterialization,
};

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub lease_duration: Duration,
    pub idle_sleep: std::time::Duration,
    pub error_sleep: std::time::Duration,
}

impl WorkerConfig {
    pub fn test() -> Self {
        Self {
            lease_duration: Duration::minutes(5),
            idle_sleep: std::time::Duration::from_millis(10),
            error_sleep: std::time::Duration::from_millis(10),
        }
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            lease_duration: Duration::minutes(5),
            idle_sleep: std::time::Duration::from_secs(5),
            error_sleep: std::time::Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOutcome {
    NoJob,
    Succeeded { job_id: String },
    RetryWaiting { job_id: String },
    Failed { job_id: String },
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("{0}")]
    Storage(#[from] StorageError),
    #[error("failed to read accepted audio metadata: {0}")]
    AudioMetadata(std::io::Error),
    #[error("duration probe failed: {0}")]
    DurationProbe(#[from] DurationProbeError),
    #[error("transcription engine failed: {0}")]
    Engine(#[from] EngineFailure),
}

#[derive(Debug, Clone)]
pub struct TranscriptionInput {
    pub audio_path: PathBuf,
    pub audio_format: String,
    pub language: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionOutput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineFailure {
    #[error("transient engine failure: {message}")]
    Transient {
        failure_code: String,
        message: String,
        retry_after_seconds: Option<i64>,
    },
    #[error("terminal engine failure: {failure_code}: {message}")]
    Terminal {
        failure_code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DurationProbeError {
    #[error("audio duration could not be derived")]
    InvalidAudio,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AudioSliceError {
    #[error("audio could not be sliced")]
    InvalidAudio,
    #[error("slicer failed: {0}")]
    Failed(String),
}

#[allow(async_fn_in_trait)]
pub trait TranscriptionEngine {
    async fn transcribe(
        &self,
        input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, EngineFailure>;
}

#[allow(async_fn_in_trait)]
pub trait AudioSlicer {
    async fn slices(
        &self,
        input: &Path,
        audio_format: &str,
    ) -> Result<Vec<PathBuf>, AudioSliceError>;
}

#[allow(async_fn_in_trait)]
pub trait DurationProbe {
    async fn duration_ms(
        &self,
        input: &Path,
        audio_format: &str,
    ) -> Result<i64, DurationProbeError>;
}

#[derive(Clone)]
pub struct OpenAiTranscriptionEngine<S> {
    base_url: String,
    api_key: String,
    slicer: S,
    client: reqwest::Client,
}

impl<S> OpenAiTranscriptionEngine<S> {
    pub fn new(base_url: String, api_key: String, slicer: S) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            slicer,
            client: reqwest::Client::new(),
        }
    }
}

impl<S> TranscriptionEngine for OpenAiTranscriptionEngine<S>
where
    S: AudioSlicer + Sync,
{
    async fn transcribe(
        &self,
        input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, EngineFailure> {
        let slices = self
            .slicer
            .slices(&input.audio_path, &input.audio_format)
            .await
            .map_err(|error| EngineFailure::Terminal {
                failure_code: "audio_invalid".to_owned(),
                message: error.to_string(),
            })?;
        let mut texts = Vec::with_capacity(slices.len());
        for slice in &slices {
            texts.push(self.transcribe_slice(&input, slice).await?);
        }
        cleanup_generated_slices(&input.audio_path, &slices).await;
        Ok(TranscriptionOutput {
            text: texts.join("\n"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct FfprobeDurationProbe;

impl DurationProbe for FfprobeDurationProbe {
    async fn duration_ms(
        &self,
        input: &Path,
        _audio_format: &str,
    ) -> Result<i64, DurationProbeError> {
        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=duration")
            .arg("-of")
            .arg("default=noprint_wrappers=1:nokey=1")
            .arg(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|_| DurationProbeError::InvalidAudio)?;
        if !output.status.success() {
            return Err(DurationProbeError::InvalidAudio);
        }
        let raw = String::from_utf8_lossy(&output.stdout);
        let seconds = raw
            .trim()
            .parse::<f64>()
            .map_err(|_| DurationProbeError::InvalidAudio)?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(DurationProbeError::InvalidAudio);
        }
        Ok((seconds * 1_000.0).round() as i64)
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegAudioSlicer {
    max_slice_bytes: u64,
}

impl FfmpegAudioSlicer {
    pub const OPENAI_MAX_AUDIO_BYTES: u64 = 26_214_400;

    pub fn openai_limit() -> Self {
        Self {
            max_slice_bytes: Self::OPENAI_MAX_AUDIO_BYTES,
        }
    }

    pub fn with_max_slice_bytes(max_slice_bytes: u64) -> Self {
        Self { max_slice_bytes }
    }
}

impl AudioSlicer for FfmpegAudioSlicer {
    async fn slices(
        &self,
        input: &Path,
        audio_format: &str,
    ) -> Result<Vec<PathBuf>, AudioSliceError> {
        let size = tokio::fs::metadata(input)
            .await
            .map_err(|error| AudioSliceError::Failed(error.to_string()))?
            .len();
        if size <= self.max_slice_bytes {
            return Ok(vec![input.to_path_buf()]);
        }

        let duration_ms = FfprobeDurationProbe
            .duration_ms(input, audio_format)
            .await
            .map_err(|_| AudioSliceError::InvalidAudio)?;
        let base_part_count = size.div_ceil(self.max_slice_bytes).max(1);
        let duration_seconds = duration_ms as f64 / 1_000.0;
        let parent = input.parent().ok_or_else(|| {
            AudioSliceError::Failed("accepted audio path has no parent directory".to_owned())
        })?;

        for multiplier in [1_u64, 2, 4, 8] {
            let part_count = base_part_count * multiplier;
            let part_seconds = duration_seconds / part_count as f64;
            let slice_dir = parent.join(format!(".oracy-slices-{}", Ulid::new()));
            tokio::fs::create_dir(&slice_dir)
                .await
                .map_err(|error| AudioSliceError::Failed(error.to_string()))?;

            let mut slices = Vec::new();
            for index in 0..part_count {
                let start = index as f64 * part_seconds;
                let length = if index == part_count - 1 {
                    (duration_seconds - start).max(0.001)
                } else {
                    part_seconds.max(0.001)
                };
                let output_path = slice_dir.join(format!("slice-{index}.{audio_format}"));
                let output = Command::new("ffmpeg")
                    .arg("-hide_banner")
                    .arg("-loglevel")
                    .arg("error")
                    .arg("-y")
                    .arg("-ss")
                    .arg(format!("{start:.3}"))
                    .arg("-t")
                    .arg(format!("{length:.3}"))
                    .arg("-i")
                    .arg(input)
                    .arg(&output_path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .map_err(|error| AudioSliceError::Failed(error.to_string()))?;
                if !output.status.success() {
                    cleanup_generated_slices(input, &slices).await;
                    let _ = tokio::fs::remove_dir(&slice_dir).await;
                    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    return Err(AudioSliceError::Failed(message));
                }
                slices.push(output_path);
            }

            let mut all_under_limit = true;
            for slice in &slices {
                let slice_size = tokio::fs::metadata(slice)
                    .await
                    .map_err(|error| AudioSliceError::Failed(error.to_string()))?
                    .len();
                if slice_size > self.max_slice_bytes {
                    all_under_limit = false;
                    break;
                }
            }
            if all_under_limit {
                return Ok(slices);
            }
            cleanup_generated_slices(input, &slices).await;
        }
        Err(AudioSliceError::Failed(
            "ffmpeg slices remained over the per-request byte limit".to_owned(),
        ))
    }
}

async fn cleanup_generated_slices(original: &Path, slices: &[PathBuf]) {
    let original = original.to_path_buf();
    let mut parents = HashSet::new();
    for slice in slices {
        if slice != &original {
            if let Some(parent) = slice.parent() {
                parents.insert(parent.to_path_buf());
            }
            let _ = tokio::fs::remove_file(slice).await;
        }
    }
    for parent in parents {
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

impl<S> OpenAiTranscriptionEngine<S> {
    async fn transcribe_slice(
        &self,
        input: &TranscriptionInput,
        slice: &Path,
    ) -> Result<String, EngineFailure> {
        let bytes = tokio::fs::read(slice)
            .await
            .map_err(|error| EngineFailure::Terminal {
                failure_code: "storage_error".to_owned(),
                message: format!("failed to read audio slice: {error}"),
            })?;
        let filename = slice
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("audio")
            .to_owned();
        let mut form = reqwest::multipart::Form::new()
            .text("model", input.model.clone())
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes).file_name(filename),
            );
        if let Some(language) = input.language.as_deref() {
            form = form.text("language", language.to_owned());
        }

        let response = self
            .client
            .post(format!("{}/v1/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|error| EngineFailure::Transient {
                failure_code: "engine_error".to_owned(),
                message: error.to_string(),
                retry_after_seconds: None,
            })?;

        if !response.status().is_success() {
            return Err(classify_openai_error(response).await);
        }

        let body = response
            .json::<OpenAiTranscriptionResponse>()
            .await
            .map_err(|error| EngineFailure::Transient {
                failure_code: "engine_error".to_owned(),
                message: format!("OpenAI transcription response was invalid: {error}"),
                retry_after_seconds: None,
            })?;
        Ok(body.text)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

async fn classify_openai_error(response: reqwest::Response) -> EngineFailure {
    let status = response.status();
    let retry_after_seconds = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let message = response
        .text()
        .await
        .unwrap_or_else(|_| format!("OpenAI transcription request failed with {status}"));

    match status {
        StatusCode::REQUEST_TIMEOUT => EngineFailure::Transient {
            failure_code: "engine_timeout".to_owned(),
            message,
            retry_after_seconds,
        },
        StatusCode::TOO_MANY_REQUESTS => EngineFailure::Transient {
            failure_code: "engine_rate_limited".to_owned(),
            message,
            retry_after_seconds,
        },
        status if status.is_server_error() => EngineFailure::Transient {
            failure_code: "engine_error".to_owned(),
            message,
            retry_after_seconds,
        },
        StatusCode::BAD_REQUEST | StatusCode::UNSUPPORTED_MEDIA_TYPE => EngineFailure::Terminal {
            failure_code: "audio_invalid".to_owned(),
            message,
        },
        _ => EngineFailure::Terminal {
            failure_code: "engine_error".to_owned(),
            message,
        },
    }
}

pub async fn process_one_available_job<E, D>(
    storage: &Storage,
    engine: &E,
    duration_probe: &D,
    config: WorkerConfig,
) -> Result<ProcessOutcome, WorkerError>
where
    E: TranscriptionEngine,
    D: DurationProbe,
{
    let now = OffsetDateTime::now_utc();
    let lease_token = Ulid::new().to_string();
    let Some(job) = storage
        .claim_next_transcription_job(&lease_token, now, now + config.lease_duration)
        .await?
    else {
        return Ok(ProcessOutcome::NoJob);
    };

    let duration_ms = match duration_probe
        .duration_ms(&job.accepted_audio_path, &job.audio_format)
        .await
    {
        Ok(duration_ms) => duration_ms,
        Err(error) => {
            storage
                .fail_leased_job(TerminalJobFailure {
                    api_key_id: job.api_key_id.clone(),
                    job_id: job.id.clone(),
                    lease_token: lease_token.clone(),
                    failure_code: "audio_invalid".to_owned(),
                    failure_message: error.to_string(),
                    retryable_by_client: false,
                    now: OffsetDateTime::now_utc(),
                })
                .await?;
            return Ok(ProcessOutcome::Failed { job_id: job.id });
        }
    };
    let transcription = match engine
        .transcribe(TranscriptionInput {
            audio_path: job.accepted_audio_path.clone(),
            audio_format: job.audio_format.clone(),
            language: job.language.clone(),
            model: job.transcription_model.clone(),
        })
        .await
    {
        Ok(transcription) => transcription,
        Err(EngineFailure::Transient {
            failure_code,
            message,
            retry_after_seconds,
        }) => {
            let now = OffsetDateTime::now_utc();
            let next_attempt_at = now + Duration::seconds(retry_after_seconds.unwrap_or(60).max(1));
            let outcome = storage
                .record_transient_job_failure(TransientJobFailure {
                    api_key_id: job.api_key_id.clone(),
                    job_id: job.id.clone(),
                    lease_token: lease_token.clone(),
                    failure_code,
                    failure_message: message,
                    now,
                    next_attempt_at,
                })
                .await?;
            return match outcome {
                RetryOutcome::RetryWaiting(job) => {
                    Ok(ProcessOutcome::RetryWaiting { job_id: job.id })
                }
                RetryOutcome::Failed(job) => Ok(ProcessOutcome::Failed { job_id: job.id }),
            };
        }
        Err(EngineFailure::Terminal {
            failure_code,
            message,
        }) => {
            storage
                .fail_leased_job(TerminalJobFailure {
                    api_key_id: job.api_key_id.clone(),
                    job_id: job.id.clone(),
                    lease_token: lease_token.clone(),
                    retryable_by_client: failure_code != "audio_invalid",
                    failure_code,
                    failure_message: message,
                    now: OffsetDateTime::now_utc(),
                })
                .await?;
            return Ok(ProcessOutcome::Failed { job_id: job.id });
        }
    };
    let audio_size_bytes = tokio::fs::metadata(&job.accepted_audio_path)
        .await
        .map_err(WorkerError::AudioMetadata)?
        .len() as i64;
    let created_at = OffsetDateTime::now_utc();
    let voice_note_id = Ulid::new().to_string();

    storage
        .complete_leased_job_with_voice_note(
            &job.api_key_id,
            &job.id,
            &lease_token,
            VoiceNoteMaterialization {
                voice_note: NewVoiceNote {
                    id: voice_note_id.clone(),
                    audio_duration_seconds: duration_ms as f64 / 1_000.0,
                    audio_format: job.audio_format.clone(),
                    audio_size_bytes,
                    language: job.language.clone(),
                    model: job.transcription_model.clone(),
                    processing_time_ms: (created_at - now).whole_milliseconds() as i64,
                    cost_cents: None,
                    created_at,
                    recorded_at: job.recorded_at,
                },
                initial_version: NewVoiceNoteVersion {
                    id: Ulid::new().to_string(),
                    text: transcription.text.clone(),
                    created_at,
                },
                segments: vec![NewSegment {
                    id: Ulid::new().to_string(),
                    position: 0,
                    start_ms: 0,
                    end_ms: duration_ms,
                    text: transcription.text,
                }],
                embedding: None,
            },
        )
        .await?;

    Ok(ProcessOutcome::Succeeded { job_id: job.id })
}

pub async fn run_worker_loop<E, D>(
    storage: Storage,
    engine: E,
    duration_probe: D,
    config: WorkerConfig,
) where
    E: TranscriptionEngine + Sync,
    D: DurationProbe + Sync,
{
    loop {
        match process_one_available_job(&storage, &engine, &duration_probe, config.clone()).await {
            Ok(ProcessOutcome::NoJob) => tokio::time::sleep(config.idle_sleep).await,
            Ok(_) => {}
            Err(error) => {
                tracing::error!("transcription worker iteration failed: {error}");
                tokio::time::sleep(config.error_sleep).await;
            }
        }
    }
}
