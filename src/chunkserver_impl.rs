use std::fs;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::proto::chunk;
use crate::proto::chunk::chunk_client::ChunkClient;

use crate::chunkserver_service::ChunkService;
use crate::proto::chunk::chunk_server::Chunk;
use crate::proto::chunk::{
    AppendRequest, AppendResponse, DeleteRequest, DeleteResponse, ReadRequest, ReadResponse,
    SendChunkRequest, SendChunkResponse, UploadRequest, UploadResponse,
};

#[tonic::async_trait]
impl Chunk for ChunkService {
    /// Transfers a chunk from this chunkserver to another chunkserver
    /// Sent from master to chunk server
    async fn transfer_chunk(
        &self,
        request: Request<SendChunkRequest>,
    ) -> Result<Response<SendChunkResponse>, Status> {
        let req = request.into_inner();
        let chunk_name = req.chunk_name;
        let target_address = req.target_address;

        info!(
            "[transfer_chunk] Received request to transfer chunk '{}' to '{}'",
            chunk_name, target_address
        );

        // Step 1: Read the chunk's data from local storage
        // Split the chunk_name into file_name and chunk_id
        let mut file_name_part = String::new();
        let mut chunk_id_part: u64 = 0;
        if let Some((file_part, id_part)) = chunk_name.split_once("_chunk_") {
            // Assign values to the variables
            file_name_part = file_part.to_string();
            if let Ok(id) = id_part.parse::<u64>() {
                chunk_id_part = id;
            } else {
                error!(
                    "[transfer_chunk] Failed to parse chunk_id from '{}'",
                    id_part
                );
            }
        } else {
            error!(
                "[transfer_chunk] Invalid chunk_name format: '{}'",
                chunk_name
            );
        }
        debug!("[transfer_chunk] File name: {}", file_name_part);
        debug!("[transfer_chunk] Chunk ID: {}", chunk_id_part);

        let file_path = format!(
            "{}/{}/{}",
            self.addr_sanitized, self.config.data_path, chunk_name
        );
        info!(
            "[transfer_chunk] Reading chunk '{}' from file: {}",
            chunk_name, file_path
        );

        let mut file = tokio::fs::File::open(&file_path).await.map_err(|e| {
            tonic::Status::internal(format!(
                "[transfer_chunk] Failed to open file '{}': {}",
                file_path, e
            ))
        })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await.map_err(|e| {
            tonic::Status::internal(format!(
                "[transfer_chunk] Failed to read file '{}': {}",
                file_path, e
            ))
        })?;

        // Step 2: Connect to the target chunkserver
        let mut client = ChunkClient::connect(format!("http://{}", target_address))
            .await
            .map_err(|e| {
                tonic::Status::internal(format!(
                    "[transfer_chunk] Failed to connect to target chunkserver '{}': {}",
                    target_address, e
                ))
            })?;

        // Step 3: Send the chunk data as an UploadRequest
        let mut stream = client
            .upload(tonic::Request::new(tokio_stream::iter(vec![
                UploadRequest {
                    request: Some(chunk::upload_request::Request::Info(chunk::FileInfo {
                        file_name: file_name_part.to_string(),
                        chunk_id: chunk_id_part,
                    })),
                },
                UploadRequest {
                    request: Some(chunk::upload_request::Request::Chunk(chunk::FileChunk {
                        data: buffer.clone(),
                    })),
                },
            ])))
            .await
            .map_err(|e| {
                tonic::Status::internal(format!(
                    "[transfer_chunk] Failed to send chunk to target chunkserver '{}': {}",
                    target_address, e
                ))
            })?;

        // Handle response from target
        let response = stream.get_mut();
        info!(
            "[transfer_chunk] File '{}' chunk '{}' successfully transferred from '{}' to '{}'. Response: {}",
            file_name_part, chunk_id_part, self.addr, target_address, response.message
        );

        Ok(tonic::Response::new(SendChunkResponse {
            message: format!(
                "Chunk '{}' successfully transferred to target '{}'",
                file_name_part, target_address
            ),
        }))
    }

    async fn upload(
        &self,
        request: Request<tonic::Streaming<UploadRequest>>,
    ) -> Result<Response<UploadResponse>, Status> {
        info!("Upload request received.");

        let mut stream = request.into_inner();
        let mut file_name = String::new();
        let mut file: Option<File> = None;

        while let Some(req) = stream.message().await? {
            match req.request {
                Some(chunk::upload_request::Request::Info(info)) => {
                    file_name = info.file_name.clone();
                    info!("Starting upload for file: {}", file_name);
                    let chunk_id = info.chunk_id;
                    let file_path = format!(
                        "{}/{}/{}_chunk_{}",
                        self.addr_sanitized, self.config.data_path, file_name, chunk_id
                    );
                    info!("Saving file to: {}", file_path);

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
        debug!("File '{}' uploaded successfully.", file_name);
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
        info!("Fetching file: {}", file_path);

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
        debug!("Content is: {}", content);
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
        info!("Deleting chunk file: {}", file_path);

        fs::remove_file(&file_path).map_err(|e| {
            Status::internal(format!("Failed to delete file '{}': {}", file_path, e))
        })?;

        // Remove file chunk from metadata
        let chunk_to_remove = format!("{}_chunk_{}", file_name, chunk_id);
        let mut server_chunks_guard = self.server_chunks.lock().await;
        if server_chunks_guard.remove(&chunk_to_remove) {
            info!("Removed chunk: {}", chunk_to_remove);
        } else {
            warn!("chunk not found: {}", chunk_to_remove);
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
        info!("Appending to file: {}", file_path);

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
