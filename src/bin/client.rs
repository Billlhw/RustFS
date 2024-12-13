use crate::master::{master_client::MasterClient, ChunkInfo};
use chunk::chunk_client::ChunkClient;
use chunk::{AppendRequest, DeleteRequest, FileChunk, FileInfo, ReadRequest, UploadRequest};
use rustfs::config::{load_config, MasterConfig};
use std::env;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;

pub mod master {
    tonic::include_proto!("master");
}

pub mod chunk {
    tonic::include_proto!("chunk");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = load_config("config.toml")?;
    let master_config: MasterConfig = config.master;

    // Connect to the master server
    let mut master_client = MasterClient::connect(format!(
        "http://{}:{}",
        master_config.host, master_config.port
    ))
    .await?;

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
            println!("Attempting to open file: {}", file_name);
            // println!("Current directory: {:?}", std::env::current_dir());
            let file_metadata = tokio::fs::metadata(&file_name).await.map_err(|e| {
                eprintln!("Failed to get metadata for file '{}': {}", file_name, e);
                e
            })?;
            let file_size = file_metadata.len();
            println!("File size: {} bytes", file_size);

            println!("Requesting chunk assignment for file: {}", file_name);
            let assign_response = master_client
                .assign_chunks(Request::new(master::AssignRequest {
                    file_name: file_name.clone(),
                    file_size,
                }))
                .await?;
            let inner_response = assign_response.into_inner();
            println!("Assignment inner response: {:?}", inner_response);

            if let Err(e) = upload_file(
                inner_response.chunk_info_list,
                inner_response.file_name.clone(),
            )
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
            let primary_server_addresses =
                get_primary_server_addresses(&mut master_client, file_name).await?; //TODO: use randomized strategy for load balancing
            if let Err(e) = read_file(primary_server_addresses, file_name).await {
                eprintln!("Error during read: {}", e);
            }
        }
        "delete" => {
            if args.len() < 3 {
                eprintln!("Usage: delete <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let primary_server_addresses =
                get_primary_server_addresses(&mut master_client, file_name).await?;
            if let Err(e) = delete_file(primary_server_addresses, file_name).await {
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
            let primary_server_addresses =
                get_primary_server_addresses(&mut master_client, file_name).await?;
            if let Err(e) = append_file(primary_server_addresses, file_name, data).await {
                eprintln!("Error during append: {}", e);
            }
        }
        _ => {
            eprintln!("Invalid command. Available commands: upload, read, delete, append");
        }
    }

    Ok(())
}

///get_chunk_server_address
async fn get_primary_server_addresses(
    master_client: &mut MasterClient<tonic::transport::Channel>,
    file_name: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    // Query the master server for the chunk server addresses
    let response = master_client
        .get_file_chunks(Request::new(master::FileChunkMappingRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    // Fetch the chunk info list from the response
    let chunk_info_list = response.into_inner().chunks;

    // Collect the first server address from each chunk
    let server_addresses: Vec<String> = chunk_info_list
        .iter()
        .filter_map(|chunk| chunk.server_addresses.get(0).cloned())
        .collect();

    // Check if we found any server addresses
    if server_addresses.is_empty() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No available chunk servers for the file",
        )));
    }

    Ok(server_addresses)
}

//TODO: change to separate file by chunk size, add configuration in client (change configuration to common)
async fn upload_file(
    chunk_info_list: Vec<ChunkInfo>,
    file_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Attempting to open file: {}", file_name);
    let mut chunk_client =
        ChunkClient::connect(format!("http://{}", chunk_info_list[0].server_addresses[0])).await?;
    let file = File::open(&file_name).await.map_err(|e| {
        eprintln!("Failed to open file '{}': {}", file_name, e);
        e
    })?;

    let (tx, rx) = tokio::sync::mpsc::channel(4);

    tokio::spawn(async move {
        let file_info = FileInfo {
            file_name: file_name.clone(),
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
        let mut file = file; // Move into the loop scope
        loop {
            let n = match file.read(&mut buf).await {
                Ok(n) if n == 0 => break, // EOF
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
async fn read_file(
    primary_server_addresses: Vec<String>,
    file_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut file_content = String::new();

    for (chunk_id, server_address) in primary_server_addresses.iter().enumerate() {
        // Connect to the chunk server
        let mut chunk_client = ChunkClient::connect(format!("http://{}", server_address)).await?;

        // Read the chunk
        let response = chunk_client
            .read(Request::new(ReadRequest {
                file_name: file_name.to_string(),
                chunk_id: chunk_id as u64, // Pass the chunk ID
            }))
            .await?;

        // Append the chunk content to the file content
        file_content.push_str(&response.into_inner().content);
    }

    println!("{}", file_content);
    Ok(file_content)
}

async fn delete_file(
    primary_server_addresses: Vec<String>,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for (chunk_id, server_address) in primary_server_addresses.iter().enumerate() {
        let mut chunk_client = ChunkClient::connect(format!("http://{}", server_address)).await?;

        // Send delete request for each chunk
        let response = chunk_client
            .delete(Request::new(DeleteRequest {
                file_name: file_name.to_string(),
                chunk_id: chunk_id as u64, // Include chunk_id in the request
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

async fn append_file(
    primary_server_addresses: Vec<String>,
    file_name: &str,
    data: String,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some((chunk_id, server_address)) = primary_server_addresses.iter().enumerate().last() {
        let mut chunk_client = ChunkClient::connect(format!("http://{}", server_address)).await?;
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
