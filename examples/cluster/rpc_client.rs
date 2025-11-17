//! Example: Using RPC client directly
//!
//! This example shows how to use the generated gRPC client to
//! interact with a Primary node.
//!
//! Start a Primary node first, then run:
//!   cargo run --example rpc_client --features cluster

use aidb::cluster::rpc::proto::{
    storage_client::StorageClient,
    GetRequest, PutRequest, DeleteRequest, ScanRequest,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    println!("Connecting to Primary node...");
    
    // Connect to primary
    let mut client = StorageClient::connect("http://127.0.0.1:50051").await?;
    println!("Connected!\n");
    
    // PUT operation
    println!("=== PUT Operation ===");
    let request = tonic::Request::new(PutRequest {
        key: b"example_key".to_vec(),
        value: b"example_value".to_vec(),
    });
    
    let response = client.put(request).await?;
    println!("Put response: {:?}\n", response.into_inner());
    
    // GET operation
    println!("=== GET Operation ===");
    let request = tonic::Request::new(GetRequest {
        key: b"example_key".to_vec(),
    });
    
    let response = client.get(request).await?;
    let resp = response.into_inner();
    println!("Get response - Found: {}", resp.found);
    if resp.found {
        println!("Value: {:?}\n", String::from_utf8_lossy(&resp.value));
    }
    
    // SCAN operation
    println!("=== SCAN Operation ===");
    let request = tonic::Request::new(ScanRequest {
        start_key: vec![],
        end_key: vec![],
        limit: 10,
    });
    
    let mut stream = client.scan(request).await?.into_inner();
    println!("Scanning first 10 keys:");
    
    while let Some(item) = stream.message().await? {
        println!("  {:?} => {:?}",
            String::from_utf8_lossy(&item.key),
            String::from_utf8_lossy(&item.value)
        );
    }
    
    // DELETE operation
    println!("\n=== DELETE Operation ===");
    let request = tonic::Request::new(DeleteRequest {
        key: b"example_key".to_vec(),
    });
    
    let response = client.delete(request).await?;
    println!("Delete response: {:?}\n", response.into_inner());
    
    // Verify deletion
    println!("=== Verify Deletion ===");
    let request = tonic::Request::new(GetRequest {
        key: b"example_key".to_vec(),
    });
    
    let response = client.get(request).await?;
    let resp = response.into_inner();
    println!("Get after delete - Found: {}", resp.found);
    
    Ok(())
}
