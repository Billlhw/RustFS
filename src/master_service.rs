use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

use crate::config::{CommonConfig, MasterConfig};
use crate::proto::master;

// Import the Master service and messages
use master::ChunkInfo;

#[derive(Debug, Default)]
pub struct MasterService {
    pub file_chunks: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // File -> List of ChunkInfo
    pub chunk_servers: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // ChunkServer -> List of chunks
    pub last_heartbeat_time: Arc<RwLock<HashMap<String, u64>>>, // ChunkServer -> Last heartbeat timestamp
    pub chunk_map: Arc<RwLock<HashMap<String, ChunkInfo>>>,     // chunkID -> ChunkInfo
    pub config: MasterConfig,
    pub common_config: CommonConfig,
    pub addr: String, // TODO: use it in heartbeat to the master node
}

// Implement a constructor for MasterService
impl MasterService {
    pub fn new(addr: &str, config: MasterConfig, common_config: CommonConfig) -> Self {
        Self {
            file_chunks: Arc::new(RwLock::new(HashMap::new())),
            chunk_servers: Arc::new(RwLock::new(HashMap::new())),
            last_heartbeat_time: Arc::new(RwLock::new(HashMap::new())),
            chunk_map: Arc::new(RwLock::new(HashMap::new())), // Initialize the new map
            addr: addr.to_string(),
            config, // Store the configuration, field init shorthand
            common_config,
        }
    }
}
