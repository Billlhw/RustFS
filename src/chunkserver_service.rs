use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

use crate::config::{ChunkServerConfig, CommonConfig};
use crate::proto::master::{master_client::MasterClient, HeartbeatRequest};
use crate::util::connect_to_master;

#[derive(Clone, Debug, Default)]
pub struct ChunkService {
    pub addr: String,                               // Chunkserver address
    pub addr_sanitized: String,                     // Sanitized address, used for file directories
    pub server_chunks: Arc<Mutex<HashSet<String>>>, // Track metadata of all chunks stored
    pub config: ChunkServerConfig,
    pub common_config: CommonConfig,
}

impl ChunkService {
    pub fn new(
        addr: &str,
        addr_sanitized: &str,
        config: ChunkServerConfig,
        common_config: CommonConfig,
    ) -> Self {
        Self {
            server_chunks: Arc::new(Mutex::new(HashSet::new())),
            addr: addr.to_string(),
            addr_sanitized: addr_sanitized.to_string(),
            config,
            common_config,
        }
    }

    pub async fn send_heartbeat(
        &self,
        master_client: MasterClient<tonic::transport::Channel>, // Take ownership
    ) -> Result<(), Box<dyn std::error::Error>> {
        let interval_duration = Duration::from_secs(self.common_config.heartbeat_interval);
        let addr = self.addr.clone();
        let server_chunks = self.server_chunks.clone(); // Clone the Arc<Mutex<HashSet>> pointer
        let master_addrs = self.common_config.master_addrs.clone(); // Clone master_addrs to ensure 'static lifetime
        let mut first_time_reconnected = false;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);
            let mut client = master_client; // Move the owned client into the task

            loop {
                // Wait for interval seconds
                if !first_time_reconnected {
                    interval.tick().await;
                } else {
                    first_time_reconnected = false;
                }

                // Collect chunk information
                let chunks: Vec<String> = server_chunks
                    .lock()
                    .await
                    .iter()
                    .cloned() // Clone each String from the HashSet
                    .collect();

                // Create and send the heartbeat request
                let request = HeartbeatRequest {
                    chunkserver_address: addr.clone(),
                    chunks,
                };

                match client.heartbeat(tonic::Request::new(request)).await {
                    Ok(response) => {
                        println!(
                            "Heartbeat acknowledged by Master: {}",
                            response.into_inner().message
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to send heartbeat: {}", e);

                        // Attempt to reconnect to the master
                        match connect_to_master(&master_addrs).await {
                            Ok(new_client) => {
                                println!("Reconnected to Master");
                                client = new_client;
                                first_time_reconnected = true; // Avoid waiting for heartbeat_interval before retrying
                            }
                            Err(e) => {
                                eprintln!("Failed to reconnect to Master: {}", e);
                                break; // Exit the loop on repeated failures
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
