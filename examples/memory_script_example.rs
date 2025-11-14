//! Example: Memory-based Lua Script with Read-Your-Writes Support
//!
//! This example demonstrates using MemoryScriptContext which supports
//! read-your-writes semantics. Scripts can read their own uncommitted writes.

use aidb::script::MemoryScriptContext;
use aidb::{Options, DB};
use std::sync::Arc;

fn main() -> Result<(), aidb::Error> {
    // Open the database
    let db = Arc::new(DB::open("./example_memory_script_db", Options::default())?);

    println!("=== Memory Script Context (Read-Your-Writes) Example ===\n");

    // Example 1: Counter with read-your-writes
    println!("Example 1: Counter Increment");
    println!("----------------------------");

    {
        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Initialize counter
        ctx.put(b"counter", b"0");

        // Read and increment (read-your-writes works!)
        for i in 1..=5 {
            let current = ctx.get(b"counter")?.unwrap();
            let count: i32 = String::from_utf8_lossy(&current).parse().unwrap();
            let new_count = count + 1;

            println!("  Iteration {}: {} -> {}", i, count, new_count);

            ctx.put(b"counter", new_count.to_string().as_bytes());
        }

        // Final value before commit
        let final_value = ctx.get(b"counter")?.unwrap();
        println!("  Final value in transaction: {}", String::from_utf8_lossy(&final_value));

        // Commit all operations atomically
        ctx.commit()?;
    }

    // Verify committed value
    let committed = db.get(b"counter")?.unwrap();
    println!("  Committed value in DB: {}\n", String::from_utf8_lossy(&committed));

    // Example 2: Complex State Management
    println!("Example 2: Shopping Cart with Read-Your-Writes");
    println!("----------------------------------------------");

    {
        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Add items to cart
        ctx.put(b"cart:item1:name", b"Laptop");
        ctx.put(b"cart:item1:price", b"1200");
        ctx.put(b"cart:item1:qty", b"1");

        ctx.put(b"cart:item2:name", b"Mouse");
        ctx.put(b"cart:item2:price", b"25");
        ctx.put(b"cart:item2:qty", b"2");

        // Calculate total (reading our own writes)
        let mut total = 0;
        for i in 1..=2 {
            let price_key = format!("cart:item{}:price", i);
            let qty_key = format!("cart:item{}:qty", i);
            let name_key = format!("cart:item{}:name", i);

            let name = ctx.get(name_key.as_bytes())?.unwrap();
            let price: i32 = String::from_utf8_lossy(&ctx.get(price_key.as_bytes())?.unwrap())
                .parse()
                .unwrap();
            let qty: i32 =
                String::from_utf8_lossy(&ctx.get(qty_key.as_bytes())?.unwrap()).parse().unwrap();

            let item_total = price * qty;
            total += item_total;

            println!(
                "  Item {}: {} x ${} = ${}",
                i,
                String::from_utf8_lossy(&name),
                qty,
                item_total
            );
        }

        println!("  Total: ${}", total);

        // Store the total
        ctx.put(b"cart:total", total.to_string().as_bytes());

        // Commit the entire cart
        ctx.commit()?;
    }

    println!("  Cart saved successfully!\n");

    // Example 3: Conditional Logic with Read-Your-Writes
    println!("Example 3: Inventory Management");
    println!("-------------------------------");

    // Setup initial inventory
    db.put(b"inventory:laptop", b"10")?;
    db.put(b"inventory:mouse", b"50")?;

    {
        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Try to reserve items
        let laptop_stock = ctx.get(b"inventory:laptop")?.unwrap();
        let laptop_count: i32 = String::from_utf8_lossy(&laptop_stock).parse().unwrap();

        if laptop_count >= 2 {
            println!("  Reserving 2 laptops...");
            ctx.put(b"inventory:laptop", (laptop_count - 2).to_string().as_bytes());
            ctx.put(b"order:123:laptop", b"2");

            // Read the updated inventory (read-your-writes)
            let new_stock = ctx.get(b"inventory:laptop")?.unwrap();
            println!("  Updated laptop stock: {}", String::from_utf8_lossy(&new_stock));
        } else {
            println!("  Insufficient laptop stock!");
        }

        let mouse_stock = ctx.get(b"inventory:mouse")?.unwrap();
        let mouse_count: i32 = String::from_utf8_lossy(&mouse_stock).parse().unwrap();

        if mouse_count >= 5 {
            println!("  Reserving 5 mice...");
            ctx.put(b"inventory:mouse", (mouse_count - 5).to_string().as_bytes());
            ctx.put(b"order:123:mouse", b"5");

            // Read the updated inventory (read-your-writes)
            let new_stock = ctx.get(b"inventory:mouse")?.unwrap();
            println!("  Updated mouse stock: {}", String::from_utf8_lossy(&new_stock));
        } else {
            println!("  Insufficient mouse stock!");
        }

        ctx.commit()?;
        println!("  Order 123 reserved successfully!\n");
    }

    // Example 4: Rollback on Error
    println!("Example 4: Transaction Rollback");
    println!("-------------------------------");

    {
        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        ctx.put(b"temp:value1", b"will be rolled back");
        ctx.put(b"temp:value2", b"also rolled back");

        // Verify we can read our writes
        assert!(ctx.get(b"temp:value1")?.is_some());
        println!("  Wrote temporary values...");

        // Explicit rollback (or just drop the context)
        ctx.rollback();
        println!("  Rolled back transaction");
    }

    // Verify rollback worked
    assert!(db.get(b"temp:value1")?.is_none());
    println!("  Verified: values were not committed\n");

    // Close the database
    db.close()?;

    println!("=== Example completed successfully ===");

    Ok(())
}
