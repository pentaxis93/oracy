use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tracing::info;

use crate::auth::AuthStore;
use crate::config::Settings;
use crate::state::AppState;
use crate::storage::{Storage, StorageError};

pub const CONFIG_ENV_VAR: &str = "ORACY_CONFIG";

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("{CONFIG_ENV_VAR} is not set")]
    MissingConfigEnv,
    #[error("failed to read config file {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("accepted audio directory does not exist: {0}")]
    MissingAcceptedAudioDir(PathBuf),
    #[error("accepted audio path is not a directory: {0}")]
    AcceptedAudioPathNotDirectory(PathBuf),
    #[error("accepted audio directory is not writable: {path}: {source}")]
    AcceptedAudioDirNotWritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Storage(#[from] StorageError),
}

pub async fn load_runtime_from_env() -> Result<(std::net::SocketAddr, AppState), BootstrapError> {
    let config_path = std::env::var_os(CONFIG_ENV_VAR).ok_or(BootstrapError::MissingConfigEnv)?;
    load_runtime_from_path(Path::new(&config_path)).await
}

pub async fn load_runtime_from_path(
    config_path: &Path,
) -> Result<(std::net::SocketAddr, AppState), BootstrapError> {
    let raw = fs::read_to_string(config_path).map_err(|source| BootstrapError::ReadConfig {
        path: config_path.to_path_buf(),
        source,
    })?;
    let mut settings: Settings =
        toml::from_str(&raw).map_err(|source| BootstrapError::ParseConfig {
            path: config_path.to_path_buf(),
            source,
        })?;

    settings.accepted_audio_dir =
        resolve_config_relative_path(config_path, &settings.accepted_audio_dir)?;
    settings.database_path = resolve_config_relative_path(config_path, &settings.database_path)?;
    info!(
        "accepted audio storage directory resolved to {}",
        settings.accepted_audio_dir.display()
    );
    info!(
        "database path resolved to {}",
        settings.database_path.display()
    );

    validate_settings(&settings)?;
    ensure_writable_directory(&settings.accepted_audio_dir)?;
    let auth_store = AuthStore::try_from_configs(&settings.api_keys)
        .map_err(|error| BootstrapError::InvalidConfiguration(error.to_string()))?;
    let storage = Storage::connect(&settings.database_path).await?;

    let state = AppState {
        accepted_audio_dir: settings.accepted_audio_dir.clone(),
        auth_store: Arc::new(auth_store),
        storage,
    };

    Ok((settings.listen_addr, state))
}

fn resolve_config_relative_path(
    config_path: &Path,
    configured_path: &Path,
) -> Result<PathBuf, BootstrapError> {
    if configured_path.is_absolute() {
        return Ok(configured_path.to_path_buf());
    }

    let absolute_config_path = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| {
                BootstrapError::InvalidConfiguration(format!(
                    "failed to resolve config file directory for {}: {source}",
                    config_path.display()
                ))
            })?
            .join(config_path)
    };
    let canonical_config_path = fs::canonicalize(&absolute_config_path).map_err(|source| {
        BootstrapError::InvalidConfiguration(format!(
            "failed to resolve config file path for {}: {source}",
            config_path.display()
        ))
    })?;
    let config_dir = canonical_config_path.parent().ok_or_else(|| {
        BootstrapError::InvalidConfiguration(format!(
            "config file path has no parent directory: {}",
            config_path.display()
        ))
    })?;

    Ok(config_dir.join(configured_path))
}

fn validate_settings(settings: &Settings) -> Result<(), BootstrapError> {
    if settings.api_keys.is_empty() {
        return Err(BootstrapError::InvalidConfiguration(
            "at least one api_keys entry is required".to_owned(),
        ));
    }

    Ok(())
}

fn ensure_writable_directory(path: &Path) -> Result<(), BootstrapError> {
    if !path.exists() {
        return Err(BootstrapError::MissingAcceptedAudioDir(path.to_path_buf()));
    }

    if !path.is_dir() {
        return Err(BootstrapError::AcceptedAudioPathNotDirectory(
            path.to_path_buf(),
        ));
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let probe_path = path.join(format!(".oracy-write-probe-{unique}"));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
    {
        Ok(_) => {
            fs::remove_file(&probe_path).map_err(|source| {
                BootstrapError::AcceptedAudioDirNotWritable {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            Ok(())
        }
        Err(source) => Err(BootstrapError::AcceptedAudioDirNotWritable {
            path: path.to_path_buf(),
            source,
        }),
    }
}
