//! Event definitions for the order fulfillment saga example
//!
//! This module defines all the events that occur during the saga workflow,
//! providing a complete audit trail of the distributed transaction.

use crate::sagas::types::*;
use serde::{Deserialize, Serialize};

// Saga coordination events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaEvent {
    SagaStarted {
        saga_id: SagaId,
        order_id: OrderId,
        customer_id: CustomerId,
        total_amount: Money,
        started_at: chrono::DateTime<chrono::Utc>,
    },
    PaymentInitiated {
        saga_id: SagaId,
        payment_id: PaymentId,
        amount: Money,
        method: PaymentMethod,
    },
    PaymentCompleted {
        saga_id: SagaId,
        payment_id: PaymentId,
        amount: Money,
    },
    PaymentFailed {
        saga_id: SagaId,
        payment_id: PaymentId,
        reason: String,
    },
    InventoryReservationStarted {
        saga_id: SagaId,
        items: Vec<OrderItem>,
    },
    InventoryReserved {
        saga_id: SagaId,
        reservations: Vec<InventoryReservation>,
    },
    InventoryReservationFailed {
        saga_id: SagaId,
        product_id: ProductId,
        requested: Quantity,
        available: Quantity,
    },
    ShippingArranged {
        saga_id: SagaId,
        shipment_id: ShipmentId,
        address: ShippingAddress,
        carrier: String,
    },
    ShippingFailed {
        saga_id: SagaId,
        reason: String,
    },
    SagaCompleted {
        saga_id: SagaId,
        completed_at: chrono::DateTime<chrono::Utc>,
    },
    SagaFailed {
        saga_id: SagaId,
        reason: String,
        failed_at: chrono::DateTime<chrono::Utc>,
    },
    CompensationStarted {
        saga_id: SagaId,
        actions: Vec<CompensationAction>,
    },
    CompensationCompleted {
        saga_id: SagaId,
        completed_at: chrono::DateTime<chrono::Utc>,
    },
    CompensationFailed {
        saga_id: SagaId,
        action: CompensationAction,
        reason: String,
    },
}

// Order-specific events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderEvent {
    OrderSubmitted {
        order_id: OrderId,
        customer_id: CustomerId,
        items: Vec<OrderItem>,
        total_amount: Money,
        submitted_at: chrono::DateTime<chrono::Utc>,
    },
    OrderPaymentStarted {
        order_id: OrderId,
        payment_id: PaymentId,
        amount: Money,
    },
    OrderPaymentCompleted {
        order_id: OrderId,
        payment_id: PaymentId,
        amount: Money,
    },
    OrderInventoryReserved {
        order_id: OrderId,
        reservations: Vec<InventoryReservation>,
    },
    OrderShipped {
        order_id: OrderId,
        shipment_id: ShipmentId,
        tracking_number: String,
    },
    OrderCompleted {
        order_id: OrderId,
        completed_at: chrono::DateTime<chrono::Utc>,
    },
    OrderCancelled {
        order_id: OrderId,
        reason: String,
        cancelled_at: chrono::DateTime<chrono::Utc>,
    },
}

// Payment service events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentEvent {
    PaymentAuthorized {
        payment_id: PaymentId,
        order_id: OrderId,
        amount: Money,
        method: PaymentMethod,
        authorized_at: chrono::DateTime<chrono::Utc>,
    },
    PaymentCaptured {
        payment_id: PaymentId,
        amount: Money,
        captured_at: chrono::DateTime<chrono::Utc>,
    },
    PaymentDeclined {
        payment_id: PaymentId,
        reason: String,
        declined_at: chrono::DateTime<chrono::Utc>,
    },
    PaymentRefunded {
        payment_id: PaymentId,
        amount: Money,
        refunded_at: chrono::DateTime<chrono::Utc>,
    },
    PaymentError {
        payment_id: PaymentId,
        error: String,
        occurred_at: chrono::DateTime<chrono::Utc>,
    },
}

// Inventory service events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InventoryEvent {
    InventoryChecked {
        product_id: ProductId,
        available_quantity: Quantity,
        checked_at: chrono::DateTime<chrono::Utc>,
    },
    InventoryReserved {
        product_id: ProductId,
        quantity: Quantity,
        order_id: OrderId,
        reserved_at: chrono::DateTime<chrono::Utc>,
    },
    InventoryReleased {
        product_id: ProductId,
        quantity: Quantity,
        order_id: OrderId,
        released_at: chrono::DateTime<chrono::Utc>,
    },
    InventoryInsufficient {
        product_id: ProductId,
        requested: Quantity,
        available: Quantity,
        checked_at: chrono::DateTime<chrono::Utc>,
    },
    InventoryDeducted {
        product_id: ProductId,
        quantity: Quantity,
        order_id: OrderId,
        deducted_at: chrono::DateTime<chrono::Utc>,
    },
}

// Shipping service events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShippingEvent {
    ShipmentCreated {
        shipment_id: ShipmentId,
        order_id: OrderId,
        address: ShippingAddress,
        carrier: String,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    ShipmentDispatched {
        shipment_id: ShipmentId,
        tracking_number: String,
        dispatched_at: chrono::DateTime<chrono::Utc>,
    },
    ShipmentDelivered {
        shipment_id: ShipmentId,
        delivered_at: chrono::DateTime<chrono::Utc>,
    },
    ShipmentCancelled {
        shipment_id: ShipmentId,
        reason: String,
        cancelled_at: chrono::DateTime<chrono::Utc>,
    },
    ShipmentError {
        shipment_id: ShipmentId,
        error: String,
        occurred_at: chrono::DateTime<chrono::Utc>,
    },
}

// Event conversion utilities for easier testing and serialization
impl SagaEvent {
    pub fn saga_id(&self) -> &SagaId {
        match self {
            SagaEvent::SagaStarted { saga_id, .. }
            | SagaEvent::PaymentInitiated { saga_id, .. }
            | SagaEvent::PaymentCompleted { saga_id, .. }
            | SagaEvent::PaymentFailed { saga_id, .. }
            | SagaEvent::InventoryReservationStarted { saga_id, .. }
            | SagaEvent::InventoryReserved { saga_id, .. }
            | SagaEvent::InventoryReservationFailed { saga_id, .. }
            | SagaEvent::ShippingArranged { saga_id, .. }
            | SagaEvent::ShippingFailed { saga_id, .. }
            | SagaEvent::SagaCompleted { saga_id, .. }
            | SagaEvent::SagaFailed { saga_id, .. }
            | SagaEvent::CompensationStarted { saga_id, .. }
            | SagaEvent::CompensationCompleted { saga_id, .. }
            | SagaEvent::CompensationFailed { saga_id, .. } => saga_id,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SagaEvent::SagaCompleted { .. }
                | SagaEvent::SagaFailed { .. }
                | SagaEvent::CompensationCompleted { .. }
        )
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            SagaEvent::PaymentFailed { .. }
                | SagaEvent::InventoryReservationFailed { .. }
                | SagaEvent::ShippingFailed { .. }
                | SagaEvent::SagaFailed { .. }
                | SagaEvent::CompensationFailed { .. }
        )
    }
}

impl OrderEvent {
    pub fn order_id(&self) -> &OrderId {
        match self {
            OrderEvent::OrderSubmitted { order_id, .. }
            | OrderEvent::OrderPaymentStarted { order_id, .. }
            | OrderEvent::OrderPaymentCompleted { order_id, .. }
            | OrderEvent::OrderInventoryReserved { order_id, .. }
            | OrderEvent::OrderShipped { order_id, .. }
            | OrderEvent::OrderCompleted { order_id, .. }
            | OrderEvent::OrderCancelled { order_id, .. } => order_id,
        }
    }
}

impl PaymentEvent {
    pub fn payment_id(&self) -> &PaymentId {
        match self {
            PaymentEvent::PaymentAuthorized { payment_id, .. }
            | PaymentEvent::PaymentCaptured { payment_id, .. }
            | PaymentEvent::PaymentDeclined { payment_id, .. }
            | PaymentEvent::PaymentRefunded { payment_id, .. }
            | PaymentEvent::PaymentError { payment_id, .. } => payment_id,
        }
    }
}

impl InventoryEvent {
    pub fn product_id(&self) -> &ProductId {
        match self {
            InventoryEvent::InventoryChecked { product_id, .. }
            | InventoryEvent::InventoryReserved { product_id, .. }
            | InventoryEvent::InventoryReleased { product_id, .. }
            | InventoryEvent::InventoryInsufficient { product_id, .. }
            | InventoryEvent::InventoryDeducted { product_id, .. } => product_id,
        }
    }
}

impl ShippingEvent {
    pub fn shipment_id(&self) -> &ShipmentId {
        match self {
            ShippingEvent::ShipmentCreated { shipment_id, .. }
            | ShippingEvent::ShipmentDispatched { shipment_id, .. }
            | ShippingEvent::ShipmentDelivered { shipment_id, .. }
            | ShippingEvent::ShipmentCancelled { shipment_id, .. }
            | ShippingEvent::ShipmentError { shipment_id, .. } => shipment_id,
        }
    }
}
