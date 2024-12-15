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
    // Starts a periodic task to check for failed chunk servers and reassign their chunks.
    pub async fn start_heartbeat_checker(&self) {
        let interval = self.config.cron_interval;
        let heartbeat_failure_threshold = self.config.heartbeat_failure_threshold;
        let heartbeat_interval = self.common_config.heartbeat_interval;

        let file_chunks = Arc::clone(&self.file_chunks);
        let chunk_servers = Arc::clone(&self.chunk_servers);
        let last_heartbeat_time = Arc::clone(&self.last_heartbeat_time);
        let chunk_map = Arc::clone(&self.chunk_map);
        let common_config = self.common_config.clone();
        let max_allowed_chunks = self.common_config.max_allowed_chunks;
        // Periodic heartbeat check task implementation
        tokio::spawn(async move {
            let mut ticker = time::interval(Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                // Check for failed servers
                let mut failed_servers = Vec::new();
                {
                    let last_heartbeat = last_heartbeat_time.read().await;
                    for (server, &last_time) in last_heartbeat.iter() {
                        if now - last_time > heartbeat_failure_threshold * heartbeat_interval {
                            failed_servers.push(server.clone());
                        }
                    }
                }

                if failed_servers.is_empty() {
                    continue;
                }

                println!("[Cron Task] Failed servers detected: {:?}", failed_servers);
            }
        });
    }
}
