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
    otp: Option<String>,
}

impl Client {
    pub async fn new(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config = load_config(config_path)?;
        let common_config: CommonConfig = config.common;

        let master_client = connect_to_master(&common_config.master_addrs).await?;

        Ok(Client {
            common_config,
            master_client,
            otp: None,
        })
    }

    pub async fn authenticate(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.common_config.use_authentication {
            info!("Authentication is disabled. Skipping OTP request.");
            return Ok(());
        }
        info!("Authenticating user: {}", username);

        // Create and send the authentication request to the master server
        let response = self
            .master_client
            .authenticate(Request::new(rustfs::proto::master::AuthenticateRequest {
                username: username.to_string(),
                password: password.to_string(),
            }))
            .await;

        match response {
            Ok(response) => {
                let auth_response = response.into_inner();
                self.otp = Some(auth_response.otp);

                info!(
                    "User '{}' authenticated successfully. OTP received and stored.",
                    username
                );
                Ok(())
            }
            Err(err) => {
                // Log the specific error and re-throw it
                error!(
                    "Authentication failed for user '{}': {}",
                    username,
                    err.message()
                );
                Err(Box::new(err))
            }
        }
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
                let otp_clone = self.otp.clone();
                let otp_value = otp_clone.unwrap_or_else(|| "".to_string());

                tokio::spawn(async move {
                    let file_info = FileInfo {
                        file_name: file_name_clone,
                        chunk_id: chunk_id.parse::<u64>().unwrap_or(0),
                    };

                    if let Err(e) = tx
                        .send(UploadRequest {
                            request: Some(chunk::upload_request::Request::Info(file_info)),
                            otp: otp_value.clone(),
                            is_internal: false,
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
                            otp: otp_value.clone(),
                            is_internal: false,
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
        let otp_clone = self.otp.clone();
        let otp_value = otp_clone.unwrap_or_else(|| "".to_string());

        for (chunk_id, server_address) in randomized_server_addresses.iter().enumerate() {
            // Connect to the chunk server
            let mut chunk_client =
                ChunkClient::connect(format!("http://{}", server_address)).await?;

            // Read the chunk
            let response = chunk_client
                .read(Request::new(ReadRequest {
                    file_name: file_name.to_string(),
                    chunk_id: chunk_id as u64,
                    otp: otp_value.clone(),
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
        let otp_clone = self.otp.clone();
        let otp_value = otp_clone.unwrap_or_else(|| "".to_string());

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
                                otp: otp_value.clone(),
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
        all_server_addresses: Vec<Vec<String>>, // 2D vector of server addresses
        file_name: &str,
        data: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let otp_clone = self.otp.clone();
        let otp_value = otp_clone.unwrap_or_else(|| "".to_string());

        for (chunk_id, server_addresses) in all_server_addresses.iter().enumerate() {
            let mut append_tasks = vec![];

            for server_address in server_addresses {
                let chunk_id = chunk_id as u64; // Convert to u64 for compatibility
                let server_address = server_address.clone();
                let file_name = file_name.to_string();
                let data = data.clone();
                let otp_value = otp_value.clone();

                // Spawn a task for each replica
                let task = tokio::spawn(async move {
                    match ChunkClient::connect(format!("http://{}", server_address)).await {
                        Ok(mut chunk_client) => {
                            match chunk_client
                                .append(Request::new(AppendRequest {
                                    file_name,
                                    chunk_id,
                                    data,
                                    otp: otp_value,
                                }))
                                .await
                            {
                                Ok(response) => {
                                    info!(
                                        "Append Response from server {} for chunk {}: {}",
                                        server_address,
                                        chunk_id,
                                        response.into_inner().message
                                    );
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to append to chunk {} on server {}: {}",
                                        chunk_id, server_address, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to connect to server {}: {}", server_address, e);
                        }
                    }
                });

                append_tasks.push(task);
            }

            // Wait for all tasks to complete for the current chunk
            futures::future::join_all(append_tasks).await;
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
        error!("Usage: client <command> [arguments] [-u <username>] [-p <password>]");
        error!("Commands: upload <file_name>, read <file_name>, delete <file_name>, append <file_name> <data>");
        return Ok(());
    }
    let operation = args[1].as_str();
    let mut username: Option<String> = None;
    let mut password: Option<String> = None;

    // Extract the authentication parameters
    let mut i = 2; // Start after the command name
    while i < args.len() {
        match args[i].as_str() {
            "-u" => {
                if i + 1 < args.len() {
                    username = Some(args[i + 1].clone());
                    i += 1; // Skip the next argument as it's the username
                } else {
                    error!("Missing username after -u");
                    return Ok(());
                }
            }
            "-p" => {
                if i + 1 < args.len() {
                    password = Some(args[i + 1].clone());
                    i += 1; // Skip the next argument as it's the password
                } else {
                    error!("Missing password after -p");
                    return Ok(());
                }
            }
            _ => {} // Ignore other arguments
        }
        i += 1;
    }

    // Authenticate the user
    if client.common_config.use_authentication {
        if let (Some(ref username), Some(ref password)) = (username.as_deref(), password.as_deref())
        {
            client.authenticate(username, password).await?;
            info!("after auth");
        } else {
            error!("Authentication requires both username and password.");
            return Ok(());
        }
    }

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
            let all_server_addresses =
                client
                    .get_all_server_addresses(file_name)
                    .await
                    .map_err(|e| {
                        error!("Error retrieving server addresses: {}", e);
                        e
                    })?;

            if let Err(e) = client
                .append_file(all_server_addresses, file_name, data)
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
