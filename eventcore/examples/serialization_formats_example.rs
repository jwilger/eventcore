//! Example demonstrating different serialization formats in `EventCore`
//!
//! This example shows how to configure and use different serialization
//! formats (JSON, `MessagePack`, Bincode) for event storage.
#![allow(clippy::similar_names)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]

use eventcore::serialization::{EventSerializer, SerializationFormat};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OrderEvent {
    order_id: String,
    customer_id: String,
    items: Vec<OrderItem>,
    total_amount: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OrderItem {
    product_id: String,
    quantity: u32,
    price: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create sample order event
    let order_event = OrderEvent {
        order_id: "ORDER-12345".to_string(),
        customer_id: "CUST-98765".to_string(),
        items: vec![
            OrderItem {
                product_id: "PROD-001".to_string(),
                quantity: 2,
                price: 2999,
            },
            OrderItem {
                product_id: "PROD-002".to_string(),
                quantity: 1,
                price: 4999,
            },
        ],
        total_amount: 10997,
    };

    // Compare serialization formats
    println!("=== Serialization Format Comparison ===\n");

    compare_format(SerializationFormat::Json, &order_event).await?;
    compare_format(SerializationFormat::MessagePack, &order_event).await?;
    compare_format(SerializationFormat::Bincode, &order_event).await?;

    // Demonstrate configuration
    println!("\n=== Configuration Example ===\n");
    demonstrate_configuration();

    // Performance comparison
    println!("\n=== Performance Comparison ===\n");
    benchmark_formats(&order_event).await?;

    Ok(())
}

async fn compare_format(
    format: SerializationFormat,
    event: &OrderEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    let serializer = format.create_serializer();

    // Serialize
    let start = Instant::now();
    let serialized = serializer.serialize(event, "OrderEvent").await?;
    let serialize_time = start.elapsed();

    // Deserialize
    let start = Instant::now();
    let _deserialized: OrderEvent = serializer.deserialize(&serialized, "OrderEvent").await?;
    let deserialize_time = start.elapsed();

    println!("{} Format:", format);
    println!("  Size: {} bytes", serialized.len());
    println!("  Serialize time: {:?}", serialize_time);
    println!("  Deserialize time: {:?}", deserialize_time);
    println!("  MIME type: {}", format.mime_type());
    println!("  File extension: .{}", format.file_extension());

    // Show first few bytes for visualization
    let preview = &serialized[..serialized.len().min(50)];
    println!("  Preview: {:?}...", preview);
    println!();

    Ok(())
}

async fn benchmark_formats(event: &OrderEvent) -> Result<(), Box<dyn std::error::Error>> {
    const ITERATIONS: usize = 10000;

    for format in [
        SerializationFormat::Json,
        SerializationFormat::MessagePack,
        SerializationFormat::Bincode,
    ] {
        let serializer = format.create_serializer();

        // Benchmark serialization
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = serializer.serialize(event, "OrderEvent").await?;
        }
        let total_serialize = start.elapsed();

        // Get one serialized copy for deserialization benchmark
        let serialized = serializer.serialize(event, "OrderEvent").await?;

        // Benchmark deserialization
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _: OrderEvent = serializer.deserialize(&serialized, "OrderEvent").await?;
        }
        let total_deserialize = start.elapsed();

        println!("{} Performance ({} iterations):", format, ITERATIONS);
        println!("  Total serialize time: {:?}", total_serialize);
        println!(
            "  Avg serialize time: {:?}",
            total_serialize / ITERATIONS as u32
        );
        println!("  Total deserialize time: {:?}", total_deserialize);
        println!(
            "  Avg deserialize time: {:?}",
            total_deserialize / ITERATIONS as u32
        );
        println!();
    }

    Ok(())
}

fn demonstrate_configuration() {
    // Example: Choosing format based on requirements
    let format = if std::env::var("OPTIMIZE_FOR_SIZE").is_ok() {
        SerializationFormat::MessagePack // Best compression
    } else if std::env::var("OPTIMIZE_FOR_SPEED").is_ok() {
        SerializationFormat::Bincode // Fastest
    } else {
        SerializationFormat::Json // Default, human-readable
    };

    println!("Selected format: {}", format);
}
