//! Example: Lua Script with Automatic Rollback
//!
//! This example demonstrates how to use Lua scripting with automatic rollback
//! in AiDb. Scripts that fail will automatically rollback all changes.

use aidb::{DB, Options};
use std::sync::Arc;

fn main() -> Result<(), aidb::Error> {
    // Open the database
    let db = Arc::new(DB::open("./example_script_db", Options::default())?);

    println!("=== Lua Scripting with Automatic Rollback Example ===\n");

    // Example 1: Successful script (all changes committed)
    println!("Example 1: Successful transfer");
    println!("-----------------------------------");
    
    // Initialize accounts
    db.put(b"account:alice:balance", b"1000")?;
    db.put(b"account:bob:balance", b"500")?;
    
    println!("Initial balances:");
    println!("  Alice: {} credits", String::from_utf8_lossy(&db.get(b"account:alice:balance")?.unwrap()));
    println!("  Bob:   {} credits", String::from_utf8_lossy(&db.get(b"account:bob:balance")?.unwrap()));
    
    // Transfer 200 credits from Alice to Bob
    let script = r#"
        -- Read current balances
        local alice_balance = tonumber(db.get("account:alice:balance"))
        local bob_balance = tonumber(db.get("account:bob:balance"))
        
        -- Check if Alice has sufficient balance
        if alice_balance < 200 then
            error("Insufficient balance")
        end
        
        -- Perform transfer
        db.put("account:alice:balance", tostring(alice_balance - 200))
        db.put("account:bob:balance", tostring(bob_balance + 200))
        
        return "Transfer successful"
    "#;
    
    match db.execute_script_with_result(script) {
        Ok(result) => {
            println!("\nScript result: {:?}", result);
            println!("\nAfter transfer:");
            println!("  Alice: {} credits", String::from_utf8_lossy(&db.get(b"account:alice:balance")?.unwrap()));
            println!("  Bob:   {} credits", String::from_utf8_lossy(&db.get(b"account:bob:balance")?.unwrap()));
        }
        Err(e) => println!("Script failed: {}", e),
    }
    
    // Example 2: Failed script (all changes rolled back)
    println!("\n\nExample 2: Failed transfer (insufficient balance)");
    println!("---------------------------------------------------");
    
    println!("Before attempt:");
    println!("  Alice: {} credits", String::from_utf8_lossy(&db.get(b"account:alice:balance")?.unwrap()));
    println!("  Bob:   {} credits", String::from_utf8_lossy(&db.get(b"account:bob:balance")?.unwrap()));
    
    // Try to transfer 1000 credits (will fail)
    let script = r#"
        local alice_balance = tonumber(db.get("account:alice:balance"))
        local bob_balance = tonumber(db.get("account:bob:balance"))
        
        if alice_balance < 1000 then
            error("Insufficient balance - cannot transfer 1000 credits")
        end
        
        db.put("account:alice:balance", tostring(alice_balance - 1000))
        db.put("account:bob:balance", tostring(bob_balance + 1000))
        
        return "Transfer successful"
    "#;
    
    match db.execute_script_with_result(script) {
        Ok(result) => println!("\nScript result: {:?}", result),
        Err(e) => println!("\nScript failed (as expected): {}", e),
    }
    
    println!("\nAfter failed attempt (balances unchanged due to rollback):");
    println!("  Alice: {} credits", String::from_utf8_lossy(&db.get(b"account:alice:balance")?.unwrap()));
    println!("  Bob:   {} credits", String::from_utf8_lossy(&db.get(b"account:bob:balance")?.unwrap()));
    
    // Example 3: Complex script with multiple operations
    println!("\n\nExample 3: Batch update with validation");
    println!("------------------------------------------");
    
    let script = r#"
        -- Initialize order items
        db.put("order:1:item", "laptop")
        db.put("order:1:price", "1200")
        db.put("order:2:item", "mouse")
        db.put("order:2:price", "25")
        db.put("order:3:item", "keyboard")
        db.put("order:3:price", "80")
        
        -- Calculate total
        local total = 0
        for i = 1, 3 do
            local price = tonumber(db.get("order:" .. i .. ":price"))
            total = total + price
        end
        
        db.put("order:total", tostring(total))
        
        return "Total: $" .. total
    "#;
    
    match db.execute_script_with_result(script) {
        Ok(result) => {
            println!("Script result: {:?}", result);
            println!("\nOrder details:");
            for i in 1..=3 {
                let item_key = format!("order:{}:item", i);
                let price_key = format!("order:{}:price", i);
                let item_bytes = db.get(item_key.as_bytes())?.unwrap();
                let price_bytes = db.get(price_key.as_bytes())?.unwrap();
                let item = String::from_utf8_lossy(&item_bytes);
                let price = String::from_utf8_lossy(&price_bytes);
                println!("  Order {}: {} - ${}", i, item, price);
            }
            let total_bytes = db.get(b"order:total")?.unwrap();
            let total = String::from_utf8_lossy(&total_bytes);
            println!("  Total: ${}", total);
        }
        Err(e) => println!("Script failed: {}", e),
    }
    
    // Close the database
    db.close()?;
    
    println!("\n=== Example completed successfully ===");
    
    Ok(())
}
