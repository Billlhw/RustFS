use clap::{Arg, Command};
use std::net::SocketAddr;
use tonic::transport::Server;

use rustfs::config::{load_config, ChunkServerConfig, CommonConfig};
use rustfs::proto::chunk::chunk_server::ChunkServer;
use rustfs::proto::master::RegisterRequest;
use rustfs::util::connect_to_master;
use tracing::{debug, error, info};
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

// Using modules chunkserver_impl and chunkserver_service from `src/`
use rustfs::chunkserver_service::ChunkService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load logging config
    let config = load_config("config.toml")?;
    let log_path = config.chunkserver.log_path.clone();
    let log_level = config.common.log_level.clone();

    // Set up logger
    let file_appender = rolling::daily(log_path, "chunkserver.log");
    let stdout_layer = fmt::layer().with_writer(std::io::stdout).with_ansi(true);
    let file_layer = fmt::layer().with_writer(file_appender).with_ansi(false); // Disable ANSI escape codes for file logs
    let env_filter = EnvFilter::from_default_env().add_directive(log_level.parse().unwrap());
    let subscriber = Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer);

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");

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
    let chunkserver_config: ChunkServerConfig = config.chunkserver;
    let common_config: CommonConfig = config.common;

    // Create path to data files of chunkserver
    let sanitized_address = addr.to_string().replace(':', "_"); // Convert to a valid directory name
    let full_data_path = format!("{}/{}", sanitized_address, chunkserver_config.data_path);
    if !std::path::Path::new(&full_data_path).exists() {
        std::fs::create_dir_all(&full_data_path).map_err(|e| {
            error!(
                "Failed to create data directory '{}': {}",
                full_data_path, e
            );
            e
        })?;
    }
    debug!("Data directory verified: {}", full_data_path);

    // Connect to the master, if no master is available, exit the program
    let mut master_client = connect_to_master(&common_config.master_addrs).await?;

    // Send register request to master
    let response = master_client
        .register_chunk_server(RegisterRequest {
            address: addr.to_string(),
        })
        .await?;
    info!("Registered with Master: {}", response.into_inner().message);

    // Start chunkserver service
    info!("ChunkServer running at {}", addr);
    let service = ChunkService::new(
        &addr.to_string(),
        &sanitized_address,
        chunkserver_config,
        common_config,
    );

    // Periodically cleanup outdated OTP
    service.start_otp_cleanup();

    // Clone the master_client and spawn the heartbeat task at background
    let heartbeat_client = master_client.clone();
    let heartbeat_service = service.clone();
    tokio::spawn(async move {
        if let Err(e) = heartbeat_service.send_heartbeat(heartbeat_client).await {
            error!("Heartbeat task failed: {}", e);
        }
    });

    Server::builder()
        .add_service(ChunkServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
