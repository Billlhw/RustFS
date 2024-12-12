use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};

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
    chunk_servers: Arc<RwLock<HashMap<String, usize>>>, // ChunkServer -> Load (number of chunks)
    last_heartbeat_time: Arc<RwLock<HashMap<String, u64>>>, // ChunkServer -> Last heartbeat timestamp
}

#[tonic::async_trait]
impl Master for MasterService {
    async fn register_chunk_server(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let server_address = request.into_inner().address;
        println!("Registering chunk server: {}", server_address);

        let mut chunk_servers = self.chunk_servers.write().await;
        chunk_servers.insert(server_address.clone(), 0); // Initially 0 load

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
            chunks: _,
        } = request.into_inner();

        println!("Heartbeat received from: {}", chunkserver_address);

        // Get the current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Status::internal("SystemTime before UNIX EPOCH!"))?
            .as_secs();

        // Update last_heartbeat_time
        let mut last_heartbeat_time = self.last_heartbeat_time.write().await;
        last_heartbeat_time.insert(chunkserver_address.clone(), now);

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
        let file_name = request.into_inner().file_name;
        println!("Assigning chunks for file: {}", file_name);

        let mut metadata = self.metadata.write().await;
        let mut chunk_servers = self.chunk_servers.write().await;

        // Select up to 3 servers with the least load
        let mut selected_servers = vec![];
        for (addr, _) in chunk_servers.iter().take(3) {
            selected_servers.push(addr.clone());
        }

        if selected_servers.is_empty() {
            return Err(Status::internal("No available chunk servers"));
        }

        // Generate a chunk ID and update metadata
        let chunk_id = format!("{}_chunk_{}", file_name, metadata.len() + 1);
        let chunk_info = ChunkInfo {
            chunk_id: chunk_id.clone(),
            server_addresses: selected_servers.clone(),
        };

        metadata
            .entry(file_name.clone())
            .or_insert_with(Vec::new)
            .push(chunk_info);

        // Update chunkserver load using references
        for server in &selected_servers {
            if let Some(load) = chunk_servers.get_mut(server) {
                *load += 1;
            }
        }

        Ok(Response::new(AssignResponse {
            file_name,
            chunk_id,
            server_address: selected_servers.join(", "), // Use reference here
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
    let addr = "[::1]:50051".parse()?;
    let master_service = MasterService::default();

    println!("MasterService listening on {}", addr);

    Server::builder()
        .add_service(MasterServer::new(master_service))
        .serve(addr)
        .await?;

    Ok(())
}
