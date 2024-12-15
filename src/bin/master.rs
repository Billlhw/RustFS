use clap::{Arg, Command};
use std::net::SocketAddr;
use tonic::transport::Server;

use crate::master::PingMasterRequest;
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

    // Determine the leader
    let mut leader_found = false;
    for master_addr in &common_config.master_addrs {
        if master_addr == addr {
            continue; // Skip pinging itself
        }

        let target_url = format!("http://{}", master_addr);
        match rustfs::proto::master::master_client::MasterClient::connect(target_url).await {
            Ok(mut client) => {
                let request = tonic::Request::new(PingMasterRequest {
                    sender_address: addr.clone(),
                });

                match client.ping_master(request).await {
                    Ok(response) => {
                        let response = response.into_inner();
                        if response.is_leader {
                            println!("Leader found at: {}", master_addr);
                            leader_found = true;
                            break;
                        }
                    }
                    Err(e) => eprintln!("Failed to contact master at {}: {}", master_addr, e),
                }
            }
            Err(e) => eprintln!("Failed to connect to {}: {}", master_addr, e),
        }
    }

    let is_leader = !leader_found;
    if is_leader {
        println!("No leader found. This node will act as the leader.");
    } else {
        println!("This node is not the leader.");
    }

    let master_service = MasterService::new(&addr, config.master, common_config);

    if is_leader {
        master_service.start_heartbeat_checker().await;
    } else {
        // TODO: save the leader address and periodically ping the leader to check if it is available
    }

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr.parse::<SocketAddr>()?)
        .await?;

    Ok(())
}
