use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub r2: R2Config,
    pub video: VideoConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub secret_key: String,
    pub admin_password: String,
    pub max_concurrent_encodes: usize,
    pub max_concurrent_uploads: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct R2Config {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub public_base_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VideoConfig {
    pub encoder: String,
}

impl Config {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let content = fs::read_to_string(path)
            .await
            .context("Failed to read config file")?;
        let config: Config =
            serde_yaml::from_str(&content).context("Failed to parse config file")?;
        Ok(config)
    }
}
