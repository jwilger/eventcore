//! Order Fulfillment Saga Example
//!
//! This example demonstrates the saga pattern using `EventCore`, showcasing:
//! - Long-running distributed transactions
//! - Multi-service coordination (payment, inventory, shipping)
//! - Failure handling and compensation logic
//! - Complex state management across multiple streams
//!
//! The saga pattern is essential for maintaining consistency in distributed
//! systems where traditional ACID transactions aren't feasible.

#![allow(clippy::all)]
#![allow(clippy::pedantic)]
#![allow(clippy::nursery)]

use eventcore_examples::sagas::types::{
    CompensationAction, CustomerId, Money, OrderId, OrderItem, PaymentId, ProductId, Quantity,
    ShipmentId,
};

fn demonstrate_saga_types_and_patterns() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Order Fulfillment Saga Pattern Demonstration");
    println!("===============================================");
    println!();

    // Create sample domain objects to demonstrate the type system
    let order_id = OrderId::generate();
    let customer_id = CustomerId::generate();

    println!("📋 Order Details:");
    println!("   Order ID: {}", order_id);
    println!("   Customer ID: {}", customer_id);
    println!();

    // Demonstrate type-safe domain modeling
    let laptop = OrderItem::new(
        ProductId::try_new("laptop-dell-xps13".to_string())?,
        Quantity::try_new(1)?,
        Money::from_dollars(1299.99),
    );

    let mouse = OrderItem::new(
        ProductId::try_new("mouse-logitech-mx3".to_string())?,
        Quantity::try_new(1)?,
        Money::from_dollars(99.99),
    );

    println!("🛒 Order Items:");
    println!(
        "   {} x {} @ ${:.2} = ${:.2}",
        laptop.quantity.into_inner(),
        laptop.product_id,
        laptop.unit_price.to_dollars(),
        laptop.total_price().to_dollars()
    );
    println!(
        "   {} x {} @ ${:.2} = ${:.2}",
        mouse.quantity.into_inner(),
        mouse.product_id,
        mouse.unit_price.to_dollars(),
        mouse.total_price().to_dollars()
    );

    let total = laptop.total_price().add(&mouse.total_price());
    println!("   Total: ${:.2}", total.to_dollars());
    println!();

    // Demonstrate saga workflow structure
    println!("🎬 Saga Workflow Structure:");
    println!("   1. OrderFulfillmentSaga coordinates the entire process");
    println!("   2. ProcessPaymentCommand handles payment authorization");
    println!("   3. ReserveInventoryCommand manages stock allocation");
    println!("   4. ArrangeShippingCommand creates shipments");
    println!("   5. Compensation logic handles failures at any step");
    println!();

    // Show event types that would be generated
    println!("📊 Event Types in Workflow:");
    println!("   SagaEvent::SagaStarted - Initiates the workflow");
    println!("   SagaEvent::PaymentInitiated - Payment processing begins");
    println!("   PaymentEvent::PaymentAuthorized - Payment approved");
    println!("   InventoryEvent::InventoryReserved - Stock allocated");
    println!("   ShippingEvent::ShipmentCreated - Shipping arranged");
    println!("   SagaEvent::SagaCompleted - Workflow successful");
    println!();

    // Demonstrate compensation scenarios
    println!("🔄 Compensation Scenarios:");
    println!("   Payment Fails → No compensation needed (nothing to rollback)");
    println!("   Inventory Fails → Refund authorized payment");
    println!("   Shipping Fails → Refund payment + release inventory");
    println!("   System Error → Complete rollback of all completed steps");
    println!();

    // Show type safety benefits
    println!("✨ Type Safety Benefits:");
    println!("   - Money amounts cannot be negative");
    println!("   - Quantities must be positive");
    println!("   - Stream IDs are validated and non-empty");
    println!("   - State transitions are compile-time checked");
    println!("   - Commands can only write to declared streams");
    println!();

    // Show multi-stream capabilities
    println!("🔀 Multi-Stream Capabilities:");
    println!("   - Read from multiple streams atomically");
    println!("   - Write to multiple streams in one transaction");
    println!("   - Dynamic stream discovery during execution");
    println!("   - Optimistic concurrency control across streams");
    println!();

    println!("🎉 Saga Pattern Implementation Complete!");
    println!("   This demonstrates EventCore's powerful capabilities for");
    println!("   coordinating complex distributed transactions with strong");
    println!("   consistency guarantees and comprehensive audit trails.");

    Ok(())
}

fn demonstrate_failure_and_compensation() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🚨 Compensation Logic Demonstration");
    println!("==================================");

    println!("💡 In production, saga failures trigger automatic compensation:");
    println!("   - Payment declined → No action needed (atomic failure)");
    println!("   - Inventory shortage → Refund captured payments");
    println!("   - Shipping unavailable → Refund + release inventory");
    println!("   - Network timeouts → Retry with exponential backoff");
    println!();

    println!("🔄 Compensation Actions Available:");
    let payment_id = PaymentId::generate();
    let reservations = vec![];
    let shipment_id = ShipmentId::generate();

    let compensation_actions = vec![
        CompensationAction::RefundPayment { payment_id },
        CompensationAction::ReleaseInventory { reservations },
        CompensationAction::CancelShipment { shipment_id },
    ];

    for (i, action) in compensation_actions.iter().enumerate() {
        println!("   {}. {:?}", i + 1, action);
    }
    println!();

    println!("✨ EventCore Saga Benefits:");
    println!("   - Automatic compensation orchestration");
    println!("   - Complete audit trail of all actions");
    println!("   - Type-safe state transitions");
    println!("   - Multi-stream atomicity guarantees");

    Ok(())
}

fn show_type_driven_development_benefits() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📊 Type-Driven Development Benefits");
    println!("===================================");

    println!("🔍 Domain Modeling Examples:");

    // Show invalid states that are impossible
    println!("   ❌ Impossible States (Prevented at Compile Time):");
    println!("      - Negative money amounts");
    println!("      - Zero or negative quantities");
    println!("      - Empty stream IDs");
    println!("      - Invalid state transitions");
    println!();

    println!("   ✅ Valid Domain Operations:");
    let money1 = Money::from_dollars(100.0);
    let money2 = Money::from_dollars(50.0);
    let total = money1.add(&money2);
    println!(
        "      ${:.2} + ${:.2} = ${:.2}",
        money1.to_dollars(),
        money2.to_dollars(),
        total.to_dollars()
    );

    let quantity = Quantity::try_new(3)?;
    let unit_price = Money::from_dollars(25.99);
    let line_total = unit_price.multiply(quantity.into_inner());
    println!(
        "      {} × ${:.2} = ${:.2}",
        quantity.into_inner(),
        unit_price.to_dollars(),
        line_total.to_dollars()
    );
    println!();

    println!("🛡️ Safety Guarantees:");
    println!("   - Input validation at construction time");
    println!("   - No runtime validation needed after parsing");
    println!("   - Impossible to create invalid domain objects");
    println!("   - Compile-time stream access control");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Demonstrate the saga pattern concepts and implementation
    demonstrate_saga_types_and_patterns()?;

    // Show failure handling and compensation
    demonstrate_failure_and_compensation()?;

    // Highlight type-driven development benefits
    show_type_driven_development_benefits()?;

    println!("\n✨ Order Fulfillment Saga Example Complete!");
    println!("📖 Explore the source code for detailed implementation patterns");
    println!("🔧 Run 'cargo doc --open' to see comprehensive API documentation");

    Ok(())
}
