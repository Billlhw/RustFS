use clap::{Arg, Command};
use std::net::SocketAddr;

use tonic::transport::Server;

use rustfs::config::{load_config, CommonConfig, MasterConfig};
use rustfs::master_service::MasterService;
use rustfs::proto::master;

// Using modules master_impl and master_service from `src/`
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

    let addr = matches
        .get_one::<String>("address")
        .expect("Address is required");

    // Load configuration
    let config = load_config("config.toml")?;
    let master_config: MasterConfig = config.master;
    let common_config: CommonConfig = config.common;

    // Create the server address
    println!("MasterServer running at {}", addr);
    let master_service = MasterService::new(addr, master_config, common_config);
    let addr_socket: SocketAddr = addr.parse()?;

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr_socket)
        .await?;

    Ok(())
}
