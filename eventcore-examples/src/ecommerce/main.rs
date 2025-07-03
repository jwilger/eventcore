//! E-commerce example application
//!
//! This example demonstrates a complete e-commerce order workflow with:
//! - Product catalog management
//! - Order creation and management
//! - Inventory tracking with automatic reservation
//! - Projections for analytics and reporting

#![allow(clippy::too_many_lines)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::uninlined_format_args)]

use anyhow::Result;
use eventcore::{CommandExecutor, Event, EventStore, ExecutionOptions, Projection, ReadOptions};
use eventcore_examples::ecommerce::{
    commands::*,
    events::EcommerceEvent,
    projections::{InventoryProjectionImpl, OrderSummaryProjectionImpl},
    types::*,
};
use eventcore_postgres::PostgresEventStore;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting e-commerce example");

    // Create a PostgreSQL event store
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());
    let config = eventcore_postgres::PostgresConfig::new(database_url);
    let event_store: PostgresEventStore<EcommerceEvent> = PostgresEventStore::new(config).await?;

    // Initialize the database schema
    event_store.initialize().await?;

    // Create command executor
    let executor = CommandExecutor::new(event_store.clone());

    // === Product Catalog Setup ===
    info!("Setting up product catalog");

    // Add some products to the catalog
    let laptop = Product::new(
        ProductId::try_new("PRD-LAPTOP01".to_string())?,
        Sku::try_new("LAPTOP-15-1TB".to_string())?,
        ProductName::try_new("Gaming Laptop 15-inch".to_string())?,
        Money::from_cents(149_999)?, // $1,499.99
        Some("High-performance gaming laptop with 15-inch display and 1TB SSD".to_string()),
    );

    let mouse = Product::new(
        ProductId::try_new("PRD-MOUSE01".to_string())?,
        Sku::try_new("MOUSE-WIRELESS".to_string())?,
        ProductName::try_new("Wireless Gaming Mouse".to_string())?,
        Money::from_cents(7999)?, // $79.99
        Some("Wireless gaming mouse with RGB lighting".to_string()),
    );

    let keyboard = Product::new(
        ProductId::try_new("PRD-KEYBOARD01".to_string())?,
        Sku::try_new("KEYBOARD-MECH".to_string())?,
        ProductName::try_new("Mechanical Keyboard".to_string())?,
        Money::from_cents(12999)?, // $129.99
        Some("Mechanical keyboard with blue switches".to_string()),
    );

    // Create a catalog stream for this example
    let catalog_stream = eventcore::StreamId::try_new("product-catalog".to_string())?;

    // Add products to catalog
    executor
        .execute(
            AddProductCommand::new(laptop.clone(), Quantity::new(5)?, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;
    executor
        .execute(
            AddProductCommand::new(mouse.clone(), Quantity::new(25)?, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;
    executor
        .execute(
            AddProductCommand::new(keyboard.clone(), Quantity::new(15)?, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    info!("Added {} products to catalog", 3);

    // === Order Workflow ===
    info!("Demonstrating order workflow");

    // Create customer
    let customer = Customer::new(
        CustomerEmail::try_new("alice@example.com".to_string())?,
        "Alice Johnson".to_string(),
        Some("123 Tech Street, Silicon Valley, CA 94000".to_string()),
    );

    // Create order
    let order_id = OrderId::generate();
    info!("Creating order {}", order_id);

    executor
        .execute(
            CreateOrderCommand::new(order_id.clone(), customer.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    // Add items to order
    info!("Adding items to order");

    let laptop_item = OrderItem::new(laptop.id.clone(), Quantity::new(1)?, laptop.price);

    let mouse_item = OrderItem::new(mouse.id.clone(), Quantity::new(2)?, mouse.price);

    executor
        .execute(
            AddItemToOrderCommand::new(order_id.clone(), laptop_item, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;
    executor
        .execute(
            AddItemToOrderCommand::new(order_id.clone(), mouse_item, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    // Place the order
    info!("Placing order {}", order_id);
    executor
        .execute(
            PlaceOrderCommand::new(order_id.clone(), catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    // === Second Order (Different Customer) ===
    info!("Creating second order");

    let customer2 = Customer::new(
        CustomerEmail::try_new("bob@example.com".to_string())?,
        "Bob Smith".to_string(),
        Some("456 Developer Lane, Austin, TX 78701".to_string()),
    );

    let order_id2 = OrderId::generate();
    executor
        .execute(
            CreateOrderCommand::new(order_id2.clone(), customer2),
            ExecutionOptions::default(),
        )
        .await?;

    let keyboard_item = OrderItem::new(keyboard.id.clone(), Quantity::new(1)?, keyboard.price);

    executor
        .execute(
            AddItemToOrderCommand::new(order_id2.clone(), keyboard_item, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;
    executor
        .execute(
            PlaceOrderCommand::new(order_id2.clone(), catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    // === Third Order (Test Cancellation) ===
    info!("Creating third order to demonstrate cancellation");

    let customer3 = Customer::new(
        CustomerEmail::try_new("charlie@example.com".to_string())?,
        "Charlie Brown".to_string(),
        Some("789 Startup Blvd, Seattle, WA 98101".to_string()),
    );

    let order_id3 = OrderId::generate();
    executor
        .execute(
            CreateOrderCommand::new(order_id3.clone(), customer3),
            ExecutionOptions::default(),
        )
        .await?;

    let laptop_item3 = OrderItem::new(laptop.id.clone(), Quantity::new(1)?, laptop.price);

    executor
        .execute(
            AddItemToOrderCommand::new(order_id3.clone(), laptop_item3, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await?;

    // Cancel this order
    info!("Cancelling order {}", order_id3);
    executor
        .execute(
            CancelOrderCommand::new(
                order_id3,
                "Customer changed mind".to_string(),
                catalog_stream.clone(),
            ),
            ExecutionOptions::default(),
        )
        .await?;

    // === Projections and Analytics ===
    info!("Building projections and generating reports");

    // Create projections
    let inventory_projection = InventoryProjectionImpl::new();
    let order_summary_projection = OrderSummaryProjectionImpl::new();

    let mut inventory_state = inventory_projection.initialize_state().await?;
    let mut order_summary_state = order_summary_projection.initialize_state().await?;

    // Read all events and apply to projections
    let all_streams = vec![
        catalog_stream.clone(),
        eventcore::StreamId::try_new(format!("product-{}", laptop.id))?,
        eventcore::StreamId::try_new(format!("product-{}", mouse.id))?,
        eventcore::StreamId::try_new(format!("product-{}", keyboard.id))?,
        eventcore::StreamId::try_new(format!("order-{order_id}"))?,
        eventcore::StreamId::try_new(format!("order-{order_id2}"))?,
    ];

    let stream_data = event_store
        .read_streams(&all_streams, &ReadOptions::default())
        .await?;

    // Apply events to projections
    for stored_event in stream_data.events() {
        let event = Event::new(
            stored_event.stream_id.clone(),
            stored_event.payload.clone(),
            stored_event.metadata.clone().unwrap_or_default(),
        );

        inventory_projection
            .apply_event(&mut inventory_state, &event)
            .await?;
        order_summary_projection
            .apply_event(&mut order_summary_state, &event)
            .await?;
    }

    // Display inventory report
    info!("=== INVENTORY REPORT ===");
    info!(
        "Total products in catalog: {}",
        inventory_state.total_products
    );
    info!(
        "Total inventory value: {}",
        inventory_state.total_inventory_value
    );

    info!("Current inventory levels:");
    for (product, quantity) in inventory_state.get_products_with_inventory() {
        info!(
            "  {} ({}): {} units @ {} each",
            product.name, product.sku, quantity, product.price
        );
    }

    // Check for low inventory
    let low_inventory = inventory_state.get_low_inventory_products(10);
    if !low_inventory.is_empty() {
        info!("⚠️  Products with low inventory (< 10 units):");
        for (product, quantity) in low_inventory {
            info!("  {} ({}): {} units", product.name, product.sku, quantity);
        }
    }

    // Display order summary report
    info!("=== ORDER SUMMARY REPORT ===");
    info!("Total orders created: {}", order_summary_state.total_orders);
    info!("Total revenue: {}", order_summary_state.total_revenue);
    info!(
        "Average order value: {}",
        order_summary_state.average_order_value
    );
    info!(
        "Total customers: {}",
        order_summary_state.get_total_customers()
    );

    info!("Orders by status:");
    for status in [
        OrderStatus::Draft,
        OrderStatus::Placed,
        OrderStatus::Shipped,
        OrderStatus::Delivered,
        OrderStatus::Cancelled,
    ] {
        let count = order_summary_state.get_orders_count_by_status(&status);
        if count > 0 {
            info!("  {}: {}", status, count);
        }
    }

    // === Test Invalid Operations ===
    info!("=== TESTING BUSINESS RULE VALIDATION ===");

    // Try to add duplicate product
    info!("Testing duplicate product addition (should fail)");
    if let Err(e) = executor
        .execute(
            AddProductCommand,
            AddProductInput::new(laptop.clone(), Quantity::new(5)?, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await
    {
        info!("✓ Correctly rejected duplicate product: {}", e);
    } else {
        info!("✗ Unexpectedly allowed duplicate product");
    }

    // Try to add item with insufficient inventory
    info!("Testing insufficient inventory (should fail)");
    let big_order_id = OrderId::generate();
    let big_customer = Customer::new(
        CustomerEmail::try_new("bigorder@example.com".to_string())?,
        "Big Order Customer".to_string(),
        None,
    );

    executor
        .execute(
            CreateOrderCommand,
            CreateOrderInput::new(big_order_id.clone(), big_customer),
            ExecutionOptions::default(),
        )
        .await?;

    let big_laptop_item = OrderItem::new(
        laptop.id.clone(),
        Quantity::new(100)?, // More than available
        laptop.price,
    );

    if let Err(e) = executor
        .execute(
            AddItemToOrderCommand,
            AddItemToOrderInput::new(big_order_id, big_laptop_item, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await
    {
        info!("✓ Correctly rejected insufficient inventory: {}", e);
    } else {
        info!("✗ Unexpectedly allowed insufficient inventory");
    }

    // Try to place empty order
    info!("Testing empty order placement (should fail)");
    let empty_order_id = OrderId::generate();
    let empty_customer = Customer::new(
        CustomerEmail::try_new("empty@example.com".to_string())?,
        "Empty Order Customer".to_string(),
        None,
    );

    executor
        .execute(
            CreateOrderCommand,
            CreateOrderInput::new(empty_order_id.clone(), empty_customer),
            ExecutionOptions::default(),
        )
        .await?;

    if let Err(e) = executor
        .execute(
            PlaceOrderCommand,
            PlaceOrderInput::new(empty_order_id, catalog_stream.clone()),
            ExecutionOptions::default(),
        )
        .await
    {
        info!("✓ Correctly rejected empty order: {}", e);
    } else {
        info!("✗ Unexpectedly allowed empty order");
    }

    info!("E-commerce example completed successfully!");
    Ok(())
}
