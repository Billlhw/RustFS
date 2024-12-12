use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};

use rustfs::config::{load_config, MasterConfig};
pub mod master {
    tonic::include_proto!("master");
}

// Import the Master service and messages
use master::master_server::{Master, MasterServer};
use master::{
    AssignRequest, AssignResponse, ChunkInfo, FileChunkMapping, FileChunkMappingRequest,
    HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
};

#[derive(Debug, Default)]
pub struct MasterService {
    metadata: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // File -> List of ChunkInfo
    chunk_servers: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // ChunkServer -> List of chunks
    last_heartbeat_time: Arc<RwLock<HashMap<String, u64>>>, // ChunkServer -> Last heartbeat timestamp
    chunk_map: Arc<RwLock<HashMap<String, ChunkInfo>>>,     // chunkID -> ChunkInfo
    config: MasterConfig,                                   // Configuration settings
}

// Implement a constructor for MasterService
impl MasterService {
    pub fn new(config: MasterConfig) -> Self {
        Self {
            metadata: Arc::new(RwLock::new(HashMap::new())),
            chunk_servers: Arc::new(RwLock::new(HashMap::new())),
            last_heartbeat_time: Arc::new(RwLock::new(HashMap::new())),
            chunk_map: Arc::new(RwLock::new(HashMap::new())), // Initialize the new map
            config,                                           // Store the configuration
        }
    }
}

#[tonic::async_trait]
impl Master for MasterService {
    async fn register_chunk_server(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let server_address = request.into_inner().address;
        println!("Registering chunk server: {}", server_address);

        Ok(Response::new(RegisterResponse {
            message: format!("Chunk server '{}' registered successfully.", server_address),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let HeartbeatRequest {
            chunkserver_address,
            chunks,
        } = request.into_inner();

        println!("Heartbeat received from: {}", chunkserver_address);

        // Get the current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Status::internal("SystemTime before UNIX EPOCH!"))?
            .as_secs();

        // Update last_heartbeat_time
        {
            let mut last_heartbeat_time = self.last_heartbeat_time.write().await;
            last_heartbeat_time.insert(chunkserver_address.clone(), now);
        }

        // Update chunk_servers with the received chunks
        {
            let mut chunk_servers = self.chunk_servers.write().await;

            // Parse chunks into ChunkInfo objects
            let chunk_info_list = {
                let mut collected_chunks = Vec::new();
                for chunk in chunks {
                    let chunk_id = chunk.clone();

                    // Acquire a read lock on chunk_map and get the corresponding chunk by chunk id
                    let existing_chunk_info = {
                        let chunk_map = self.chunk_map.read().await;
                        chunk_map.get(&chunk_id).cloned()
                    };

                    if let Some(chunk_info) = existing_chunk_info {
                        // If it exists, use the existing ChunkInfo
                        collected_chunks.push(chunk_info);
                    } else {
                        // If it doesn't exist, print an error and skip the chunk
                        eprintln!("Error: Chunk ID '{}' not found in chunk_map", chunk_id);
                    }
                }
                collected_chunks
            };

            // Insert or update the chunk list for this server
            chunk_servers.insert(chunkserver_address.clone(), chunk_info_list);
        }

        Ok(Response::new(HeartbeatResponse {
            message: format!(
                "Heartbeat from '{}' processed successfully at {}.",
                chunkserver_address, now
            ),
        }))
    }

    async fn assign_chunks(
        &self,
        request: Request<AssignRequest>,
    ) -> Result<Response<AssignResponse>, Status> {
        let request = request.into_inner();
        let file_name = request.file_name;
        let file_size = request.file_size;

        println!(
            "Assigning chunks for file: {} (size: {} bytes)",
            file_name, file_size
        );

        let mut metadata = self.metadata.write().await;
        let mut chunk_servers = self.chunk_servers.write().await;

        if chunk_servers.is_empty() {
            return Err(Status::internal("No available chunk servers"));
        }

        // Calculate the number of chunks of the new file (accounting partial chunks)
        let num_chunks = (file_size + self.config.chunk_size - 1) / self.config.chunk_size;

        let mut assigned_chunks = Vec::new();

        for chunk_index in 0..num_chunks {
            // Select servers that has load less than max_allowed_chunks and has minimal load
            let mut selected_servers = vec![];

            // Create a priority queue for servers based on their load (min-heap)
            let mut server_queue: BinaryHeap<Reverse<(usize, String)>> = chunk_servers
                .iter()
                .map(|(addr, chunks)| Reverse((chunks.len(), addr.clone())))
                .collect();

            // Ensure chunkservers with minimal load is selected
            while selected_servers.len() < self.config.replication_factor {
                if let Some(Reverse((load, addr))) = server_queue.pop() {
                    // Add server to selected list
                    selected_servers.push(addr.clone());
                    // Update the load and reinsert into the priority queue
                    server_queue.push(Reverse((load + 1, addr)));
                } else {
                    return Err(Status::internal(
                        "Not enough available chunk servers to satisfy replication factor",
                    ));
                }
            }

            // Generate a unique chunk ID
            let chunk_id = format!("{}_chunk_{}", file_name, chunk_index + 1);
            let chunk_info = ChunkInfo {
                chunk_id: chunk_id.clone(),
                server_addresses: selected_servers.clone(),
            };

            // Update metadata for this file
            metadata
                .entry(file_name.clone())
                .or_insert_with(Vec::new)
                .push(chunk_info.clone());

            // Update chunkserver mappings
            for server in &selected_servers {
                if let Some(chunks) = chunk_servers.get_mut(server) {
                    chunks.push(chunk_info.clone());
                }
            }

            // Track the assigned chunk
            assigned_chunks.push(chunk_info);
        }

        println!(
            "File '{}' has been divided into {} chunks and assigned to servers.",
            file_name, num_chunks
        );

        // Return the response
        Ok(Response::new(AssignResponse {
            chunk_info_list: assigned_chunks,
        }))
    }

    async fn get_file_chunks(
        &self,
        request: Request<FileChunkMappingRequest>,
    ) -> Result<Response<FileChunkMapping>, Status> {
        let file_name = request.into_inner().file_name;
        println!("Fetching chunks for file: {}", file_name);

        let metadata = self.metadata.read().await;

        let chunks = metadata
            .get(&file_name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("File '{}' not found", file_name)))?;

        Ok(Response::new(FileChunkMapping { file_name, chunks }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = load_config("config.toml")?;
    let master_config: MasterConfig = config.master;

    // Create the server address
    let addr = format!("{}:{}", master_config.host, master_config.port).parse()?;
    println!("MasterServer running at {}", addr);

    let master_service = MasterService::new(master_config);

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr)
        .await?;

    Ok(())
}