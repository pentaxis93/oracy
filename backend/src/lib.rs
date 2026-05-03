#![cfg(unix)]

pub mod abandonment_sweeper;
mod audio_durability;
pub mod audio_hash;
pub mod audio_store;
pub mod auth;
pub mod bootstrap;
pub mod collections;
pub mod config;
pub mod embedding_regeneration;
pub mod errors;
pub mod json;
pub mod metadata;
pub mod metrics;
pub mod retention_cleanup;
pub mod router;
pub mod settings;
pub mod state;
pub mod storage;
pub mod transcription_jobs;
pub mod transcription_worker;
pub mod voice_notes;
