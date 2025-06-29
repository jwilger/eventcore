//! E-commerce order workflow example
//!
//! This module demonstrates a complete e-commerce order system using EventCore's
//! multi-stream event sourcing. It includes:
//!
//! - Type-safe domain modeling with validation
//! - Product catalog management
//! - Order workflow (create, add items, place, cancel)
//! - Inventory tracking with automatic reservation
//! - Read model projections for analytics
//!
//! # Key Features
//!
//! - **Type Safety**: All domain concepts use validated types that make illegal states unrepresentable
//! - **Multi-Stream Atomicity**: Commands can read from and write to multiple streams atomically
//! - **Business Rule Enforcement**: Commands validate business rules before emitting events
//! - **Inventory Management**: Automatic inventory reservation and release
//! - **Projections**: Maintain read models for efficient querying
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use eventcore_examples::ecommerce::{
//!     commands::*,
//!     types::*,
//!     projections::*,
//! };
//! use eventcore::{CommandExecutor, EventStore, ExecutionOptions, StreamId};
//! use eventcore_memory::InMemoryEventStore;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create event store and executor
//! let event_store = InMemoryEventStore::new();
//! let executor = CommandExecutor::new(event_store.clone());
//!
//! // Create catalog stream
//! let catalog_stream = StreamId::try_new("product-catalog".to_string())?;
//!
//! // Add a product to the catalog
//! let product = Product::new(
//!     ProductId::try_new("PRD-LAPTOP01".to_string())?,
//!     Sku::try_new("LAPTOP-15".to_string())?,
//!     ProductName::try_new("Gaming Laptop".to_string())?,
//!     Money::from_cents(99999)?,
//!     Some("High-performance gaming laptop".to_string()),
//! );
//!
//! let add_product_input = AddProductInput::new(
//!     product,
//!     Quantity::new(10)?,
//!     catalog_stream.clone(),
//! );
//!
//! executor.execute(&AddProductCommand, add_product_input, ExecutionOptions::default()).await?;
//!
//! // Create an order
//! let order_id = OrderId::generate();
//! let customer = Customer::new(
//!     CustomerEmail::try_new("customer@example.com".to_string())?,
//!     "John Doe".to_string(),
//!     Some("123 Main St".to_string()),
//! );
//!
//! let create_order_input = CreateOrderInput::new(order_id.clone(), customer);
//! executor.execute(&CreateOrderCommand, create_order_input, ExecutionOptions::default()).await?;
//!
//! // Add item to order
//! let item = OrderItem::new(
//!     ProductId::try_new("PRD-LAPTOP01".to_string())?,
//!     Quantity::new(1)?,
//!     Money::from_cents(99999)?,
//! );
//!
//! let add_item_input = AddItemToOrderInput::new(order_id.clone(), item, catalog_stream.clone());
//! executor.execute(&AddItemToOrderCommand, add_item_input, ExecutionOptions::default()).await?;
//!
//! // Place the order
//! let place_order_input = PlaceOrderInput::new(order_id, catalog_stream);
//! executor.execute(&PlaceOrderCommand, place_order_input, ExecutionOptions::default()).await?;
//! # Ok(())
//! # }
//! ```

pub mod commands;
pub mod events;
pub mod projections;
pub mod types;

pub use commands::*;
pub use events::*;
pub use projections::*;
pub use types::*;
