use crate::proto::master::master_client::MasterClient;

/// Connect to the master given the list of servers
pub async fn connect_to_master(
    master_addrs: &Vec<String>,
) -> Result<MasterClient<tonic::transport::Channel>, Box<dyn std::error::Error>> {
    for addr in master_addrs {
        match MasterClient::connect(format!("http://{}", addr)).await {
            Ok(client) => {
                println!("Connected to master at: {}", addr);
                return Ok(client); // Return the MasterClient directly
            }
            Err(e) => {
                println!("Failed to connect to master at {}: {}", addr, e);
            }
        }
    }

    // If none of the addresses are connectable, return an error
    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No master servers are connectable",
    )))
}
