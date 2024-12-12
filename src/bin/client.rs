// src/bin/client.rs

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

    // Connect to the master
    let mut master_client = MasterClient::connect(format!(
        "http://{}:{}",
        master_config.host, master_config.port
    ))
    .await?;

    // Read command-line arguments
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
            if let Err(e) = upload_file(&mut master_client, file_name).await {
                eprintln!("Error: {}", e);
            }
        }
        "read" => {
            if args.len() < 3 {
                eprintln!("Usage: read <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            if let Err(e) = read_file(&mut master_client, file_name).await {
                eprintln!("Error: {}", e);
            }
        }
        "delete" => {
            if args.len() < 3 {
                eprintln!("Usage: delete <file_name>");
                return Ok(());
            }
            let file_name = args[2].as_str();
            if let Err(e) = delete_file(&mut master_client, file_name).await {
                eprintln!("Error: {}", e);
            }
        }
        _ => {
            eprintln!("Invalid command. Available commands: upload, read, delete");
        }
    }

    Ok(())
}

async fn upload_file(
    client: &mut MasterClient<tonic::transport::Channel>,
    file_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(&file_name).await.map_err(|e| {
        eprintln!("Failed to open file '{}': {}", file_name, e);
        e
    })?;

    let (tx, rx) = tokio::sync::mpsc::channel(4);

    let file_name_clone = file_name.clone();
    let mut file_clone = file.try_clone().await.map_err(|e| {
        eprintln!("Failed to clone file '{}': {}", file_name, e);
        e
    })?;

    tokio::spawn(async move {
        let file_info = FileInfo {
            file_name: file_name_clone,
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
        loop {
            let n = match file_clone.read(&mut buf).await {
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

    let response = client.upload(Request::new(ReceiverStream::new(rx))).await?;

    println!("Upload Response: {}", response.into_inner().message);

    Ok(())
}

async fn read_file(
    client: &mut MasterClient<tonic::transport::Channel>,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .read(Request::new(ReadRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    println!("File Content:\n{}", response.into_inner().content);

    Ok(())
}

async fn delete_file(
    client: &mut MasterClient<tonic::transport::Channel>,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .delete(Request::new(DeleteRequest {
            file_name: file_name.to_string(),
        }))
        .await?;

    println!("Delete Response: {}", response.into_inner().message);

    Ok(())
}
