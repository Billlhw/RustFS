// Implements the internal logic and utilities of the MasterService struct
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

use crate::config::{CommonConfig, MasterConfig};
use crate::proto::master;

// Import the Master service and messages
use crate::proto::chunk::chunk_client::ChunkClient;
use crate::proto::chunk::SendChunkRequest;
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

    pub fn is_leader(&self) -> bool {
        self.addr == self.config.master_address
    }

    /// Starts a periodic task to check for failed chunk servers and reassign their chunks.
    pub async fn start_heartbeat_checker(&self) {
        let interval = self.config.cron_interval; // Interval for the periodic task
        let heartbeat_failure_threshold = self.config.heartbeat_failure_threshold; // Threashold for determining server failure, in number of heartbeat_intervals
        let heartbeat_interval = self.common_config.heartbeat_interval; // Interval for chunkserver heartbeats

        let file_chunks = Arc::clone(&self.file_chunks); // File -> List of ChunkInfo
        let chunk_servers = Arc::clone(&self.chunk_servers); // ChunkServer -> List of chunks
        let last_heartbeat_time = Arc::clone(&self.last_heartbeat_time); // ChunkServer -> Last heartbeat timestamp
        let chunk_map = Arc::clone(&self.chunk_map); // chunkID -> ChunkInfo
        let common_config = self.common_config.clone();
        let max_allowed_chunks = self.common_config.max_allowed_chunks;

        // Spawn an asynchronous task
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

                // Handle reassigning chunks for each failed server
                for failed_server in failed_servers.clone() {
                    // Collect chunks from the failed server (chunks_to_reassign is a list of all removed servers)
                    let chunks_to_reassign = {
                        let mut chunk_servers_lock = chunk_servers.write().await;
                        chunk_servers_lock
                            .remove(&failed_server)
                            .unwrap_or_default()
                    };

                    println!(
                        "[Cron Task] Reassigning chunks from failed server: {:?}",
                        chunks_to_reassign
                    );

                    for chunk_info in chunks_to_reassign {
                        let file_name = chunk_info
                            .chunk_id
                            .split("_chunk_")
                            .next()
                            .unwrap_or_default();

                        // Get a list of chunkservers the current chunk is running on
                        let source_servers: Vec<String> = {
                            let file_chunks_lock = file_chunks.read().await;
                            file_chunks_lock
                                .get(file_name)
                                .map(|chunks| {
                                    chunks
                                        .iter()
                                        .filter_map(|chunk| {
                                            if chunk.chunk_id == chunk_info.chunk_id {
                                                Some(
                                                    chunk
                                                        .server_addresses
                                                        .iter()
                                                        .filter(|addr| addr != &&failed_server)
                                                        .cloned()
                                                        .collect::<Vec<_>>(),
                                                )
                                            } else {
                                                None
                                            }
                                        })
                                        .flatten()
                                        .collect()
                                })
                                .unwrap_or_default()
                        };

                        if source_servers.is_empty() {
                            eprintln!(
                                "[Cron Task] No healthy source servers found for chunk '{}'. Skipping reassignment.",
                                chunk_info.chunk_id
                            );
                            continue;
                        }

                        println!(
                            "[Cron Task] Chunk {:?} source servers: {:?}",
                            chunk_info.chunk_id, source_servers
                        );

                        let existing_replicas = {
                            let file_chunks_lock = file_chunks.read().await;
                            file_chunks_lock
                                .get(file_name)
                                .map(|chunks| {
                                    chunks
                                        .iter()
                                        .filter(|chunk| {
                                            chunk
                                                .server_addresses
                                                .iter()
                                                .any(|addr| addr != &failed_server)
                                        })
                                        .count()
                                })
                                .unwrap_or(0)
                        };
                        let needed_replicas = common_config.replication_factor - existing_replicas;
                        if needed_replicas == 0 {
                            println!(
                                "[Cron Task] Chunk '{}' already has enough replicas on healthy servers.",
                                chunk_info.chunk_id
                            );
                            continue;
                        }

                        // Collect available chunk servers for reassignment
                        // (available means load is less than max_allowed_chunks and does not store the same chunk)
                        let available_servers: HashMap<String, Vec<ChunkInfo>> = {
                            let chunk_servers_lock = chunk_servers.read().await;
                            chunk_servers_lock
                                .iter()
                                .filter(|(_addr, chunks)| {
                                    chunks.len() < max_allowed_chunks
                                        && !chunks.iter().any(|c| c.chunk_id == chunk_info.chunk_id)
                                })
                                .map(|(addr, chunks)| (addr.clone(), chunks.clone()))
                                .collect()
                        };
                        println!(
                            "[Cron Task] Chunk {:?} has available servers: {:?}",
                            chunk_info.chunk_id, available_servers
                        );

                        let mut server_queue: BinaryHeap<Reverse<(usize, String)>> =
                            available_servers
                                .iter()
                                .map(|(addr, chunks)| Reverse((chunks.len(), addr.clone())))
                                .collect();

                        let mut selected_servers = Vec::new();
                        while selected_servers.len() < needed_replicas {
                            if let Some(Reverse((load, addr))) = server_queue.pop() {
                                if !selected_servers.contains(&addr) {
                                    selected_servers.push(addr.clone());
                                    // Add load to the selected server
                                    server_queue.push(Reverse((load + 1, addr)));
                                }
                            } else {
                                break; // Not enough available servers
                            }
                        }

                        if selected_servers.is_empty() {
                            eprintln!(
                                "[Cron Task] No selected servers to reassign chunk '{}'",
                                chunk_info.chunk_id
                            );
                            continue;
                        }

                        println!(
                            "[Cron Task] Reassigning chunk '{}' to servers: {:?}",
                            chunk_info.chunk_id, selected_servers
                        );

                        // Transfer chunk data
                        for target_server in &selected_servers {
                            // Get the first source server
                            let source_server = match source_servers.get(0) {
                                Some(server) => server.clone(),
                                None => {
                                    eprintln!("[Cron Task] No source servers available. Skipping target '{}'", target_server);
                                    continue;
                                }
                            };

                            let chunk_id = chunk_info.chunk_id.clone();

                            // Attempt to connect to the source server
                            let mut source_client =
                                match ChunkClient::connect(format!("http://{}", source_server))
                                    .await
                                {
                                    Ok(client) => client,
                                    Err(e) => {
                                        eprintln!(
                                        "[Cron Task] Failed to connect to source server '{}': {}",
                                        source_server, e
                                    );
                                        continue;
                                    }
                                };

                            // Prepare the request to send the chunk
                            let send_request = SendChunkRequest {
                                chunk_name: chunk_id.clone(),
                                target_address: target_server.clone(),
                            };

                            // Attempt to transfer the chunk
                            match source_client
                                .transfer_chunk(tonic::Request::new(send_request))
                                .await
                            {
                                Ok(_) => {
                                    println!(
                                        "[Cron Task] Successfully transferred chunk '{}' from source server '{}' to target server '{}'.",
                                        chunk_id, source_server, target_server
                                    );
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[Cron Task] Failed to transfer chunk '{}' from '{}' to '{}': {}",
                                        chunk_id, source_server, target_server, e
                                    );
                                }
                            }
                        }

                        // Update chunk server metadata and chunk map
                        let new_chunk_info = ChunkInfo {
                            chunk_id: chunk_info.chunk_id.clone(),
                            server_addresses: selected_servers.clone(),
                            version: chunk_info.version + 1,
                        };

                        for server in &selected_servers {
                            let mut chunk_servers_lock = chunk_servers.write().await;
                            if let Some(chunks) = chunk_servers_lock.get_mut(server) {
                                // update chunkserver => list of chunks mapping
                                chunks.push(new_chunk_info.clone());
                            }
                        }

                        {
                            // overwrite the chunk info
                            let mut chunk_map_lock = chunk_map.write().await;
                            chunk_map_lock.insert(new_chunk_info.chunk_id.clone(), new_chunk_info);
                        }

                        // Update file chunk selected servers in file_chunks mapping
                        {
                            let mut file_chunks_lock = file_chunks.write().await;
                            for (_file_name, chunk_list) in file_chunks_lock.iter_mut() {
                                if let Some(chunk) = chunk_list
                                    .iter_mut()
                                    .find(|c| c.chunk_id == chunk_info.chunk_id)
                                {
                                    chunk.server_addresses = selected_servers.clone();
                                    chunk.version += 1;
                                }
                            }
                        }
                    }
                }
                // Remove all failed servers from the last_heartbeat_time hashmap
                {
                    let mut last_heartbeat_lock = last_heartbeat_time.write().await;
                    for failed_server in &failed_servers {
                        last_heartbeat_lock.remove(failed_server);
                    }
                }
            }
        });
    }
}
