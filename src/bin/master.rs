use clap::{Arg, Command};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;

use crate::master::PingMasterRequest;
use rustfs::config::load_config;
use rustfs::master_service::MasterService;
use rustfs::proto::master;
use tracing::{error, info};
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load logging config
    let config = load_config("config.toml")?;
    let log_path = config.master.log_path.clone();
    let log_level = config.common.log_level.clone();

    // Set up logger
    let file_appender = rolling::daily(log_path, "master.log");
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

    info!("MasterServer running at {}", addr);

    // Determine the leader
    let mut leader_found = false;
    let mut actural_master_addr = addr;
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
                            info!("Leader found at: {}", master_addr);
                            actural_master_addr = master_addr;
                            leader_found = true;
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to contact master at {}: {}", actural_master_addr, e)
                    }
                }
            }
            Err(e) => error!("Failed to connect to {}: {}", master_addr, e),
        }
    }

    let is_leader = !leader_found;

    let master_service = Arc::new(MasterService::new(
        &addr,
        config.master,
        common_config.clone(),
        is_leader,
        actural_master_addr,
    ));
    if is_leader {
        info!("No leader found. This node will act as the leader.");
        Arc::clone(&master_service).start_heartbeat_checker().await;
    } else {
        info!("This node is not the leader.");
        Arc::clone(&master_service)
            .start_shadow_master_ping_task()
            .await;
    }

    Server::builder()
        .add_service(master::master_server::MasterServer::new(master_service))
        .serve(addr.parse::<SocketAddr>()?)
        .await?;

    Ok(())
}
