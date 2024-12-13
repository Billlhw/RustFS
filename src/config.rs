use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Default)]
pub struct MasterConfig {
    pub host: String,
    pub port: u16,
    pub replication_factor: usize,
    pub max_allowed_chunks: usize,
    pub chunk_size: u64,
    pub metadata_path: String,
}

#[derive(Debug, Deserialize)]
pub struct ChunkServerConfig {
    pub host: String,
    pub port: u16,
    pub data_path: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub master: MasterConfig,
    pub chunkserver: Option<ChunkServerConfig>,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    // Read the config file
    let config_content = fs::read_to_string(path)?;
    // Parse the TOML content into the Config struct
    let config: Config = toml::from_str(&config_content)?;
    Ok(config)
}
