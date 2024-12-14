use serde::Deserialize;
use std::fs;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct MasterConfig {
    pub log_path: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct ChunkServerConfig {
    pub chunk_size: u64,
    pub data_path: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct ClientConfig {
    pub log_path: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct CommonConfig {
    pub master_addrs: Vec<String>, // List of master addresses
    pub heartbeat_interval: u64,   // Heartbeat interval in seconds
    pub chunk_size: u64,           // Chunk size in bytes
    pub max_allowed_chunks: usize, // Maximum number of chunks per chunkserver
    pub replication_factor: usize, // Number of chunk replicas
    pub log_level: String,         // Log level (e.g., "debug", "info", etc.)
    pub log_output: String,        // Log output (e.g., "stdout", "file", etc.)
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub master: MasterConfig,
    pub chunkserver: ChunkServerConfig,
    pub client: ClientConfig,
    pub common: CommonConfig,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    // Read the config file
    let config_content = fs::read_to_string(path)?;
    // Parse the TOML content into the Config struct
    let config: Config = toml::from_str(&config_content)?;
    Ok(config)
}
