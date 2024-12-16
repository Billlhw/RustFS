use chunk::chunk_client::ChunkClient;
use chunk::{AppendRequest, DeleteRequest, FileChunk, FileInfo, ReadRequest, UploadRequest};
use rand::seq::SliceRandom;
use std::env;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use tracing::{debug, error, info};
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

use rustfs::config::{load_config, CommonConfig};
use rustfs::proto::master::{
    master_client::MasterClient, AssignRequest, ChunkInfo, DeleteFileRequest,
    FileChunkMappingRequest,
};
use rustfs::util::connect_to_master;

pub mod chunk {
    tonic::include_proto!("chunk");
}

pub struct Client {
    common_config: CommonConfig,
    master_client: MasterClient<tonic::transport::Channel>,
}

impl Client {
    pub async fn new(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config = load_config(config_path)?;
        let common_config: CommonConfig = config.common;

        let master_client = connect_to_master(&common_config.master_addrs).await?;

        Ok(Client {
            common_config,
            master_client,
        })
    }

    /// Randomly select a server address for each chunk for read operations
    pub async fn get_randomized_server_addresses(
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

        let mut rng = rand::thread_rng();
        let mut randomized_addresses = Vec::new();

        // Randomly choose a server for server_addresses of each chunk
        for chunk_info in chunk_info_list.iter() {
            if let Some(random_server) = chunk_info.server_addresses.choose(&mut rng) {
                randomized_addresses.push(random_server.clone());
            } else {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No available servers for one or more chunks",
                )));
            }
        }

        if randomized_addresses.is_empty() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No available chunk servers for the file",
            )));
        }

        Ok(randomized_addresses)
    }

    /// Select a server address for each chunk for write operations
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

    /// Retrieves all server addresses for each chunk of the specified file.
    ///
    /// Returns a 2D vector where each inner vector contains the server addresses
    /// for a single chunk.
    pub async fn get_all_server_addresses(
        &mut self,
        file_name: &str,
    ) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
        let response = self
            .master_client
            .get_file_chunks(Request::new(FileChunkMappingRequest {
                file_name: file_name.to_string(),
            }))
            .await?;

        let chunk_info_list = response.into_inner().chunks;
        let all_server_addresses: Vec<Vec<String>> = chunk_info_list
            .iter()
            .map(|chunk| chunk.server_addresses.clone()) // Clone server addresses for each chunk
            .collect();

        if all_server_addresses.is_empty() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No available chunk servers for the file",
            )));
        }

        Ok(all_server_addresses)
    }

    pub async fn upload_file(
        &self,
        chunk_info_list: Vec<ChunkInfo>,
        file_name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Attempting to open file: {}", file_name);
        let mut file = File::open(&file_name).await.map_err(|e| {
            error!("Failed to open file '{}': {}", file_name, e);
            e
        })?;

        // Separate the file into chunks
        let chunk_size = self.common_config.chunk_size as usize;
        let mut chunks = Vec::new();
        let mut buf = vec![0; chunk_size];
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break; // EOF
            }
            chunks.push(buf[..n].to_vec());
        }

        // Check the length of chunk is the same as chunk_info_list (from AssignResponse)
        if chunks.len() != chunk_info_list.len() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Mismatch between number of file chunks and chunk_info_list length",
            )));
        }

        // Iterate through each chunk and upload to all chunkservers // TODO: upload to the primary chunkserver only
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let chunk_info = &chunk_info_list[chunk_index];
            for server_address in &chunk_info.server_addresses {
                let mut chunk_client =
                    ChunkClient::connect(format!("http://{}", server_address)).await?;

                let (tx, rx) = tokio::sync::mpsc::channel(4);

                let file_name_clone = file_name.clone();
                let chunk_id = chunk_info.chunk_id.clone();
                let chunk_data = chunk.clone();

                tokio::spawn(async move {
                    let file_info = FileInfo {
                        file_name: file_name_clone,
                        chunk_id: chunk_id.parse::<u64>().unwrap_or(0), // 使用 chunk_id
                    };

                    if let Err(e) = tx
                        .send(UploadRequest {
                            request: Some(chunk::upload_request::Request::Info(file_info)),
                        })
                        .await
                    {
                        error!("Failed to send file info: {}", e);
                        return;
                    }

                    let file_chunk = FileChunk { data: chunk_data };

                    if let Err(e) = tx
                        .send(UploadRequest {
                            request: Some(chunk::upload_request::Request::Chunk(file_chunk)),
                        })
                        .await
                    {
                        error!("Failed to send file chunk: {}", e);
                        return;
                    }
                });

                let response = chunk_client
                    .upload(Request::new(ReceiverStream::new(rx)))
                    .await?;

                debug!(
                    "Upload Response from server {} for chunk {}: {}",
                    server_address,
                    chunk_index,
                    response.into_inner().message
                );
            }
        }

        info!("File upload completed successfully.");
        Ok(())
    }

    /// Read each chunk and then concatenate
    pub async fn read_file(
        &self,
        randomized_server_addresses: Vec<String>,
        file_name: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut file_content = String::new();

        for (chunk_id, server_address) in randomized_server_addresses.iter().enumerate() {
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
        all_server_addresses: Vec<Vec<String>>, // 2D vector of server addresses for each chunk
        file_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (chunk_id, server_addresses) in all_server_addresses.iter().enumerate() {
            for server_address in server_addresses {
                // Connect to the chunk server
                match ChunkClient::connect(format!("http://{}", server_address)).await {
                    Ok(mut chunk_client) => {
                        // Send the delete request
                        match chunk_client
                            .delete(Request::new(DeleteRequest {
                                file_name: file_name.to_string(),
                                chunk_id: chunk_id as u64,
                            }))
                            .await
                        {
                            Ok(response) => {
                                info!(
                                    "Delete Response from {} for chunk {}: {}",
                                    server_address,
                                    chunk_id,
                                    response.into_inner().message
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to delete chunk {} from server {}: {}",
                                    chunk_id, server_address, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to connect to chunk server {} for chunk {}: {}",
                            server_address, chunk_id, e
                        );
                    }
                }
            }
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

            info!(
                "Append Response from server {}: {}",
                server_address,
                response.into_inner().message
            );
        } else {
            error!("No servers available to append data.");
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load logging config
    let config = load_config("config.toml")?;
    let log_path = config.client.log_path;
    let log_level = config.common.log_level;

    // Set up logger
    let file_appender = rolling::daily(log_path, "client.log");
    let stdout_layer = fmt::layer().with_writer(std::io::stdout).with_ansi(true);
    let file_layer = fmt::layer().with_writer(file_appender).with_ansi(false); // Disable ANSI escape codes for file logs
    let env_filter = EnvFilter::from_default_env().add_directive(log_level.parse().unwrap());
    let subscriber = Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer);

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");

    // Create client instance and load config
    let mut client = Client::new("config.toml").await?;

    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        error!("Usage: client <command> [arguments]");
        error!("Commands: upload <file_name>, read <file_name>, delete <file_name>");
        return Ok(());
    }
    let operation = args[1].as_str();

    match operation {
        "upload" => {
            if args.len() < 3 {
                error!("Usage: upload <file_name>");
                return Ok(());
            }
            let file_name = args[2].clone();
            let file_metadata = tokio::fs::metadata(&file_name).await.map_err(|e| {
                error!("Failed to get metadata for file '{}': {}", file_name, e);
                e
            })?;
            let file_size = file_metadata.len();
            debug!("File size: {} bytes", file_size);

            info!("Requesting chunk assignment for file: {}", file_name);
            let assign_response = client
                .master_client
                .assign_chunks(Request::new(AssignRequest {
                    file_name: file_name.clone(),
                    file_size,
                }))
                .await?
                .into_inner();
            debug!("Got chunk assignment for file: {}", file_name);

            if let Err(e) = client
                .upload_file(assign_response.chunk_info_list, file_name)
                .await
            {
                error!("Error during upload: {}", e);
            }
        }
        "read" => {
            if args.len() < 3 {
                error!("Usage: read <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let randomized_server_addresses = client
                .get_randomized_server_addresses(file_name)
                .await
                .map_err(|e| {
                    error!("Error retrieving random server addresses: {}", e);
                    e
                })?;

            if let Err(e) = client
                .read_file(randomized_server_addresses, file_name)
                .await
            {
                error!("Error during read: {}", e);
            }
        }
        "delete" => {
            if args.len() < 3 {
                error!("Usage: delete <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();

            // Obtain a complete list of addresses
            let all_server_addresses =
                client
                    .get_all_server_addresses(file_name)
                    .await
                    .map_err(|e| {
                        error!("Error retrieving server addresses: {}", e);
                        e
                    })?;

            if all_server_addresses.is_empty() {
                error!("No chunk servers found for file '{}'.", file_name);
                return Ok(());
            }

            // Create DeleteFileRequest for metadata on the Master node
            let request = DeleteFileRequest {
                file_name: file_name.to_string(),
            };

            // Call delete_file RPC
            match client.master_client.delete_file(request).await {
                Ok(response) => {
                    if response.get_ref().success {
                        info!("File '{}' deleted successfully.", file_name);
                    } else {
                        error!(
                            "Failed to delete file '{}': {}",
                            file_name,
                            response.get_ref().message
                        );
                        return Ok(()); // Stop further execution if the deletion failed
                    }
                }
                Err(e) => {
                    error!("Error during delete: {}", e);
                    return Ok(()); // Stop further execution if the RPC call failed
                }
            }

            if let Err(e) = client.delete_file(all_server_addresses, file_name).await {
                error!("Error during delete: {}", e);
            }
        }
        "append" => {
            if args.len() < 4 {
                error!("Usage: append <file_name> <data>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let data = args[3].to_string();
            let primary_server_addresses = client
                .get_primary_server_addresses(file_name)
                .await
                .map_err(|e| {
                    error!("Error retrieving server addresses: {}", e);
                    e
                })?;

            if let Err(e) = client
                .append_file(primary_server_addresses, file_name, data)
                .await
            {
                error!("Error during append: {}", e);
            }
        }
        _ => {
            error!("Invalid command. Available commands: upload, read, delete, append");
        }
    }

    Ok(())
}
