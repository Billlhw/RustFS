use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write; // Read crate
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};

pub mod filesystem {
    tonic::include_proto!("filesystem");
    tonic::include_proto!("master");
}

use filesystem::file_system_server::{FileSystem, FileSystemServer};
use filesystem::{
    DeleteRequest, DeleteResponse, ReadRequest, ReadResponse, UploadRequest, UploadResponse,
};

#[derive(Debug, Default)]
pub struct FileSystemService {
    files: Arc<Mutex<HashMap<String, String>>>, // Track files and their paths
}

#[tonic::async_trait]
impl FileSystem for FileSystemService {
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
                Some(filesystem::upload_request::Request::Info(info)) => {
                    file_name = info.file_name.clone();
                    println!("Starting upload for file: {}", file_name);
                    file =
                        Some(File::create(&file_name).map_err(|e| {
                            Status::internal(format!("Failed to create file: {}", e))
                        })?);

                    let mut files = self.files.lock().await;
                    files.insert(file_name.clone(), file_name.clone());
                }
                Some(filesystem::upload_request::Request::Chunk(chunk)) => {
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
    let addr = "[::1]:50051".parse()?;
    let service = FileSystemService::default();

    println!("Filesystem server running at {}", addr);

    Server::builder()
        .add_service(FileSystemServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
