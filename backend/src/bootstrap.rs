use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::auth::AuthStore;
use crate::config::Settings;
use crate::state::AppState;

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
}

pub fn load_runtime_from_env() -> Result<(std::net::SocketAddr, AppState), BootstrapError> {
    let config_path = std::env::var_os(CONFIG_ENV_VAR).ok_or(BootstrapError::MissingConfigEnv)?;
    load_runtime_from_path(Path::new(&config_path))
}

pub fn load_runtime_from_path(
    config_path: &Path,
) -> Result<(std::net::SocketAddr, AppState), BootstrapError> {
    let raw = fs::read_to_string(config_path).map_err(|source| BootstrapError::ReadConfig {
        path: config_path.to_path_buf(),
        source,
    })?;
    let settings: Settings =
        toml::from_str(&raw).map_err(|source| BootstrapError::ParseConfig {
            path: config_path.to_path_buf(),
            source,
        })?;

    validate_settings(&settings)?;
    ensure_writable_directory(&settings.accepted_audio_dir)?;

    let state = AppState {
        accepted_audio_dir: settings.accepted_audio_dir.clone(),
        auth_store: Arc::new(AuthStore::from_configs(&settings.api_keys)),
    };

    Ok((settings.listen_addr, state))
}

fn validate_settings(settings: &Settings) -> Result<(), BootstrapError> {
    if settings.api_keys.is_empty() {
        return Err(BootstrapError::InvalidConfiguration(
            "at least one api_keys entry is required".to_owned(),
        ));
    }

    let mut seen_ids = HashSet::new();
    let mut seen_keys = HashSet::new();

    for key in &settings.api_keys {
        if key.api_key_id.trim().is_empty() {
            return Err(BootstrapError::InvalidConfiguration(
                "api_key_id must not be blank".to_owned(),
            ));
        }

        if key.api_key_id != key.api_key_id.trim() {
            return Err(BootstrapError::InvalidConfiguration(format!(
                "api_key_id '{}' has surrounding whitespace",
                key.api_key_id
            )));
        }

        if key.key.trim().is_empty() {
            return Err(BootstrapError::InvalidConfiguration(
                "api key material must not be blank".to_owned(),
            ));
        }

        if key.key != key.key.trim() {
            return Err(BootstrapError::InvalidConfiguration(format!(
                "api key material for api_key_id '{}' has surrounding whitespace",
                key.api_key_id
            )));
        }

        if !seen_ids.insert(key.api_key_id.clone()) {
            return Err(BootstrapError::InvalidConfiguration(format!(
                "duplicate api_key_id: {}",
                key.api_key_id
            )));
        }

        if !seen_keys.insert(key.key.clone()) {
            return Err(BootstrapError::InvalidConfiguration(
                "duplicate api key material".to_owned(),
            ));
        }
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
