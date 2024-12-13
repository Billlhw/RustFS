use crate::master::master_client::MasterClient;
use chunk::chunk_client::ChunkClient;
use chunk::{DeleteRequest, FileChunk, FileInfo, ReadRequest, UploadRequest};
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
            println!("Assignment response: {:?}", assign_response);

            let chunk_server_address =
                get_chunk_server_address(&mut master_client, &file_name).await?;
            println!("Client->Chunk server address: {}", chunk_server_address);

            if let Err(e) = upload_file(&chunk_server_address, file_name).await {
                eprintln!("Error during upload: {}", e);
            }
        }
        "read" => {
            if args.len() < 3 {
                eprintln!("Usage: read <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let chunk_server_address =
                get_chunk_server_address(&mut master_client, file_name).await?;
            if let Err(e) = read_file(&chunk_server_address, file_name).await {
                eprintln!("Error during read: {}", e);
            }
        }
        "delete" => {
            if args.len() < 3 {
                eprintln!("Usage: delete <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            let chunk_server_address =
                get_chunk_server_address(&mut master_client, file_name).await?;
            if let Err(e) = delete_file(&chunk_server_address, file_name).await {
                eprintln!("Error during delete: {}", e);
            }
        }
        _ => {
            eprintln!("Invalid command. Available commands: upload, read, delete");
        }
    }

    Ok(())
}

async fn get_chunk_server_address(
    master_client: &mut MasterClient<tonic::transport::Channel>,
    file_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Query the master server for the chunk server address
    let response = master_client
        .get_file_chunks(Request::new(master::FileChunkMappingRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    // Fetch the first server address from the response
    let chunk_info_list = response.into_inner().chunks;
    if let Some(first_chunk) = chunk_info_list.get(0) {
        if let Some(server_address) = first_chunk.server_addresses.get(0) {
            return Ok(server_address.clone());
        }
    }

    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No available chunk servers for the file",
    )))
}

async fn upload_file(
    chunk_server_address: &str,
    file_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Attempting to open file: {}", file_name);
    let mut chunk_client = ChunkClient::connect(format!("http://{}", chunk_server_address)).await?;
    println!("TEST");
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

async fn read_file(
    chunk_server_address: &str,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut chunk_client = ChunkClient::connect(format!("http://{}", chunk_server_address)).await?;
    let response = chunk_client
        .read(Request::new(ReadRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    println!("File Content:\n{}", response.into_inner().content);

    Ok(())
}

async fn delete_file(
    chunk_server_address: &str,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut chunk_client = ChunkClient::connect(format!("http://{}", chunk_server_address)).await?;
    let response = chunk_client
        .delete(Request::new(DeleteRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    println!("Delete Response: {}", response.into_inner().message);

    Ok(())
}
