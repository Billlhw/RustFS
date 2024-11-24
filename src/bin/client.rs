// src/bin/client.rs

use filesystem::file_system_client::FileSystemClient;
use filesystem::{DeleteRequest, FileChunk, FileInfo, ReadRequest, UpdateRequest, UploadRequest};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::wrappers::ReceiverStream;

pub mod filesystem {
    tonic::include_proto!("filesystem");
}

use std::io::{self, Write};
use tonic::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to the server once
    let mut client = FileSystemClient::connect("http://[::1]:50051").await?;

    // Interactive loop
    loop {
        // Display the prompt
        print!("Client> ");
        io::stdout().flush().unwrap();

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        // Exit if the user types "exit"
        if input.eq_ignore_ascii_case("exit") {
            println!("Exiting client.");
            break;
        }

        // Split the input into command and arguments
        let mut parts = input.splitn(3, ' ');
        let operation = parts.next();
        let file_name = parts.next();
        let additional_input = parts.next();

        match operation {
            Some("upload") => {
                if let Some(file_name) = file_name {
                    if let Err(e) = upload_file(&mut client, file_name.to_string()).await {
                        eprintln!("Error: {}", e);
                    }
                } else {
                    eprintln!("Usage: upload <file_name>");
                }
            }
            Some("read") => {
                if let Some(file_name) = file_name {
                    if let Err(e) = read_file(&mut client, file_name).await {
                        eprintln!("Error: {}", e);
                    }
                } else {
                    eprintln!("Usage: read <file_name>");
                }
            }
            Some("update") => {
                if let Some(file_name) = file_name {
                    if let Some(new_content) = additional_input {
                        if let Err(e) = update_file(&mut client, file_name, new_content).await {
                            eprintln!("Error: {}", e);
                        }
                    } else {
                        eprintln!("Usage: update <file_name> <new_content>");
                    }
                } else {
                    eprintln!("Usage: update <file_name> <new_content>");
                }
            }
            Some("delete") => {
                if let Some(file_name) = file_name {
                    if let Err(e) = delete_file(&mut client, file_name).await {
                        eprintln!("Error: {}", e);
                    }
                } else {
                    eprintln!("Usage: delete <file_name>");
                }
            }
            _ => {
                eprintln!(
                    "Invalid command. Available commands: upload, read, update, delete, exit"
                );
            }
        }
    }

    Ok(())
}

async fn upload_file(
    client: &mut FileSystemClient<tonic::transport::Channel>,
    file_name: String, // Changed from &str to String
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(&file_name).await.map_err(|e| {
        eprintln!("Failed to open file '{}': {}", file_name, e);
        e
    })?;

    let (tx, rx) = tokio::sync::mpsc::channel(4);

    // Clone necessary variables to move into the async task
    let file_name_clone = file_name.clone();
    let mut file_clone = file.try_clone().await.map_err(|e| {
        eprintln!("Failed to clone file '{}': {}", file_name, e);
        e
    })?;

    // Spawn a task to send the file data
    tokio::spawn(async move {
        let file_info = FileInfo {
            file_name: file_name_clone,
        };

        if let Err(e) = tx
            .send(UploadRequest {
                request: Some(filesystem::upload_request::Request::Info(file_info)),
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
                    request: Some(filesystem::upload_request::Request::Chunk(file_chunk)),
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
    client: &mut FileSystemClient<tonic::transport::Channel>,
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

async fn update_file(
    client: &mut FileSystemClient<tonic::transport::Channel>,
    file_name: &str,
    new_content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .update(Request::new(UpdateRequest {
            file_name: file_name.to_string(),
            new_content: new_content.as_bytes().to_vec(),
        }))
        .await?;

    println!("Update Response: {}", response.into_inner().message);

    Ok(())
}

async fn delete_file(
    client: &mut FileSystemClient<tonic::transport::Channel>,
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
