use clap::{Arg, Command};
use std::net::SocketAddr;
use tonic::transport::Server;

use rustfs::config::{load_config, ChunkServerConfig, CommonConfig};
use rustfs::proto::chunk::chunk_server::ChunkServer;
use rustfs::proto::master::RegisterRequest;
use rustfs::util::connect_to_master;

// Using modules chunkserver_impl and chunkserver_service from `src/`
use rustfs::chunkserver_service::ChunkService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let matches = Command::new("ChunkServer")
        .version("1.0")
        .about("Starts a ChunkServer")
        .arg(
            Arg::new("address")
                .short('a')
                .value_name("ADDR")
                .help("Sets the address for the ChunkServer (e.g., 127.0.0.1:50010)")
                .required(true),
        )
        .get_matches();

    let address = matches
        .get_one::<String>("address")
        .expect("Address is required");
    let addr: SocketAddr = address.parse().expect("Invalid address format");

    // Load configuration
    let config = load_config("config.toml")?;
    let chunkserver_config: ChunkServerConfig = config.chunkserver;
    let common_config: CommonConfig = config.common;

    // Create path to data files of chunkserver
    let sanitized_address = addr.to_string().replace(':', "_"); // Convert to a valid directory name
    let full_data_path = format!("{}/{}", sanitized_address, chunkserver_config.data_path);
    if !std::path::Path::new(&full_data_path).exists() {
        std::fs::create_dir_all(&full_data_path).map_err(|e| {
            eprintln!(
                "Failed to create data directory '{}': {}",
                full_data_path, e
            );
            e
        })?;
    }
    println!("Data directory verified: {}", full_data_path);

    // Connect to the master, if no master is available, exit the program
    let mut master_client = connect_to_master(&common_config.master_addrs).await?;

    // Send register request to master
    let response = master_client
        .register_chunk_server(RegisterRequest {
            address: addr.to_string(),
        })
        .await?;
    println!("Registered with Master: {}", response.into_inner().message);

    // Start chunkserver service
    println!("ChunkServer running at {}", addr);
    let service = ChunkService::new(
        &addr.to_string(),
        &sanitized_address,
        chunkserver_config,
        common_config,
    );

    // Clone the master_client and spawn the heartbeat task at background
    let heartbeat_client = master_client.clone();
    let heartbeat_service = service.clone();
    tokio::spawn(async move {
        if let Err(e) = heartbeat_service.send_heartbeat(heartbeat_client).await {
            eprintln!("Heartbeat task failed: {}", e);
        }
    });

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
