use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use serde::Deserialize;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use ulid::Ulid;

use crate::storage::{
    NewEmbedding, NewSegment, NewTranscript, NewTranscriptVersion, Storage, StorageError,
    TranscriptMaterialization,
};

const PROCESSING_LEASE: Duration = Duration::minutes(30);
const FIRST_RETRY_DELAY: Duration = Duration::seconds(30);

pub trait TranscriptionEngine {
    fn transcribe(
        &self,
        input: TranscriptionInput<'_>,
    ) -> Result<TranscriptionOutput, TranscriptionEngineError>;
}

pub struct TranscriptionInput<'a> {
    pub audio_path: &'a Path,
    pub model: &'a str,
    pub language: Option<&'a str>,
}

pub struct TranscriptionOutput {
    pub text: String,
    pub model: String,
    pub processing_time_ms: i64,
    pub cost_cents: Option<i64>,
}

pub struct OpenAiTranscriptionEngine {
    api_key: String,
    client: reqwest::blocking::Client,
}

#[derive(Debug, Error)]
pub enum TranscriptionEngineError {
    #[error("transient transcription engine failure")]
    Transient,
    #[error("terminal transcription engine failure")]
    Terminal,
}

impl OpenAiTranscriptionEngine {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl TranscriptionEngine for OpenAiTranscriptionEngine {
    fn transcribe(
        &self,
        input: TranscriptionInput<'_>,
    ) -> Result<TranscriptionOutput, TranscriptionEngineError> {
        let file = std::fs::File::open(input.audio_path)
            .map_err(|_| TranscriptionEngineError::Terminal)?;
        let file_part = reqwest::blocking::multipart::Part::reader(file)
            .file_name("audio")
            .mime_str("application/octet-stream")
            .map_err(|_| TranscriptionEngineError::Terminal)?;
        let mut form = reqwest::blocking::multipart::Form::new()
            .text("model", input.model.to_owned())
            .text("response_format", "json")
            .part("file", file_part);
        if let Some(language) = input.language {
            form = form.text("language", language.to_owned());
        }

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(|_| TranscriptionEngineError::Transient)?;
        let status = response.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TranscriptionEngineError::Transient);
        }
        if !status.is_success() {
            return Err(TranscriptionEngineError::Terminal);
        }
        let body: OpenAiTranscriptionResponse = response
            .json()
            .map_err(|_| TranscriptionEngineError::Terminal)?;

        Ok(TranscriptionOutput {
            text: body.text,
            model: input.model.to_owned(),
            processing_time_ms: 0,
            cost_cents: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

#[derive(Debug, Error)]
pub enum TranscriptionWorkerError {
    #[error("{0}")]
    Storage(#[from] StorageError),
    #[error("queued job has no resolved model: {0}")]
    MissingResolvedModel(String),
    #[error("queued job has no accepted audio path: {0}")]
    MissingAcceptedAudioPath(String),
    #[error("failed to inspect audio duration for {path}: {source}")]
    AudioDuration {
        path: PathBuf,
        #[source]
        source: AudioDurationError,
    },
    #[error("{0}")]
    Engine(#[from] TranscriptionEngineError),
}

#[derive(Debug, Error)]
pub enum AudioDurationError {
    #[error("audio file could not be opened: {0}")]
    Open(#[from] std::io::Error),
    #[error("audio format could not be probed: {0}")]
    Probe(#[from] symphonia::core::errors::Error),
    #[error("audio track is missing duration metadata")]
    MissingDuration,
}

pub async fn process_one_queued_job(
    storage: &Storage,
    engine: &impl TranscriptionEngine,
    now: OffsetDateTime,
) -> Result<bool, TranscriptionWorkerError> {
    let Some(job) = storage
        .claim_next_transcription_job(now, now + PROCESSING_LEASE)
        .await?
    else {
        return Ok(false);
    };

    let model = job
        .resolved_model
        .as_deref()
        .ok_or_else(|| TranscriptionWorkerError::MissingResolvedModel(job.id.clone()))?;
    if job.accepted_audio_path.as_os_str().is_empty() {
        return Err(TranscriptionWorkerError::MissingAcceptedAudioPath(job.id));
    }

    let duration = audio_duration_seconds(&job.accepted_audio_path).map_err(|source| {
        TranscriptionWorkerError::AudioDuration {
            path: job.accepted_audio_path.clone(),
            source,
        }
    })?;
    let audio_size_bytes = std::fs::metadata(&job.accepted_audio_path)
        .map_err(AudioDurationError::Open)
        .map_err(|source| TranscriptionWorkerError::AudioDuration {
            path: job.accepted_audio_path.clone(),
            source,
        })?
        .len() as i64;
    let output = match engine.transcribe(TranscriptionInput {
        audio_path: &job.accepted_audio_path,
        model,
        language: job.language.as_deref(),
    }) {
        Ok(output) => output,
        Err(TranscriptionEngineError::Transient) => {
            storage
                .record_transient_engine_failure(
                    job.api_key_id.as_str(),
                    job.id.as_str(),
                    now,
                    now + FIRST_RETRY_DELAY,
                )
                .await?;
            return Ok(true);
        }
        Err(TranscriptionEngineError::Terminal) => {
            storage
                .record_terminal_engine_failure(job.api_key_id.as_str(), job.id.as_str(), now)
                .await?;
            return Ok(true);
        }
    };

    let voice_note_id = Ulid::new().to_string();
    let version_id = Ulid::new().to_string();
    let segment_id = Ulid::new().to_string();
    let embedding_created_at = now;
    storage
        .complete_job_with_transcript(
            job.api_key_id.as_str(),
            job.id.as_str(),
            TranscriptMaterialization {
                transcript: NewTranscript {
                    id: voice_note_id.clone(),
                    audio_duration_seconds: duration,
                    audio_format: job.audio_format,
                    audio_size_bytes,
                    transcript_language: job.language,
                    model: output.model,
                    processing_time_ms: output.processing_time_ms,
                    cost_cents: output.cost_cents,
                    created_at: now,
                    recorded_at: job.recorded_at,
                },
                initial_version: NewTranscriptVersion {
                    id: version_id,
                    transcript: output.text.clone(),
                    created_at: now,
                },
                segments: vec![NewSegment {
                    id: segment_id,
                    position: 0,
                    start_ms: 0,
                    end_ms: (duration * 1000.0).round() as i64,
                    text: output.text,
                }],
                embedding: NewEmbedding {
                    model: "deferred-until-27".to_owned(),
                    vector: Vec::new(),
                    created_at: embedding_created_at,
                },
            },
        )
        .await?;

    Ok(true)
}

pub async fn run_transcription_worker(storage: Storage, engine: OpenAiTranscriptionEngine) {
    let poll_interval = StdDuration::from_secs(5);
    loop {
        match process_one_queued_job(&storage, &engine, OffsetDateTime::now_utc()).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(poll_interval).await,
            Err(error) => {
                tracing::error!("transcription worker failed: {error}");
                tokio::time::sleep(poll_interval).await;
            }
        }
    }
}

pub fn audio_duration_seconds(path: &Path) -> Result<f64, AudioDurationError> {
    let file = File::open(path)?;
    let media_source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        media_source,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let track = probed
        .format
        .default_track()
        .ok_or(AudioDurationError::MissingDuration)?;
    let time_base = track
        .codec_params
        .time_base
        .ok_or(AudioDurationError::MissingDuration)?;
    let frames = track
        .codec_params
        .n_frames
        .ok_or(AudioDurationError::MissingDuration)?;
    let duration = time_base.calc_time(frames);

    Ok(duration.seconds as f64 + duration.frac)
}
