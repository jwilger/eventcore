//! Type definitions for the order fulfillment saga example
//!
//! This module defines all the domain types used in the saga pattern implementation,
//! demonstrating how EventCore enables complex distributed workflows with strong
//! type safety and compensation logic.

use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Core domain identifiers
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
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
    validate(not_empty, len_char_max = 255),
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
    validate(not_empty, len_char_max = 255),
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
    validate(not_empty, len_char_max = 255),
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
    validate(not_empty, len_char_max = 255),
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
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
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
pub struct SagaId(String);

// Money type for financial operations
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
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct Money(u64);

impl Money {
    pub fn new(cents: u64) -> Self {
        Money::try_new(cents).unwrap()
    }

    pub fn from_dollars(dollars: f64) -> Self {
        Money::try_new((dollars * 100.0).round() as u64).unwrap()
    }

    pub fn to_dollars(&self) -> f64 {
        self.into_inner() as f64 / 100.0
    }

    pub fn add(&self, other: &Money) -> Money {
        Money::try_new(self.into_inner() + other.into_inner()).unwrap()
    }

    pub fn multiply(&self, quantity: u32) -> Money {
        Money::try_new(self.into_inner() * quantity as u64).unwrap()
    }
}

// Quantity type for inventory
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
        Into,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct Quantity(u32);

// Order item representing a product and quantity
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: ProductId,
    pub quantity: Quantity,
    pub unit_price: Money,
}

impl OrderItem {
    pub fn new(product_id: ProductId, quantity: Quantity, unit_price: Money) -> Self {
        Self {
            product_id,
            quantity,
            unit_price,
        }
    }

    pub fn total_price(&self) -> Money {
        self.unit_price.multiply(self.quantity.into_inner())
    }
}

// Saga states representing the workflow progression
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaStatus {
    Started,
    PaymentProcessing,
    PaymentCompleted,
    InventoryReserved,
    ShippingArranged,
    Completed,
    Failed { reason: String },
    Compensating,
    Compensated,
}

// Order aggregate data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub customer_id: CustomerId,
    pub items: HashMap<ProductId, OrderItem>,
    pub total_amount: Money,
    pub status: OrderStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Draft,
    Submitted,
    PaymentPending,
    PaymentCompleted,
    InventoryReserved,
    Shipped,
    Delivered,
    Cancelled { reason: String },
}

// Payment information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentDetails {
    pub payment_id: PaymentId,
    pub amount: Money,
    pub method: PaymentMethod,
    pub status: PaymentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentMethod {
    CreditCard { last_four: String },
    BankTransfer { account_number: String },
    DigitalWallet { provider: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentStatus {
    Pending,
    Authorized,
    Captured,
    Failed { reason: String },
    Refunded,
}

// Inventory reservation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryReservation {
    pub product_id: ProductId,
    pub quantity: Quantity,
    pub reserved_at: chrono::DateTime<chrono::Utc>,
}

// Shipping information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShippingDetails {
    pub shipment_id: ShipmentId,
    pub address: ShippingAddress,
    pub carrier: String,
    pub tracking_number: Option<String>,
    pub status: ShippingStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShippingAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip_code: String,
    pub country: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShippingStatus {
    Pending,
    Arranged,
    InTransit,
    Delivered,
    Failed { reason: String },
}

// Saga coordination state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SagaState {
    pub saga_id: SagaId,
    pub order_id: OrderId,
    pub customer_id: CustomerId,
    pub status: SagaStatus,
    pub payment_id: Option<PaymentId>,
    pub shipment_id: Option<ShipmentId>,
    pub reservations: Vec<InventoryReservation>,
    pub compensation_actions: Vec<CompensationAction>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompensationAction {
    RefundPayment {
        payment_id: PaymentId,
    },
    ReleaseInventory {
        reservations: Vec<InventoryReservation>,
    },
    CancelShipment {
        shipment_id: ShipmentId,
    },
}

// Error types for saga operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaError {
    OrderNotFound(OrderId),
    PaymentFailed {
        reason: String,
    },
    InsufficientInventory {
        product_id: ProductId,
        requested: Quantity,
        available: Quantity,
    },
    ShippingUnavailable {
        reason: String,
    },
    CompensationFailed {
        action: CompensationAction,
        reason: String,
    },
    InvalidStateTransition {
        from: SagaStatus,
        to: SagaStatus,
    },
}

impl std::fmt::Display for SagaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SagaError::OrderNotFound(id) => write!(f, "Order not found: {}", id),
            SagaError::PaymentFailed { reason } => write!(f, "Payment failed: {}", reason),
            SagaError::InsufficientInventory {
                product_id,
                requested,
                available,
            } => {
                write!(
                    f,
                    "Insufficient inventory for product {}: requested {}, available {}",
                    product_id, requested, available
                )
            }
            SagaError::ShippingUnavailable { reason } => {
                write!(f, "Shipping unavailable: {}", reason)
            }
            SagaError::CompensationFailed { action, reason } => {
                write!(f, "Compensation failed for {:?}: {}", action, reason)
            }
            SagaError::InvalidStateTransition { from, to } => {
                write!(f, "Invalid state transition from {:?} to {:?}", from, to)
            }
        }
    }
}

impl std::error::Error for SagaError {}

// Helper functions for creating test data
impl OrderId {
    pub fn generate() -> Self {
        OrderId::try_new(format!(
            "order-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}

impl CustomerId {
    pub fn generate() -> Self {
        CustomerId::try_new(format!(
            "customer-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}

impl ProductId {
    pub fn generate() -> Self {
        ProductId::try_new(format!(
            "product-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}

impl PaymentId {
    pub fn generate() -> Self {
        PaymentId::try_new(format!(
            "payment-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}

impl ShipmentId {
    pub fn generate() -> Self {
        ShipmentId::try_new(format!(
            "shipment-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}

impl SagaId {
    pub fn generate() -> Self {
        SagaId::try_new(format!(
            "saga-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap()
    }
}
