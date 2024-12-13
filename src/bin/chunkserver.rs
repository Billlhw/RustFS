use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write; // Read crate
use std::net::SocketAddr;
use std::sync::Arc;
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
    files: Arc<Mutex<HashMap<String, String>>>, // Track files and their paths
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

                    let file_path = format!("{}/{}", "./data/chunks", file_name); //TODO: make path as an variable
                    println!("Saving file to: {}", file_path);

                    file = Some(File::create(&file_path).map_err(|e| {
                        Status::internal(format!("Failed to create file at '{}': {}", file_path, e))
                    })?);

                    let mut files = self.files.lock().await;
                    files.insert(file_name.clone(), file_path.clone());
                }
                Some(chunk::upload_request::Request::Chunk(chunk)) => {
                    if let Some(f) = &mut file {
                        f.write_all(&chunk.data).map_err(|e| {
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

    async fn read(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let file_name = request.into_inner().file_name;
        println!("Fetching file: {}", file_name);

        let content = fs::read_to_string(&file_name)
            .map_err(|e| Status::internal(format!("Failed to read file '{}': {}", file_name, e)))?;

        Ok(Response::new(ReadResponse { content }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let file_name = request.into_inner().file_name;
        println!("Deleting file: {}", file_name);
        let file_path = format!("{}/{}", "./data/chunks", file_name); //TODO: make path as an variable
        println!("Deleting file: {}", file_path);

        fs::remove_file(&file_path).map_err(|e| {
            Status::internal(format!("Failed to delete file '{}': {}", file_path, e))
        })?;

        let mut files = self.files.lock().await;
        files.remove(&file_name);

        Ok(Response::new(DeleteResponse {
            message: format!("File '{}' deleted successfully.", file_name),
        }))
    }

    async fn append(
        &self,
        request: Request<AppendRequest>,
    ) -> Result<Response<AppendResponse>, Status> {
        let request = request.into_inner();
        let file_name = request.file_name;
        let data = request.data;

        let file_path = format!("{}/{}", "./data/chunks", file_name); //TODO: make path as an variable
        println!("Appending to file: {}", file_path);

        let mut file = fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| Status::internal(format!("Failed to open file '{}': {}", file_path, e)))?;

        file.write_all(data.as_bytes()).map_err(|e| {
            Status::internal(format!("Failed to write to file '{}': {}", file_path, e))
        })?;

        Ok(Response::new(AppendResponse {
            message: format!("Data appended to file '{}'", file_name),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = load_config("config.toml")?;
    let chunkserver_config: ChunkServerConfig =
        config.chunkserver.expect("ChunkServer config missing");
    let master_config: MasterConfig = config.master;

    if !std::path::Path::new(&chunkserver_config.data_path).exists() {
        std::fs::create_dir_all(&chunkserver_config.data_path).map_err(|e| {
            eprintln!(
                "Failed to create data directory '{}': {}",
                chunkserver_config.data_path, e
            );
            e
        })?;
    }

    println!("Data directory verified: {}", chunkserver_config.data_path);

    let addr: SocketAddr =
        format!("{}:{}", chunkserver_config.host, chunkserver_config.port).parse()?;

    println!("ChunkServer running at {}", addr);
    let service = ChunkService::default();

    let mut master_client = MasterClient::connect(format!(
        "http://{}:{}",
        master_config.host, master_config.port
    ))
    .await?;

    // Send register request
    let response = master_client
        .register_chunk_server(master::RegisterRequest {
            address: format!("{}:{}", chunkserver_config.host, chunkserver_config.port),
        })
        .await?;
    println!("Registered with Master: {}", response.into_inner().message);

    println!("Filesystem server running at {}", addr);

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
