use clap::{Arg, Command};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};

use rustfs::config::{load_config, CommonConfig, MasterConfig};
use rustfs::proto::master;

// Import the Master service and messages
use master::master_server::Master;
use master::{
    AssignRequest, AssignResponse, ChunkInfo, FileChunkMapping, FileChunkMappingRequest,
    HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
};

#[derive(Debug, Default)]
pub struct MasterService {
    file_chunks: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // File -> List of ChunkInfo
    chunk_servers: Arc<RwLock<HashMap<String, Vec<ChunkInfo>>>>, // ChunkServer -> List of chunks
    last_heartbeat_time: Arc<RwLock<HashMap<String, u64>>>, // ChunkServer -> Last heartbeat timestamp
    chunk_map: Arc<RwLock<HashMap<String, ChunkInfo>>>,     // chunkID -> ChunkInfo
    config: MasterConfig,
    common_config: CommonConfig,
    addr: String, // TODO: use it in heartbeat to the master node
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

#[tonic::async_trait]
impl Master for MasterService {
    async fn register_chunk_server(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let chunkserver_address = request.into_inner().address;

        let mut chunk_servers = self.chunk_servers.write().await;
        chunk_servers.insert(chunkserver_address.clone(), vec![]);
        println!(
            "Registering chunk server: {}, Current chunk servers: {:?}",
            chunkserver_address, *chunk_servers
        );

        Ok(Response::new(RegisterResponse {
            message: format!(
                "Chunk server '{}' registered successfully.",
                chunkserver_address
            ),
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
                "Heartbeat from '{}' processed successfully.",
                chunkserver_address,
            ),
        }))
    }

    /// Handles the upload of a new file.
    ///
    /// - Checks if the file name already exists. If it does, appends a suffix to make it unique.
    /// - Selects `replication_factor` chunk servers to store the file chunks.
    /// - Updates the file_chunks with the new file and chunk information.
    /// - Adds the new chunks to the chunk_map (mapping from chunk_id to ChunkInfo).
    async fn assign_chunks(
        &self,
        request: Request<AssignRequest>,
    ) -> Result<Response<AssignResponse>, Status> {
        let request = request.into_inner();
        let file_name = request.file_name;
        let file_size = request.file_size;

        let mut file_chunks = self.file_chunks.write().await;
        let mut chunk_servers = self.chunk_servers.write().await;
        let mut chunk_map = self.chunk_map.write().await;

        let mut updated_file_name = file_name.to_string();
        let mut suffix = 1;
        while file_chunks.contains_key(&updated_file_name) {
            updated_file_name = format!("{}-{}", file_name, suffix);
            suffix += 1;
        }

        println!(
            "Assigning chunks for file: {} (original name: {}, size: {} bytes)",
            updated_file_name, file_name, file_size
        );

        // Create a filtered map of available chunk servers.
        let mut avail_chunk_servers: HashMap<String, Vec<ChunkInfo>> = HashMap::new();
        for (server, chunks) in chunk_servers.iter() {
            if chunks.len() < self.common_config.max_allowed_chunks {
                avail_chunk_servers.insert(server.clone(), chunks.clone());
            }
        }

        if avail_chunk_servers.is_empty() {
            return Err(Status::internal(
                "No available chunk servers: all servers are full",
            ));
        }

        // Calculate the number of chunks of the new file (accounting partial chunks)
        let num_chunks =
            (file_size + self.common_config.chunk_size - 1) / self.common_config.chunk_size;

        let mut assigned_chunks = Vec::new();

        for chunk_index in 0..num_chunks {
            // Select servers that has minimal load
            let mut selected_servers = vec![];

            // Create a priority queue for servers based on their load (min-heap)
            let mut server_queue: BinaryHeap<Reverse<(usize, String)>> = avail_chunk_servers
                .iter()
                .map(|(addr, chunks)| Reverse((chunks.len(), addr.clone())))
                .collect();

            // Ensure chunkservers with minimal load is selected
            while selected_servers.len() < self.common_config.replication_factor {
                if let Some(Reverse((load, addr))) = server_queue.pop() {
                    // Check if repeated (when avail_chunk_servers.len() < replication_factor)
                    if selected_servers.contains(&addr.to_string()) {
                        break;
                    }
                    // Add server to selected list
                    selected_servers.push(addr.clone());
                    // Update the load and reinsert into the priority queue
                    server_queue.push(Reverse((load + 1, addr)));
                }
            }
            println!("selected servers: {:?}", selected_servers);

            // Generate a unique chunk ID
            let chunk_id = format!("{}_chunk_{}", updated_file_name, chunk_index);
            let chunk_info = ChunkInfo {
                chunk_id: chunk_id.clone(),
                server_addresses: selected_servers.clone(),
                version: 0,
            };

            // Update file_chunks metadata for this file
            file_chunks
                .entry(file_name.clone())
                .and_modify(|vec| {
                    vec.clear();
                    vec.push(chunk_info.clone());
                })
                .or_insert_with(|| vec![chunk_info.clone()]);

            // Update chunkserver mappings
            for server in &selected_servers {
                if let Some(chunks) = chunk_servers.get_mut(server) {
                    chunks.push(chunk_info.clone());
                }
            }
            chunk_map.insert(chunk_id.clone(), chunk_info.clone());
            println!("[assign_chunks] updated chunk_servers: {:?}", chunk_servers);

            // Track the assigned chunk
            assigned_chunks.push(chunk_info);
        }

        println!(
            "File '{}' has been divided into {} chunk(s) and assigned to servers.",
            file_name, num_chunks
        );

        // Return the response
        Ok(Response::new(AssignResponse {
            file_name: updated_file_name,
            chunk_info_list: assigned_chunks,
        }))
    }

    async fn get_file_chunks(
        &self,
        request: Request<FileChunkMappingRequest>,
    ) -> Result<Response<FileChunkMapping>, Status> {
        let file_name = request.into_inner().file_name;
        println!("Fetching chunks for file: {}", file_name);

        let file_chunks = self.file_chunks.read().await;
        println!("[get_file_chunks] Current file_chunks: {:?}", file_chunks);
        let chunks = file_chunks
            .get(&file_name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("File '{}' not found", file_name)))?;

        Ok(Response::new(FileChunkMapping { file_name, chunks }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let matches = Command::new("ChunkServer")
        .version("1.0")
        .about("Starts a ChunkServer")
        .arg(
            Arg::new("address")
                .short('a')
                .value_name("ADDR")
                .help("Sets the address for the ChunkServer (e.g., 127.0.0.1:50010)")
                .required(true),
        )
        .get_matches();

    let addr = matches
        .get_one::<String>("address")
        .expect("Address is required");

    // Load configuration
    let config = load_config("config.toml")?;
    let master_config: MasterConfig = config.master;
    let common_config: CommonConfig = config.common;

    // Create the server address
    println!("MasterServer running at {}", addr);
    let master_service = MasterService::new(addr, master_config, common_config);
    let addr_socket: SocketAddr = addr.parse()?;

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr_socket)
        .await?;

    Ok(())
}
