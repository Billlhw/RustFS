use clap::{Arg, Command};
use std::net::SocketAddr;
use tonic::transport::Server;

use rustfs::config::load_config;
use rustfs::master_impl::determine_leader; // Import determine_leader from master_impl.rs
use rustfs::master_service::MasterService;
use rustfs::proto::master;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let matches = Command::new("MasterServer")
        .version("1.0")
        .about("Starts a MasterServer")
        .arg(
            Arg::new("address")
                .short('a')
                .value_name("ADDR")
                .help("Sets the address for the MasterServer (e.g., 127.0.0.1:50051)")
                .required(true),
        )
        .get_matches();

    let addr = matches
        .get_one::<String>("address")
        .expect("Address is required");

    // Load configuration
    let config = load_config("config.toml")?;
    let common_config = config.common;

    println!("MasterServer running at {}", addr);

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr.parse::<SocketAddr>()?)
        .await?;

    Ok(())
}
