use clap::{Arg, Command};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};

use crate::master::master_client::MasterClient;
use rustfs::config::{load_config, ChunkServerConfig, MasterConfig};
pub mod chunk {
    tonic::include_proto!("chunk");
}
pub mod master {
    tonic::include_proto!("master");
}

use chunk::chunk_server::{Chunk, ChunkServer};
use chunk::{
    AppendRequest, AppendResponse, DeleteRequest, DeleteResponse, ReadRequest, ReadResponse,
    UploadRequest, UploadResponse,
};

#[derive(Debug, Default)]
pub struct ChunkService {
    addr_str: String,
    files: Arc<Mutex<HashMap<String, String>>>, // Track files and their paths
    config: ChunkServerConfig,
}

impl ChunkService {
    pub fn new(config: ChunkServerConfig, addr_str: &str) -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            addr_str: addr_str.to_string(),
            config,
        }
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

                    let file_path =
                        format!("{}/{}/{}", self.addr_str, self.config.data_path, file_name);
                    println!("Saving file to: {}", file_path);

                    file = Some(tokio::fs::File::create(&file_path).await.map_err(|e| {
                        Status::internal(format!("Failed to create file at '{}': {}", file_path, e))
                    })?);
                    let mut files = self.files.lock().await;
                    files.insert(file_name.clone(), file_path.clone());
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
        //let chunk_id = req.chunk_id;
        let file_path = format!("{}/{}/{}", self.addr_str, self.config.data_path, file_name); //TODO: add chunk-id in file name
        println!("Fetching file: {}", file_path);

        // Read up to the chunk size from the file
        let mut buffer = vec![0; self.config.chunk_size as usize];
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
        let file_path = format!("{}/{}/{}", self.addr_str, self.config.data_path, file_name);

        println!("Deleting chunk file: {}", file_path);

        fs::remove_file(&file_path).map_err(|e| {
            Status::internal(format!("Failed to delete file '{}': {}", file_path, e))
        })?;

        // TODO: Remove file from metadata
        let mut files = self.files.lock().await;
        files.remove(&file_name); // Adjust if you track chunks separately

        Ok(Response::new(DeleteResponse {
            message: format!(
                "Chunk '{}' of file '{}' deleted successfully.",
                chunk_id, file_name
            ),
        }))
    }

    async fn append(
        &self,
        request: Request<AppendRequest>,
    ) -> Result<Response<AppendResponse>, Status> {
        let req = request.into_inner();
        let file_name = req.file_name;
        let chunk_id = req.chunk_id;
        let data = req.data;

        let file_path = format!("{}/{}/{}", self.addr_str, self.config.data_path, file_name); //TODO: add chunk id
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
    let chunkserver_config: ChunkServerConfig =
        config.chunkserver.expect("ChunkServer config missing");
    let master_config: MasterConfig = config.master; //TODO: use common config

    // Create path to data files
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

    // Start chunkserver service
    println!("ChunkServer running at {}", addr);
    let service = ChunkService::new(chunkserver_config, &sanitized_address);

    let mut master_client = MasterClient::connect(format!(
        "http://{}:{}",
        master_config.host, master_config.port
    ))
    .await?;

    // Send register request
    let response = master_client
        .register_chunk_server(master::RegisterRequest {
            address: addr.to_string(),
        })
        .await?;
    println!("Registered with Master: {}", response.into_inner().message);

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
