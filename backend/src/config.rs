use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

fn default_listen_addr() -> SocketAddr {
    "127.0.0.1:8080"
        .parse()
        .expect("default listen addr is valid")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: SocketAddr,
    pub accepted_audio_dir: PathBuf,
    pub database_path: PathBuf,
    pub api_keys: Vec<ApiKeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiKeyConfig {
    pub api_key_id: String,
    pub key: String,
}
