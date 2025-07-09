//! Example demonstrating type-safe subscription lifecycle management.
//!
//! This example shows how to use the typed subscription API that provides
//! compile-time guarantees about subscription state transitions.

use async_trait::async_trait;
use eventcore::{
    EventProcessor, StoredEvent, SubscriptionName, SubscriptionOptions, SubscriptionResult,
    TypedSubscription,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Example domain event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum OrderEvent {
    Created {
        id: String,
        customer: String,
    },
    ItemAdded {
        id: String,
        item: String,
        quantity: u32,
    },
    Shipped {
        id: String,
        tracking: String,
    },
}

/// Event processor that builds order statistics
struct OrderStatsProcessor {
    stats: Arc<Mutex<OrderStats>>,
}

#[derive(Debug, Default)]
struct OrderStats {
    total_orders: usize,
    total_items: usize,
    shipped_orders: usize,
}

impl OrderStatsProcessor {
    fn new() -> (Self, Arc<Mutex<OrderStats>>) {
        let stats = Arc::new(Mutex::new(OrderStats::default()));
        (
            Self {
                stats: Arc::clone(&stats),
            },
            stats,
        )
    }
}

#[async_trait]
impl EventProcessor for OrderStatsProcessor {
    type Event = OrderEvent;

    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()> {
        {
            let mut stats = self.stats.lock().unwrap();

            match &event.payload {
                OrderEvent::Created { .. } => {
                    stats.total_orders += 1;
                    println!("📦 Order created - Total orders: {}", stats.total_orders);
                }
                OrderEvent::ItemAdded { quantity, .. } => {
                    stats.total_items += *quantity as usize;
                    println!("🛒 Items added - Total items: {}", stats.total_items);
                }
                OrderEvent::Shipped { .. } => {
                    stats.shipped_orders += 1;
                    println!(
                        "🚚 Order shipped - Shipped orders: {}",
                        stats.shipped_orders
                    );
                }
            }
        }

        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        println!("✅ Subscription caught up to live position!");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Type-Safe Subscription Example\n");

    // Create event store
    let event_store = InMemoryEventStore::<OrderEvent>::new();

    // Step 1: Create uninitialized subscription
    println!("1️⃣ Creating uninitialized subscription...");
    let subscription = TypedSubscription::new(event_store);

    // Step 2: Configure the subscription
    println!("2️⃣ Configuring subscription...");
    let name = SubscriptionName::try_new("order-stats-processor")?;
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let (processor, stats) = OrderStatsProcessor::new();

    let configured = subscription.configure(name, options, Box::new(processor));

    // Step 3: Start the subscription
    println!("3️⃣ Starting subscription...");
    let running = match configured.start().await {
        Ok(running) => {
            println!("   ✓ Subscription started successfully!");
            running
        }
        Err(e) => {
            eprintln!("   ✗ Failed to start subscription: {e}");
            return Err(e.into());
        }
    };

    // Step 4: Demonstrate pause/resume
    println!("\n4️⃣ Pausing subscription...");
    let paused = running.pause().await?;
    println!("   ✓ Subscription paused");

    // Check stats while paused
    {
        let current_stats = stats.lock().unwrap();
        println!("\n📊 Current Statistics:");
        println!("   Total Orders: {}", current_stats.total_orders);
        println!("   Total Items: {}", current_stats.total_items);
        println!("   Shipped Orders: {}", current_stats.shipped_orders);
    }

    println!("\n5️⃣ Resuming subscription...");
    let running_again = paused.resume().await?;
    println!("   ✓ Subscription resumed");

    // Step 5: Stop the subscription
    println!("\n6️⃣ Stopping subscription...");
    let stopped = running_again.stop().await?;
    println!("   ✓ Subscription stopped");

    // Access final information from stopped subscription
    if let Some(name) = stopped.name() {
        println!("\n📝 Stopped subscription: {name:?}");
    }

    println!("\n✨ Example completed successfully!");

    // Demonstrate compile-time safety
    demonstrate_type_safety();

    Ok(())
}

/// This function demonstrates the compile-time safety of typed subscriptions
fn demonstrate_type_safety() {
    println!("\n🔒 Type Safety Demonstration:");
    println!("   The following operations would NOT compile:");
    println!("   - Starting an uninitialized subscription");
    println!("   - Pausing a stopped subscription");
    println!("   - Processing events on a paused subscription");
    println!("   - Configuring an already running subscription");

    // These would cause compile errors:
    // subscription.start().await;  // ERROR: start() not available on Uninitialized
    // stopped.pause().await;       // ERROR: pause() not available on Stopped
    // paused.process_events().await; // ERROR: process_events() not available on Paused
}
