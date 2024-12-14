use chunk::chunk_client::ChunkClient;
use chunk::{AppendRequest, DeleteRequest, FileChunk, FileInfo, ReadRequest, UploadRequest};
use std::env;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;

use rustfs::config::{load_config, ClientConfig, CommonConfig};
use rustfs::proto::master::{
    master_client::MasterClient, AssignRequest, ChunkInfo, FileChunkMappingRequest,
};
use rustfs::util::connect_to_master;

pub mod chunk {
    tonic::include_proto!("chunk");
}

pub struct Client {
    config: ClientConfig,
    common_config: CommonConfig,
    master_client: MasterClient<tonic::transport::Channel>, //TODO: need to retry to connect to master when master fails
}

impl Client {
    pub async fn new(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config = load_config(config_path)?;
        let client_config: ClientConfig = config.client;
        let common_config: CommonConfig = config.common;

        let master_client = connect_to_master(&common_config.master_addrs).await?;

        Ok(Client {
            config: client_config,
            common_config,
            master_client,
        })
    }

    pub async fn get_primary_server_addresses(
        &mut self,
        file_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let response = self
            .master_client
            .get_file_chunks(Request::new(FileChunkMappingRequest {
                file_name: file_name.to_string(),
            }))
            .await?;

        let chunk_info_list = response.into_inner().chunks;
        let server_addresses: Vec<String> = chunk_info_list
            .iter()
            .filter_map(|chunk| chunk.server_addresses.get(0).cloned())
            .collect();

        if server_addresses.is_empty() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No available chunk servers for the file",
            )));
        }

        Ok(server_addresses)
    }

    //TODO: change to separate file by chunk size
    pub async fn upload_file(
        &self,
        chunk_info_list: Vec<ChunkInfo>,
        file_name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Attempting to open file: {}", file_name);
        let mut chunk_client =
            ChunkClient::connect(format!("http://{}", chunk_info_list[0].server_addresses[0]))
                .await?;
        let file = File::open(&file_name).await.map_err(|e| {
            eprintln!("Failed to open file '{}': {}", file_name, e);
            e
        })?;

        let (tx, rx) = tokio::sync::mpsc::channel(4);

        tokio::spawn(async move {
            let file_info = FileInfo {
                file_name: file_name.clone(),
                chunk_id: 0, //TODO: use real chunk id
            };

            if let Err(e) = tx
                .send(UploadRequest {
                    request: Some(chunk::upload_request::Request::Info(file_info)),
                })
                .await
            {
                eprintln!("Failed to send file info: {}", e);
                return;
            }

            let mut buf = [0; 1024];
            let mut file = file;
            loop {
                let n = match file.read(&mut buf).await {
                    Ok(n) if n == 0 => break,
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("Failed to read file: {}", e);
                        break;
                    }
                };

                let file_chunk = FileChunk {
                    data: buf[..n].to_vec(),
                };

                if let Err(e) = tx
                    .send(UploadRequest {
                        request: Some(chunk::upload_request::Request::Chunk(file_chunk)),
                    })
                    .await
                {
                    eprintln!("Failed to send file chunk: {}", e);
                    break;
                }
            }
        });

        let response = chunk_client
            .upload(Request::new(ReceiverStream::new(rx)))
            .await?;

        println!("Upload Response: {}", response.into_inner().message);
        Ok(())
    }

    /// Read each chunk and then concatenate
    pub async fn read_file(
        &self,
        primary_server_addresses: Vec<String>,
        file_name: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut file_content = String::new();

        for (chunk_id, server_address) in primary_server_addresses.iter().enumerate() {
            // Connect to the chunk server
            let mut chunk_client =
                ChunkClient::connect(format!("http://{}", server_address)).await?;

            // Read the chunk
            let response = chunk_client
                .read(Request::new(ReadRequest {
                    file_name: file_name.to_string(),
                    chunk_id: chunk_id as u64,
                }))
                .await?;

            // Append the chunk content to the file content
            file_content.push_str(&response.into_inner().content);
        }

        println!("{}", file_content);
        Ok(file_content)
    }

    pub async fn delete_file(
        &self,
        primary_server_addresses: Vec<String>,
        file_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (chunk_id, server_address) in primary_server_addresses.iter().enumerate() {
            // Connect to the chunk server
            let mut chunk_client =
                ChunkClient::connect(format!("http://{}", server_address)).await?;

            // Delete the chunk
            let response = chunk_client
                .delete(Request::new(DeleteRequest {
                    file_name: file_name.to_string(),
                    chunk_id: chunk_id as u64,
                }))
                .await?;

            println!(
                "Delete Response from {}: {}",
                server_address,
                response.into_inner().message
            );
        }
        Ok(())
    }

    // Append data to the end of the file
    pub async fn append_file(
        &self,
        primary_server_addresses: Vec<String>,
        file_name: &str,
        data: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((chunk_id, server_address)) = primary_server_addresses.iter().enumerate().last()
        {
            let mut chunk_client =
                ChunkClient::connect(format!("http://{}", server_address)).await?;
            let response = chunk_client
                .append(Request::new(AppendRequest {
                    file_name: file_name.to_string(),
                    chunk_id: chunk_id as u64,
                    data,
                }))
                .await?;

            println!(
                "Append Response from server {}: {}",
                server_address,
                response.into_inner().message
            );
        } else {
            println!("No servers available to append data.");
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client instance and load config
    let mut client = Client::new("config.toml").await?;

    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: client <command> [arguments]");
        eprintln!("Commands: upload <file_name>, read <file_name>, delete <file_name>");
        return Ok(());
    }
    let operation = args[1].as_str();

    match operation {
        "upload" => {
            if args.len() < 3 {
                eprintln!("Usage: upload <file_name>");
                return Ok(());
            }
            let file_name = args[2].clone();
            let file_metadata = tokio::fs::metadata(&file_name).await.map_err(|e| {
                eprintln!("Failed to get metadata for file '{}': {}", file_name, e);
                e
            })?;
            let file_size = file_metadata.len();
            println!("File size: {} bytes", file_size);

            println!("Requesting chunk assignment for file: {}", file_name);
            let assign_response = client
                .master_client
                .assign_chunks(Request::new(AssignRequest {
                    file_name: file_name.clone(),
                    file_size,
                }))
                .await?
                .into_inner();

            if let Err(e) = client
                .upload_file(assign_response.chunk_info_list, file_name)
                .await
            {
                eprintln!("Error during upload: {}", e);
            }
        }
        "read" => {
            if args.len() < 3 {
                eprintln!("Usage: read <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let primary_server_addresses = client
                .get_primary_server_addresses(file_name)
                .await
                .map_err(|e| {
                    eprintln!("Error retrieving server addresses: {}", e);
                    e
                })?;

            if let Err(e) = client.read_file(primary_server_addresses, file_name).await {
                eprintln!("Error during read: {}", e);
            }
        }
        "delete" => {
            if args.len() < 3 {
                eprintln!("Usage: delete <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let primary_server_addresses = client
                .get_primary_server_addresses(file_name)
                .await
                .map_err(|e| {
                    eprintln!("Error retrieving server addresses: {}", e);
                    e
                })?;

            if let Err(e) = client
                .delete_file(primary_server_addresses, file_name)
                .await
            {
                eprintln!("Error during delete: {}", e);
            }
        }
        "append" => {
            if args.len() < 4 {
                eprintln!("Usage: append <file_name> <data>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let data = args[3].to_string();
            let primary_server_addresses = client
                .get_primary_server_addresses(file_name)
                .await
                .map_err(|e| {
                    eprintln!("Error retrieving server addresses: {}", e);
                    e
                })?;

            if let Err(e) = client
                .append_file(primary_server_addresses, file_name, data)
                .await
            {
                eprintln!("Error during append: {}", e);
            }
        }
        _ => {
            eprintln!("Invalid command. Available commands: upload, read, delete, append");
        }
    }

    Ok(())
}
