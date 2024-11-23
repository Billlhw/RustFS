use filesystem::file_system_client::FileSystemClient;
use filesystem::{DeleteRequest, FileChunk, FileInfo, GetRequest, UpdateRequest, UploadRequest};
// use tonic::transport::Channel;

use tokio::fs::File;
use tokio::io::AsyncReadExt;

pub mod filesystem {
    tonic::include_proto!("filesystem");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = FileSystemClient::connect("http://[::1]:50051").await?;

    // Example: Upload a file
    let file_name = "example.txt";
    let mut file = File::open(file_name).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(4);

    tokio::spawn(async move {
        let file_info = FileInfo {
            file_name: file_name.to_string(),
        };

        tx.send(UploadRequest {
            request: Some(filesystem::upload_request::Request::Info(file_info)),
        })
        .await
        .unwrap();

        let mut buf = [0; 1024];
        while let Ok(n) = file.read(&mut buf).await {
            if n == 0 {
                break;
            }

            let file_chunk = FileChunk {
                data: buf[..n].to_vec(),
            };

            tx.send(UploadRequest {
                request: Some(filesystem::upload_request::Request::Chunk(file_chunk)),
            })
            .await
            .unwrap();
        }
    });

    let response = client
        .upload(tonic::Request::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
        .await?;
    println!("Upload Response: {:?}", response);

    // Example: Get file content
    let get_response = client
        .get(tonic::Request::new(GetRequest {
            file_name: file_name.to_string(),
        }))
        .await?;
    println!("File Content: {:?}", get_response.into_inner().content);

    // Example: Update the file
    let update_response = client
        .update(tonic::Request::new(UpdateRequest {
            file_name: file_name.to_string(),
            new_content: b"Updated content".to_vec(),
        }))
        .await?;
    println!(
        "Update Response: {:?}",
        update_response.into_inner().message
    );

    // Example: Delete the file
    let delete_response = client
        .delete(tonic::Request::new(DeleteRequest {
            file_name: file_name.to_string(),
        }))
        .await?;
    println!(
        "Delete Response: {:?}",
        delete_response.into_inner().message
    );

    Ok(())
}
