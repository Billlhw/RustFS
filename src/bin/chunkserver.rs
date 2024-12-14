use clap::{Arg, Command};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::Duration;
use tonic::{transport::Server, Request, Response, Status};

use rustfs::config::{load_config, ChunkServerConfig, CommonConfig};
use rustfs::proto::master::{master_client::MasterClient, HeartbeatRequest, RegisterRequest};
use rustfs::util::connect_to_master;
pub mod chunk {
    tonic::include_proto!("chunk");
}

use chunk::chunk_server::{Chunk, ChunkServer};
use chunk::{
    AppendRequest, AppendResponse, DeleteRequest, DeleteResponse, ReadRequest, ReadResponse,
    UploadRequest, UploadResponse,
};

#[derive(Clone, Debug, Default)]
pub struct ChunkService {
    addr: String,                               // Chunkserver address
    addr_sanitized: String,                     // Sanitized address, used for file directories
    server_chunks: Arc<Mutex<HashSet<String>>>, // Track metadata of all chunks stored
    config: ChunkServerConfig,
    common_config: CommonConfig,
}

impl ChunkService {
    pub fn new(
        addr: &str,
        addr_sanitized: &str,
        config: ChunkServerConfig,
        common_config: CommonConfig,
    ) -> Self {
        Self {
            server_chunks: Arc::new(Mutex::new(HashSet::new())),
            addr: addr.to_string(),
            addr_sanitized: addr_sanitized.to_string(),
            config,
            common_config,
        }
    }

    pub async fn send_heartbeat(
        &self,
        master_client: MasterClient<tonic::transport::Channel>, // Take ownership
    ) -> Result<(), Box<dyn std::error::Error>> {
        let interval = Duration::from_secs(self.common_config.heartbeat_interval);
        let addr = self.addr.clone();
        let server_chunks = self.server_chunks.clone(); //create a new pointer to the hashset

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            let mut client = master_client; // Move the owned client into the task

            loop {
                // Heartbeat logic
                interval.tick().await;

                // Collect chunk information
                let chunks: Vec<String> = server_chunks
                    .lock()
                    .await
                    .iter()
                    .cloned() // Clone each String from the HashSet
                    .collect();

                // Create and send the heartbeat request
                let request = HeartbeatRequest {
                    chunkserver_address: addr.clone(),
                    chunks,
                };

                match client.heartbeat(tonic::Request::new(request)).await {
                    Ok(response) => {
                        println!(
                            "Heartbeat acknowledged by Master: {}",
                            response.into_inner().message
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to send heartbeat: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

#[tonic::async_trait]
impl Chunk for ChunkService {
    async fn upload(
        &self,
        request: Request<tonic::Streaming<UploadRequest>>,
    ) -> Result<Response<UploadResponse>, Status> {
        println!("Upload request received.");

        let mut stream = request.into_inner();
        let mut file_name = String::new();
        let mut file: Option<File> = None;

        while let Some(req) = stream.message().await? {
            match req.request {
                Some(chunk::upload_request::Request::Info(info)) => {
                    file_name = info.file_name.clone();
                    println!("Starting upload for file: {}", file_name);
                    let chunk_id = info.chunk_id;
                    let file_path = format!(
                        "{}/{}/{}_chunk_{}",
                        self.addr_sanitized, self.config.data_path, file_name, chunk_id
                    );
                    println!("Saving file to: {}", file_path);

                    file = Some(tokio::fs::File::create(&file_path).await.map_err(|e| {
                        Status::internal(format!("Failed to create file at '{}': {}", file_path, e))
                    })?);

                    // update metadata of chunkserver
                    let chunk_name = format!("{}_chunk_{}", file_name, chunk_id);
                    let mut server_chunks_guard = self.server_chunks.lock().await;
                    server_chunks_guard.insert(chunk_name);
                }
                Some(chunk::upload_request::Request::Chunk(chunk)) => {
                    if let Some(f) = &mut file {
                        f.write_all(&chunk.data).await.map_err(|e| {
                            Status::internal(format!("Failed to write to file: {}", e))
                        })?;
                    } else {
                        return Err(Status::invalid_argument("File info not received yet"));
                    }
                }
                None => return Err(Status::invalid_argument("Empty request")),
            }
        }
        // println!("File '{}' uploaded successfully.", file_name);
        Ok(Response::new(UploadResponse {
            message: format!("File '{}' uploaded successfully.", file_name),
        }))
    }

    /// Read the file chunk
    async fn read(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();
        let file_name = req.file_name;
        let chunk_id = req.chunk_id;
        let file_path = format!(
            "{}/{}/{}_chunk_{}",
            self.addr_sanitized, self.config.data_path, file_name, chunk_id
        );
        println!("Fetching file: {}", file_path);

        // Read up to the chunk size from the file
        let mut buffer = vec![0; self.common_config.chunk_size as usize];
        let mut file = tokio::fs::File::open(&file_path)
            .await
            .map_err(|e| Status::internal(format!("Failed to open file '{}': {}", file_path, e)))?;
        let bytes_read = file.read(&mut buffer).await.map_err(|e| {
            Status::internal(format!("Failed to read chunk file '{}': {}", file_path, e))
        })?;
        buffer.truncate(bytes_read);

        let content = String::from_utf8(buffer).map_err(|e| {
            Status::internal(format!(
                "Invalid UTF-8 content in file '{}': {}",
                file_path, e
            ))
        })?;
        println!("Content is: {}", content);
        Ok(Response::new(ReadResponse { content }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let file_name = req.file_name;
        let chunk_id = req.chunk_id;

        // Include chunk_id in the file path (if chunks are stored separately by ID)
        let file_path = format!(
            "{}/{}/{}_chunk_{}",
            self.addr_sanitized, self.config.data_path, file_name, chunk_id
        );
        println!("Deleting chunk file: {}", file_path);

        fs::remove_file(&file_path).map_err(|e| {
            Status::internal(format!("Failed to delete file '{}': {}", file_path, e))
        })?;

        // Remove file chunk from metadata
        let chunk_to_remove = format!("{}_chunk_{}", file_name, chunk_id);
        let mut server_chunks_guard = self.server_chunks.lock().await;
        if server_chunks_guard.remove(&chunk_to_remove) {
            println!("Removed chunk: {}", chunk_to_remove);
        } else {
            println!("chunk not found: {}", chunk_to_remove);
        }

        Ok(Response::new(DeleteResponse {
            message: format!(
                "Chunk '{}' of file '{}' deleted successfully.",
                chunk_id, file_name
            ),
        }))
    }

    /// TODO: currently assuming the last chunk fits the appended content
    async fn append(
        &self,
        request: Request<AppendRequest>,
    ) -> Result<Response<AppendResponse>, Status> {
        let req = request.into_inner();
        let file_name = req.file_name;
        let chunk_id = req.chunk_id;
        let data = req.data;

        let file_path = format!(
            "{}/{}/{}_chunk_{}",
            self.addr_sanitized, self.config.data_path, file_name, chunk_id
        );
        println!("Appending to file: {}", file_path);

        let mut file = File::open(&file_path)
            .await
            .map_err(|e| Status::internal(format!("Failed to open file '{}': {}", file_path, e)))?;

        // Write data to the file
        file.write_all(data.as_bytes()).await.map_err(|e| {
            Status::internal(format!("Failed to write to file '{}': {}", file_path, e))
        })?;

        Ok(Response::new(AppendResponse {
            message: format!(
                "Data appended to chunk {} of file '{}'",
                chunk_id, file_name
            ),
        }))
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

    let address = matches
        .get_one::<String>("address")
        .expect("Address is required");
    let addr: SocketAddr = address.parse().expect("Invalid address format");

    // Load configuration
    let config = load_config("config.toml")?;
    let chunkserver_config: ChunkServerConfig = config.chunkserver;
    let common_config: CommonConfig = config.common;

    // Create path to data files of chunkserver
    let sanitized_address = addr.to_string().replace(':', "_"); // Convert to a valid directory name
    let full_data_path = format!("{}/{}", sanitized_address, chunkserver_config.data_path);
    if !std::path::Path::new(&full_data_path).exists() {
        std::fs::create_dir_all(&full_data_path).map_err(|e| {
            eprintln!(
                "Failed to create data directory '{}': {}",
                full_data_path, e
            );
            e
        })?;
    }
    println!("Data directory verified: {}", full_data_path);

    // Connect to the master, if no master is available, exit the program
    let mut master_client = connect_to_master(&common_config.master_addrs).await?;

    // Send register request to master
    let response = master_client
        .register_chunk_server(RegisterRequest {
            address: addr.to_string(),
        })
        .await?;
    println!("Registered with Master: {}", response.into_inner().message);

    // Start chunkserver service
    println!("ChunkServer running at {}", addr);
    let service = ChunkService::new(
        &addr.to_string(),
        &sanitized_address,
        chunkserver_config,
        common_config,
    );

    // Clone the master_client and spawn the heartbeat task at background
    let heartbeat_client = master_client.clone();
    let heartbeat_service = service.clone();
    tokio::spawn(async move {
        if let Err(e) = heartbeat_service.send_heartbeat(heartbeat_client).await {
            eprintln!("Heartbeat task failed: {}", e);
        }
    });

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
