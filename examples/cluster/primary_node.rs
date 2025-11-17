//! Example: Running a Primary node
//!
//! This example demonstrates how to start a Primary node that serves
//! the full database via RPC.
//!
//! Usage:
//!   cargo run --example primary_node --features cluster

use aidb::cluster::PrimaryNode;
use aidb::{DB, Options};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    println!("Starting Primary node...");
    
    // Create or open database
    let options = Options::default();
    let db = DB::open("./data/primary", options)?;
    let db = Arc::new(db);
    
    // Insert some initial data
    db.put(b"hello", b"world")?;
    db.put(b"foo", b"bar")?;
    println!("Inserted initial data");
    
    // Create primary node
    let primary = PrimaryNode::new(db.clone());
    println!("Primary node created");
    
    // Start RPC server
    let addr = "127.0.0.1:50051".parse()?;
    println!("Primary node listening on {}", addr);
    println!("Press Ctrl+C to stop");
    
    primary.serve(addr).await?;
    
    Ok(())
}
