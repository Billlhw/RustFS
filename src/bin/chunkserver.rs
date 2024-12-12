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
    DeleteRequest, DeleteResponse, ReadRequest, ReadResponse, UploadRequest, UploadResponse,
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
                    file =
                        Some(File::create(&file_name).map_err(|e| {
                            Status::internal(format!("Failed to create file: {}", e))
                        })?);

                    let mut files = self.files.lock().await;
                    files.insert(file_name.clone(), file_name.clone());
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

        fs::remove_file(&file_name).map_err(|e| {
            Status::internal(format!("Failed to delete file '{}': {}", file_name, e))
        })?;

        let mut files = self.files.lock().await;
        files.remove(&file_name);

        Ok(Response::new(DeleteResponse {
            message: format!("File '{}' deleted successfully.", file_name),
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

    let addr: SocketAddr =
        format!("{}:{}", chunkserver_config.host, chunkserver_config.port).parse()?;
    let service = ChunkService::default();

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
    println!("Filesystem server running at {}", addr);

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
