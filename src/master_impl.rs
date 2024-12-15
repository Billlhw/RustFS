// Implements the gRPC server behavior defined in the Master trait
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::{Request, Response, Status};

use crate::proto::master::{
    AssignRequest, AssignResponse, ChunkInfo, DeleteFileRequest, DeleteFileResponse,
    FileChunkMapping, FileChunkMappingRequest, HeartbeatRequest, HeartbeatResponse,
    PingMasterRequest, PingMasterResponse, RegisterRequest, RegisterResponse,
    UpdateMetadataRequest, UpdateMetadataResponse,
};

// Import `MasterService` from `master_service.rs`
use crate::master_service::MasterService;
use crate::proto::master::master_server::Master;

#[tonic::async_trait]
impl Master for Arc<MasterService> {
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

    async fn update_metadata(
        &self,
        request: Request<UpdateMetadataRequest>,
    ) -> Result<Response<UpdateMetadataResponse>, Status> {
        let metadata = request.into_inner().metadata.unwrap();
        {
            let mut file_chunks = self.file_chunks.write().await;
            *file_chunks = metadata
                .file_chunks
                .into_iter()
                .map(|(key, chunk_list)| (key, chunk_list.chunks)) // Extract chunks from ChunkList
                .collect();
        }
        {
            let mut chunk_servers = self.chunk_servers.write().await;
            *chunk_servers = metadata
                .chunk_servers
                .into_iter()
                .map(|(key, chunk_list)| (key, chunk_list.chunks)) // Extract chunks from ChunkList
                .collect();
        }
        {
            let mut chunk_map = self.chunk_map.write().await;
            *chunk_map = metadata.chunk_map.into();
        }

        // Print the entire updated metadata
        {
            let file_chunks = self.file_chunks.read().await;
            let chunk_servers = self.chunk_servers.read().await;
            let chunk_map = self.chunk_map.read().await;

            println!("[update_metadata] Updated Metadata:");
            println!("file_chunks: {:#?}", *file_chunks);
            println!("chunk_servers: {:#?}", *chunk_servers);
            println!("chunk_map: {:#?}", *chunk_map);
        }

        println!("Updated metadata from leader");
        Ok(Response::new(UpdateMetadataResponse {
            message: "Metadata update applied successfully".to_string(),
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

        println!(
            "[Heartbeat] received HeartbeatRequest from: {}",
            chunkserver_address
        );

        // Get the current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Status::internal("[Heartbeat] SystemTime before UNIX EPOCH!"))?
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
                        eprintln!(
                            "[Heartbeat] Error: Chunk ID '{}' not found in chunk_map",
                            chunk_id
                        );
                    }
                }
                collected_chunks
            };

            // Insert or update the chunk list for this server
            chunk_servers.insert(chunkserver_address.clone(), chunk_info_list);
        }

        Ok(Response::new(HeartbeatResponse {
            message: format!(
                "[Heartbeat] HeartbeatRequest from '{}' processed successfully.",
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

        // Send updated metadata to registered shadow masters
        self.propagate_metadata_updates().await;

        // Return the response
        Ok(Response::new(AssignResponse {
            file_name: updated_file_name,
            chunk_info_list: assigned_chunks,
        }))
    }

    /// Deletes all chunks and metadata associated with a file.
    ///
    /// - Removes the file and its chunks from `file_chunks`.
    /// - Removes references to the file's chunks from `chunk_servers`.
    /// - Deletes the chunk metadata from `chunk_map`.
    /// - Sends metadata updates to shadow masters.
    async fn delete_file(
        &self,
        request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        let file_name = request.into_inner().file_name;

        let mut file_chunks = self.file_chunks.write().await;
        let mut chunk_servers = self.chunk_servers.write().await;
        let mut chunk_map = self.chunk_map.write().await;

        // Check if the file exists
        if let Some(chunks) = file_chunks.remove(&file_name) {
            println!("Deleting metadata for file: {}", file_name);

            // Remove the chunks from chunk_servers
            for chunk_info in &chunks {
                for server in &chunk_info.server_addresses {
                    if let Some(server_chunks) = chunk_servers.get_mut(server) {
                        server_chunks.retain(|chunk| chunk.chunk_id != chunk_info.chunk_id);
                        if server_chunks.is_empty() {
                            chunk_servers.remove(server); // Optionally clean up empty entries
                        }
                    }
                }

                // Remove the chunk from chunk_map
                chunk_map.remove(&chunk_info.chunk_id);
            }

            println!("All metadata for file '{}' has been deleted.", file_name);

            // Send updated metadata to shadow masters
            self.propagate_metadata_updates().await;

            // Return success response
            Ok(Response::new(DeleteFileResponse {
                success: true,
                message: format!("File '{}' deleted successfully.", file_name),
            }))
        } else {
            println!("File '{}' not found. No metadata deleted.", file_name);

            // Return error response
            Ok(Response::new(DeleteFileResponse {
                success: false,
                message: format!("File '{}' not found.", file_name),
            }))
        }
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

    /// Handle ping master requests
    async fn ping_master(
        &self,
        request: Request<PingMasterRequest>,
    ) -> Result<Response<PingMasterResponse>, Status> {
        let sender_address = request.into_inner().sender_address;
        println!("[ping_master] Received ping from: {}", sender_address);

        if self.is_leader().await {
            let mut shadow_masters = self.shadow_masters.write().await;
            shadow_masters.insert(sender_address.clone());
            println!(
                "[ping_master] Registered '{}' as a shadow master",
                sender_address
            );
        }

        Ok(Response::new(PingMasterResponse {
            is_leader: self.is_leader().await,
        }))
    }
}

/// Determines the leader among all configured master nodes.
///
/// - Tries to connect to all nodes listed in `master_addrs`.
/// - Returns the leader's address if found; otherwise, `None`.
pub async fn determine_leader(self_addr: &str, master_addrs: &[String]) -> Option<String> {
    for addr in master_addrs {
        if addr == self_addr {
            continue; // Skip checking itself
        }

        let target_url = format!("http://{}", addr);
        match crate::proto::master::master_client::MasterClient::connect(target_url).await {
            Ok(mut client) => {
                let request = tonic::Request::new(PingMasterRequest {
                    sender_address: self_addr.to_string(),
                });

                match client.ping_master(request).await {
                    Ok(response) => {
                        let response = response.into_inner();
                        if response.is_leader {
                            println!("Found leader at {}", addr);
                            return Some(addr.clone());
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to contact master at {}: {}", addr, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to connect to {}: {}", addr, e);
            }
        }
    }

    // No leader found
    None
}
