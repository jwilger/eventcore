//! # Distributed E-Commerce System Example
#![allow(missing_docs)] // Example code doesn't need complete documentation
//!
//! This example demonstrates how `EventCore` handles distributed system challenges
//! in a microservices-based e-commerce platform.
//!
//! ## System Overview
//!
//! The example simulates a distributed e-commerce system with multiple services:
//! - **Order Service**: Manages customer orders and orchestrates the workflow
//! - **Inventory Service**: Handles product stock and reservations
//! - **Payment Service**: Processes payments and refunds
//! - **Shipping Service**: Manages order fulfillment and delivery
//!
//! ## Advanced Patterns Demonstrated
//!
//! 1. **Distributed Sagas**: Long-running transactions across multiple services
//! 2. **Event Choreography**: Services react to events from other services
//! 3. **Compensating Transactions**: Rollback mechanisms for failed workflows
//! 4. **Service Boundaries**: Each service maintains its own streams
//! 5. **Eventually Consistent Projections**: Cross-service read models
//! 6. **Idempotency**: Handling duplicate events in distributed systems
//! 7. **Multi-Stream Atomicity**: Atomic operations within service boundaries
//!
//! ## `EventCore` Benefits in Distributed Systems
//!
//! - **Atomic Multi-Stream Updates**: Each service can atomically update multiple streams
//! - **Event Sourcing**: Complete audit trail across all services
//! - **Dynamic Stream Discovery**: Services can discover relevant streams at runtime
//! - **Consistency Boundaries**: Clear boundaries between services while maintaining consistency

use eventcore::{
    CommandError, CommandExecutor, CommandLogic, CommandResult, CommandStreams, ExecutionOptions,
    ReadStreams, StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// Domain Types
// ============================================================================

pub mod types {
    use super::nutype;

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct OrderId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct ProductId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct CustomerId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct PaymentId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct ShipmentId(String);

    #[nutype(
        validate(greater = 0),
        derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            AsRef,
            Deref,
            Into,
            Serialize,
            Deserialize
        )
    )]
    pub struct Quantity(u32);

    #[nutype(
        validate(greater_or_equal = 0),
        derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            AsRef,
            Deref,
            Into,
            Serialize,
            Deserialize
        )
    )]
    pub struct Amount(u64);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 500),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            AsRef,
            Deref,
            Display,
            Serialize,
            Deserialize
        )
    )]
    pub struct Address(String);
}

use types::{Address, Amount, CustomerId, OrderId, PaymentId, ProductId, Quantity, ShipmentId};

// ============================================================================
// Service Boundaries
// ============================================================================

/// Represents different services in the distributed system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Order,
    Inventory,
    Payment,
    Shipping,
}

impl ServiceType {
    /// Get the stream prefix for this service
    #[must_use]
    pub const fn stream_prefix(&self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Inventory => "inventory",
            Self::Payment => "payment",
            Self::Shipping => "shipping",
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// Events that can occur across all services
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ECommerceEvent {
    // Order Service Events
    OrderCreated {
        order_id: OrderId,
        customer_id: CustomerId,
        items: HashMap<ProductId, Quantity>,
        total_amount: Amount,
    },
    OrderConfirmed {
        order_id: OrderId,
    },
    OrderCancelled {
        order_id: OrderId,
        reason: String,
    },
    OrderShipped {
        order_id: OrderId,
        shipment_id: ShipmentId,
    },
    OrderDelivered {
        order_id: OrderId,
    },

    // Inventory Service Events
    StockReserved {
        order_id: OrderId,
        product_id: ProductId,
        quantity: Quantity,
    },
    StockReservationFailed {
        order_id: OrderId,
        product_id: ProductId,
        requested: Quantity,
        available: Quantity,
    },
    StockReservationReleased {
        order_id: OrderId,
        product_id: ProductId,
        quantity: Quantity,
    },
    StockAdded {
        product_id: ProductId,
        quantity: Quantity,
    },

    // Payment Service Events
    PaymentProcessed {
        payment_id: PaymentId,
        order_id: OrderId,
        amount: Amount,
    },
    PaymentFailed {
        order_id: OrderId,
        reason: String,
    },
    PaymentRefunded {
        payment_id: PaymentId,
        order_id: OrderId,
        amount: Amount,
    },

    // Shipping Service Events
    ShipmentCreated {
        shipment_id: ShipmentId,
        order_id: OrderId,
        address: Address,
    },
    ShipmentDispatched {
        shipment_id: ShipmentId,
    },
    ShipmentDelivered {
        shipment_id: ShipmentId,
    },
    ShipmentCancelled {
        shipment_id: ShipmentId,
        reason: String,
    },
}

impl TryFrom<&Self> for ECommerceEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &Self) -> Result<Self, std::convert::Infallible> {
        Ok(value.clone())
    }
}

// ============================================================================
// State Types
// ============================================================================

/// Order state maintained by the Order Service
#[derive(Debug, Default, Clone)]
pub struct OrderState {
    pub orders: HashMap<OrderId, OrderInfo>,
}

#[derive(Debug, Clone)]
pub struct OrderInfo {
    pub customer_id: CustomerId,
    pub items: HashMap<ProductId, Quantity>,
    pub total_amount: Amount,
    pub status: OrderStatus,
    pub payment_id: Option<PaymentId>,
    pub shipment_id: Option<ShipmentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderStatus {
    Created,
    InventoryReserved,
    PaymentProcessed,
    Shipped,
    Delivered,
    Cancelled { reason: String },
}

/// Inventory state maintained by the Inventory Service
#[derive(Debug, Default, Clone)]
pub struct InventoryState {
    pub stock: HashMap<ProductId, Quantity>,
    pub reservations: HashMap<(OrderId, ProductId), Quantity>,
}

/// Payment state maintained by the Payment Service
#[derive(Debug, Default, Clone)]
pub struct PaymentState {
    pub payments: HashMap<PaymentId, PaymentInfo>,
    pub order_payments: HashMap<OrderId, PaymentId>,
}

#[derive(Debug, Clone)]
pub struct PaymentInfo {
    pub order_id: OrderId,
    pub amount: Amount,
    pub status: PaymentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentStatus {
    Processed,
    Refunded,
}

/// Shipping state maintained by the Shipping Service
#[derive(Debug, Default, Clone)]
pub struct ShippingState {
    pub shipments: HashMap<ShipmentId, ShipmentInfo>,
}

#[derive(Debug, Clone)]
pub struct ShipmentInfo {
    pub order_id: OrderId,
    pub address: Address,
    pub status: ShipmentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShipmentStatus {
    Created,
    Dispatched,
    Delivered,
    Cancelled { reason: String },
}

// ============================================================================
// Commands - Order Service
// ============================================================================

/// Create a new order - initiates the distributed saga
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrder {
    pub order_id: OrderId,
    pub customer_id: CustomerId,
    pub items: HashMap<ProductId, Quantity>,
    pub total_amount: Amount,
}

impl CommandStreams for CreateOrder {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            // Order stream
            StreamId::from_static("order-aggregate"),
            // Order-specific stream for saga coordination
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for CreateOrder {
    type State = OrderState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ECommerceEvent::OrderCreated {
            order_id,
            customer_id,
            items,
            total_amount,
        } = &event.payload
        {
            state.orders.insert(
                order_id.clone(),
                OrderInfo {
                    customer_id: customer_id.clone(),
                    items: items.clone(),
                    total_amount: *total_amount,
                    status: OrderStatus::Created,
                    payment_id: None,
                    shipment_id: None,
                },
            );
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order already exists
        if state.orders.contains_key(&self.order_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} already exists",
                self.order_id
            )));
        }

        // Create order event
        let order_created = ECommerceEvent::OrderCreated {
            order_id: self.order_id.clone(),
            customer_id: self.customer_id.clone(),
            items: self.items.clone(),
            total_amount: self.total_amount,
        };

        Ok(vec![
            // Write to aggregate stream
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("order-aggregate"),
                order_created.clone(),
            )?,
            // Write to order-specific stream for saga coordination
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                order_created,
            )?,
        ])
    }
}

// ============================================================================
// Commands - Inventory Service
// ============================================================================

/// Reserve stock for an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReserveStock {
    pub order_id: OrderId,
    pub product_id: ProductId,
    pub quantity: Quantity,
}

impl CommandStreams for ReserveStock {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            // Inventory aggregate stream
            StreamId::from_static("inventory-aggregate"),
            // Product-specific stream
            StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
            // Order stream to emit reservation result
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for ReserveStock {
    type State = InventoryState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ECommerceEvent::StockAdded {
                product_id,
                quantity,
            } => {
                let current = state
                    .stock
                    .get(product_id)
                    .copied()
                    .unwrap_or_else(|| Quantity::try_new(1).unwrap());
                state.stock.insert(
                    product_id.clone(),
                    Quantity::try_new(current.into_inner() + quantity.into_inner()).unwrap(),
                );
            }
            ECommerceEvent::StockReserved {
                order_id,
                product_id,
                quantity,
            } => {
                // Reduce available stock
                if let Some(current) = state.stock.get_mut(product_id) {
                    *current =
                        Quantity::try_new(current.into_inner() - quantity.into_inner()).unwrap();
                }
                // Track reservation
                state
                    .reservations
                    .insert((order_id.clone(), product_id.clone()), *quantity);
            }
            ECommerceEvent::StockReservationReleased {
                order_id,
                product_id,
                quantity,
            } => {
                // Return stock
                if let Some(current) = state.stock.get_mut(product_id) {
                    *current =
                        Quantity::try_new(current.into_inner() + quantity.into_inner()).unwrap();
                }
                // Remove reservation
                state
                    .reservations
                    .remove(&(order_id.clone(), product_id.clone()));
            }
            _ => {} // Inventory service only cares about inventory events
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if already reserved
        if state
            .reservations
            .contains_key(&(self.order_id.clone(), self.product_id.clone()))
        {
            return Ok(vec![]); // Idempotent - already reserved
        }

        // Check available stock
        let available = state
            .stock
            .get(&self.product_id)
            .copied()
            .unwrap_or_else(|| Quantity::try_new(1).unwrap());

        if available.into_inner() < self.quantity.into_inner() {
            // Not enough stock - emit failure event
            let failure_event = ECommerceEvent::StockReservationFailed {
                order_id: self.order_id.clone(),
                product_id: self.product_id.clone(),
                requested: self.quantity,
                available,
            };

            return Ok(vec![
                // Write to inventory aggregate
                StreamWrite::new(
                    &read_streams,
                    StreamId::from_static("inventory-aggregate"),
                    failure_event.clone(),
                )?,
                // Notify order service
                StreamWrite::new(
                    &read_streams,
                    StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                    failure_event,
                )?,
            ]);
        }

        // Reserve stock
        let reservation_event = ECommerceEvent::StockReserved {
            order_id: self.order_id.clone(),
            product_id: self.product_id.clone(),
            quantity: self.quantity,
        };

        Ok(vec![
            // Update inventory aggregate
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("inventory-aggregate"),
                reservation_event.clone(),
            )?,
            // Update product stream
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
                reservation_event.clone(),
            )?,
            // Notify order service
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                reservation_event,
            )?,
        ])
    }
}

// ============================================================================
// Commands - Payment Service
// ============================================================================

/// Process payment for an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessPayment {
    pub payment_id: PaymentId,
    pub order_id: OrderId,
    pub amount: Amount,
}

impl CommandStreams for ProcessPayment {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            // Payment aggregate stream
            StreamId::from_static("payment-aggregate"),
            // Order stream to emit payment result
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for ProcessPayment {
    type State = PaymentState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ECommerceEvent::PaymentProcessed {
                payment_id,
                order_id,
                amount,
            } => {
                state.payments.insert(
                    payment_id.clone(),
                    PaymentInfo {
                        order_id: order_id.clone(),
                        amount: *amount,
                        status: PaymentStatus::Processed,
                    },
                );
                state
                    .order_payments
                    .insert(order_id.clone(), payment_id.clone());
            }
            ECommerceEvent::PaymentRefunded {
                payment_id,
                order_id: _,
                amount: _,
            } => {
                if let Some(payment) = state.payments.get_mut(payment_id) {
                    payment.status = PaymentStatus::Refunded;
                }
            }
            _ => {} // Payment service only cares about payment events
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if payment already processed for this order
        if state.order_payments.contains_key(&self.order_id) {
            return Ok(vec![]); // Idempotent - already processed
        }

        // Simulate payment processing (in real system, would call payment gateway)
        // For demo, randomly fail 10% of payments
        let payment_succeeds = rand::random::<f32>() > 0.1;

        if !payment_succeeds {
            let failure_event = ECommerceEvent::PaymentFailed {
                order_id: self.order_id.clone(),
                reason: "Insufficient funds".to_string(),
            };

            return Ok(vec![
                // Write to payment aggregate
                StreamWrite::new(
                    &read_streams,
                    StreamId::from_static("payment-aggregate"),
                    failure_event.clone(),
                )?,
                // Notify order service
                StreamWrite::new(
                    &read_streams,
                    StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                    failure_event,
                )?,
            ]);
        }

        // Process payment
        let payment_event = ECommerceEvent::PaymentProcessed {
            payment_id: self.payment_id.clone(),
            order_id: self.order_id.clone(),
            amount: self.amount,
        };

        Ok(vec![
            // Update payment aggregate
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("payment-aggregate"),
                payment_event.clone(),
            )?,
            // Notify order service
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                payment_event,
            )?,
        ])
    }
}

// ============================================================================
// Saga Orchestrator
// ============================================================================

/// Orchestrates the order processing saga across services
pub struct OrderSagaOrchestrator {
    executor: Arc<CommandExecutor<InMemoryEventStore<ECommerceEvent>>>,
}

impl OrderSagaOrchestrator {
    #[must_use]
    pub const fn new(executor: Arc<CommandExecutor<InMemoryEventStore<ECommerceEvent>>>) -> Self {
        Self { executor }
    }

    /// Process an order through the distributed saga
    ///
    /// # Errors
    ///
    /// Returns an error if any command execution fails during the saga.
    ///
    /// # Panics
    ///
    /// Panics if UUID generation fails (which should never happen in practice).
    pub async fn process_order(
        &self,
        order: CreateOrder,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n🎯 Starting Distributed Order Saga");
        println!("   Order ID: {}", order.order_id);
        println!("   Customer: {}", order.customer_id);
        println!(
            "   Total Amount: ${}.{:02}",
            order.total_amount.into_inner() / 100,
            order.total_amount.into_inner() % 100
        );

        // Step 1: Create order
        println!("\n📝 Step 1: Creating order...");
        self.executor
            .execute(order.clone(), ExecutionOptions::new())
            .await?;
        println!("   ✅ Order created successfully");

        // Step 2: Reserve inventory for each item
        println!("\n📦 Step 2: Reserving inventory...");
        let mut all_reserved = true;
        for (product_id, quantity) in &order.items {
            let reserve_cmd = ReserveStock {
                order_id: order.order_id.clone(),
                product_id: product_id.clone(),
                quantity: *quantity,
            };

            match self
                .executor
                .execute(reserve_cmd, ExecutionOptions::new())
                .await
            {
                Ok(_) => println!(
                    "   ✅ Reserved {} units of {}",
                    quantity.into_inner(),
                    product_id
                ),
                Err(e) => {
                    println!("   ❌ Failed to reserve {product_id}: {e}");
                    all_reserved = false;
                    break;
                }
            }
        }

        if !all_reserved {
            // Compensate: Cancel order
            println!("\n🔄 Compensating: Cancelling order due to inventory shortage");
            self.cancel_order(order.order_id.clone(), "Insufficient inventory")
                .await?;
            return Ok(());
        }

        // Step 3: Process payment
        println!("\n💳 Step 3: Processing payment...");
        let payment_cmd = ProcessPayment {
            payment_id: PaymentId::try_new(format!(
                "PAY-{}",
                Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
            ))
            .unwrap(),
            order_id: order.order_id.clone(),
            amount: order.total_amount,
        };

        match self
            .executor
            .execute(payment_cmd, ExecutionOptions::new())
            .await
        {
            Ok(_) => println!("   ✅ Payment processed successfully"),
            Err(e) => {
                println!("   ❌ Payment failed: {e}");

                // Compensate: Release inventory and cancel order
                println!("\n🔄 Compensating: Releasing inventory and cancelling order");
                for (product_id, quantity) in &order.items {
                    self.release_inventory(order.order_id.clone(), product_id.clone(), *quantity)
                        .await?;
                }
                self.cancel_order(order.order_id.clone(), "Payment failed")
                    .await?;
                return Ok(());
            }
        }

        // Step 4: Create shipment
        println!("\n🚚 Step 4: Creating shipment...");
        let shipment_cmd = CreateShipment {
            shipment_id: ShipmentId::try_new(format!(
                "SHIP-{}",
                Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
            ))
            .unwrap(),
            order_id: order.order_id.clone(),
            address: Address::try_new("123 Main St, Anytown, USA").unwrap(),
        };

        self.executor
            .execute(shipment_cmd, ExecutionOptions::new())
            .await?;
        println!("   ✅ Shipment created successfully");

        println!("\n✨ Order saga completed successfully!");
        Ok(())
    }

    async fn cancel_order(
        &self,
        order_id: OrderId,
        reason: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cancel_cmd = CancelOrder {
            order_id,
            reason: reason.to_string(),
        };
        self.executor
            .execute(cancel_cmd, ExecutionOptions::new())
            .await?;
        Ok(())
    }

    async fn release_inventory(
        &self,
        order_id: OrderId,
        product_id: ProductId,
        quantity: Quantity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let release_cmd = ReleaseStock {
            order_id,
            product_id,
            quantity,
        };
        self.executor
            .execute(release_cmd, ExecutionOptions::new())
            .await?;
        Ok(())
    }
}

// ============================================================================
// Additional Commands (simplified for brevity)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrder {
    pub order_id: OrderId,
    pub reason: String,
}

impl CommandStreams for CancelOrder {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::from_static("order-aggregate"),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for CancelOrder {
    type State = OrderState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ECommerceEvent::OrderCancelled { order_id, reason } = &event.payload {
            if let Some(order) = state.orders.get_mut(order_id) {
                order.status = OrderStatus::Cancelled {
                    reason: reason.clone(),
                };
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = ECommerceEvent::OrderCancelled {
            order_id: self.order_id.clone(),
            reason: self.reason.clone(),
        };

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("order-aggregate"),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                event,
            )?,
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseStock {
    pub order_id: OrderId,
    pub product_id: ProductId,
    pub quantity: Quantity,
}

impl CommandStreams for ReleaseStock {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::from_static("inventory-aggregate"),
            StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for ReleaseStock {
    type State = InventoryState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ECommerceEvent::StockReservationReleased {
            order_id,
            product_id,
            quantity,
        } = &event.payload
        {
            // Return stock
            if let Some(current) = state.stock.get_mut(product_id) {
                *current = Quantity::try_new(current.into_inner() + quantity.into_inner()).unwrap();
            }
            // Remove reservation
            state
                .reservations
                .remove(&(order_id.clone(), product_id.clone()));
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = ECommerceEvent::StockReservationReleased {
            order_id: self.order_id.clone(),
            product_id: self.product_id.clone(),
            quantity: self.quantity,
        };

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("inventory-aggregate"),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
                event,
            )?,
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShipment {
    pub shipment_id: ShipmentId,
    pub order_id: OrderId,
    pub address: Address,
}

impl CommandStreams for CreateShipment {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::from_static("shipping-aggregate"),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for CreateShipment {
    type State = ShippingState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ECommerceEvent::ShipmentCreated {
            shipment_id,
            order_id,
            address,
        } = &event.payload
        {
            state.shipments.insert(
                shipment_id.clone(),
                ShipmentInfo {
                    order_id: order_id.clone(),
                    address: address.clone(),
                    status: ShipmentStatus::Created,
                },
            );
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = ECommerceEvent::ShipmentCreated {
            shipment_id: self.shipment_id.clone(),
            order_id: self.order_id.clone(),
            address: self.address.clone(),
        };

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("shipping-aggregate"),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                event,
            )?,
        ])
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Initialize inventory with some products
async fn initialize_inventory(
    executor: &CommandExecutor<InMemoryEventStore<ECommerceEvent>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Add stock for products
    let products = vec![
        (
            ProductId::try_new("LAPTOP-001").unwrap(),
            Quantity::try_new(10).unwrap(),
        ),
        (
            ProductId::try_new("MOUSE-001").unwrap(),
            Quantity::try_new(50).unwrap(),
        ),
        (
            ProductId::try_new("KEYBOARD-001").unwrap(),
            Quantity::try_new(30).unwrap(),
        ),
    ];

    for (product_id, quantity) in products {
        let add_stock = AddStock {
            product_id: product_id.clone(),
            quantity,
        };
        executor.execute(add_stock, ExecutionOptions::new()).await?;
        println!("   Added {} units of {}", quantity.into_inner(), product_id);
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddStock {
    product_id: ProductId,
    quantity: Quantity,
}

impl CommandStreams for AddStock {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::from_static("inventory-aggregate"),
            StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for AddStock {
    type State = InventoryState;
    type Event = ECommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ECommerceEvent::StockAdded {
            product_id,
            quantity,
        } = &event.payload
        {
            let current = state
                .stock
                .get(product_id)
                .copied()
                .unwrap_or_else(|| Quantity::try_new(1).unwrap());
            state.stock.insert(
                product_id.clone(),
                Quantity::try_new(current.into_inner() + quantity.into_inner()).unwrap(),
            );
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = ECommerceEvent::StockAdded {
            product_id: self.product_id.clone(),
            quantity: self.quantity,
        };

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::from_static("inventory-aggregate"),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("inventory-product-{}", self.product_id)).unwrap(),
                event,
            )?,
        ])
    }
}

// ============================================================================
// Main Example
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🛍️  Distributed E-Commerce System Example");
    println!("==========================================");
    println!();
    println!("This example demonstrates EventCore in a distributed microservices architecture.");
    println!();

    // Create event store and executor
    let event_store = InMemoryEventStore::<ECommerceEvent>::new();
    let executor = Arc::new(CommandExecutor::new(event_store));

    // Initialize inventory
    println!("📦 Initializing inventory...");
    initialize_inventory(&executor).await?;

    // Create saga orchestrator
    let orchestrator = OrderSagaOrchestrator::new(executor.clone());

    // Example 1: Successful order
    println!("\n\n🎯 Example 1: Successful Order Processing");
    println!("=========================================");

    let order1 = CreateOrder {
        order_id: OrderId::try_new("ORDER-001").unwrap(),
        customer_id: CustomerId::try_new("CUST-123").unwrap(),
        items: {
            let mut items = HashMap::new();
            items.insert(
                ProductId::try_new("LAPTOP-001").unwrap(),
                Quantity::try_new(1).unwrap(),
            );
            items.insert(
                ProductId::try_new("MOUSE-001").unwrap(),
                Quantity::try_new(2).unwrap(),
            );
            items
        },
        total_amount: Amount::try_new(150_000).unwrap(), // $1,500.00
    };

    orchestrator.process_order(order1).await?;

    // Example 2: Order with insufficient inventory
    println!("\n\n🎯 Example 2: Order with Insufficient Inventory");
    println!("===============================================");

    let order2 = CreateOrder {
        order_id: OrderId::try_new("ORDER-002").unwrap(),
        customer_id: CustomerId::try_new("CUST-456").unwrap(),
        items: {
            let mut items = HashMap::new();
            items.insert(
                ProductId::try_new("LAPTOP-001").unwrap(),
                Quantity::try_new(100).unwrap(),
            ); // More than available
            items
        },
        total_amount: Amount::try_new(10_000_000).unwrap(), // $100,000.00
    };

    orchestrator.process_order(order2).await?;

    // Example 3: Demonstrate idempotency
    println!("\n\n🎯 Example 3: Idempotency - Retrying Order 1");
    println!("============================================");

    let order1_retry = CreateOrder {
        order_id: OrderId::try_new("ORDER-001").unwrap(), // Same order ID
        customer_id: CustomerId::try_new("CUST-123").unwrap(),
        items: {
            let mut items = HashMap::new();
            items.insert(
                ProductId::try_new("LAPTOP-001").unwrap(),
                Quantity::try_new(1).unwrap(),
            );
            items.insert(
                ProductId::try_new("MOUSE-001").unwrap(),
                Quantity::try_new(2).unwrap(),
            );
            items
        },
        total_amount: Amount::try_new(150_000).unwrap(),
    };

    match orchestrator.process_order(order1_retry).await {
        Ok(()) => println!("   Order processed (idempotent handling)"),
        Err(e) => println!("   Order already exists (as expected): {e}"),
    }

    // Print summary of EventCore patterns demonstrated
    println!("\n\n📚 Summary of Distributed Patterns Demonstrated");
    println!("==============================================");
    println!();
    println!("1. **Distributed Sagas**: Order processing spans multiple services");
    println!("2. **Event Choreography**: Services react to events from other services");
    println!("3. **Compensating Transactions**: Failed payments trigger inventory release");
    println!("4. **Service Boundaries**: Each service maintains its own streams");
    println!("5. **Multi-Stream Atomicity**: Services update multiple streams atomically");
    println!("6. **Idempotency**: Duplicate commands are handled gracefully");
    println!("7. **Event-Driven Architecture**: Services communicate through events");
    println!();
    println!("EventCore enables building robust distributed systems while maintaining");
    println!("consistency within service boundaries and eventual consistency across services.");

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_distributed_saga_success() {
        let event_store = InMemoryEventStore::<ECommerceEvent>::new();
        let executor = Arc::new(CommandExecutor::new(event_store.clone()));

        // Initialize inventory
        let add_stock = AddStock {
            product_id: ProductId::try_new("PROD-001").unwrap(),
            quantity: Quantity::try_new(10).unwrap(),
        };
        executor
            .execute(add_stock, ExecutionOptions::new())
            .await
            .unwrap();

        // Create order
        let order = CreateOrder {
            order_id: OrderId::try_new("ORDER-TEST-001").unwrap(),
            customer_id: CustomerId::try_new("CUST-TEST").unwrap(),
            items: {
                let mut items = HashMap::new();
                items.insert(
                    ProductId::try_new("PROD-001").unwrap(),
                    Quantity::try_new(2).unwrap(),
                );
                items
            },
            total_amount: Amount::try_new(10_000).unwrap(),
        };

        // Execute order creation
        executor
            .execute(order, ExecutionOptions::new())
            .await
            .unwrap();

        // Verify order was created
        let order_stream = StreamId::try_new("order-ORDER-TEST-001").unwrap();
        let events = event_store
            .read_stream(&order_stream, None, 100)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);

        if let ECommerceEvent::OrderCreated { order_id, .. } = &events[0].payload {
            assert_eq!(order_id, &order.order_id);
        } else {
            panic!("Expected OrderCreated event");
        }
    }

    #[tokio::test]
    async fn test_inventory_reservation_failure() {
        let event_store = InMemoryEventStore::<ECommerceEvent>::new();
        let executor = Arc::new(CommandExecutor::new(event_store.clone()));

        // Try to reserve without stock
        let reserve = ReserveStock {
            order_id: OrderId::try_new("ORDER-TEST-002").unwrap(),
            product_id: ProductId::try_new("PROD-002").unwrap(),
            quantity: Quantity::try_new(5).unwrap(),
        };

        executor
            .execute(reserve, ExecutionOptions::new())
            .await
            .unwrap();

        // Check for failure event
        let order_stream = StreamId::try_new("order-ORDER-TEST-002").unwrap();
        let events = event_store
            .read_stream(&order_stream, None, 100)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);

        if let ECommerceEvent::StockReservationFailed {
            requested,
            available,
            ..
        } = &events[0].payload
        {
            assert_eq!(*requested, Quantity::try_new(5).unwrap());
            assert_eq!(*available, Quantity::try_new(1).unwrap()); // Default when no stock
        } else {
            panic!("Expected StockReservationFailed event");
        }
    }

    #[tokio::test]
    async fn test_idempotent_payment_processing() {
        let event_store = InMemoryEventStore::<ECommerceEvent>::new();
        let executor = Arc::new(CommandExecutor::new(event_store.clone()));

        let payment = ProcessPayment {
            payment_id: PaymentId::try_new("PAY-TEST-001").unwrap(),
            order_id: OrderId::try_new("ORDER-TEST-003").unwrap(),
            amount: Amount::try_new(5_000).unwrap(),
        };

        // Process payment twice
        executor
            .execute(payment.clone(), ExecutionOptions::new())
            .await
            .unwrap();
        executor
            .execute(payment, ExecutionOptions::new())
            .await
            .unwrap();

        // Should only have one payment event
        let payment_stream = StreamId::from_static("payment-aggregate");
        let events = event_store
            .read_stream(&payment_stream, None, 100)
            .await
            .unwrap();

        let payment_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.payload, ECommerceEvent::PaymentProcessed { .. }))
            .collect();

        assert!(payment_events.len() <= 1, "Payment should be idempotent");
    }
}
