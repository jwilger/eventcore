//! Command implementations for the order fulfillment saga example
//!
//! This module demonstrates the saga pattern using EventCore's multi-stream
//! capabilities to coordinate distributed transactions across multiple services.

use async_trait::async_trait;
use eventcore::prelude::*;
use eventcore::{CommandLogic, CommandStreams, ReadStreams, StreamResolver, StreamWrite};
use std::collections::HashMap;

use crate::sagas::{events::*, types::*};

// ============================================================================
// Saga Coordinator Command
// ============================================================================

/// Coordinates the entire order fulfillment workflow
/// This command demonstrates the saga pattern by:
/// 1. Reading from multiple streams (order, payment, inventory, shipping)
/// 2. Orchestrating the workflow steps
/// 3. Handling failures with compensation logic
#[derive(Debug, Clone)]
pub struct OrderFulfillmentSaga {
    pub order_id: OrderId,
    pub customer_id: CustomerId,
    pub items: Vec<OrderItem>,
    pub shipping_address: ShippingAddress,
    pub payment_method: PaymentMethod,
}

#[derive(Debug, Default)]
pub struct OrderFulfillmentState {
    pub saga: Option<SagaState>,
    pub order: Option<Order>,
    pub payment: Option<PaymentDetails>,
    pub inventory_reservations: HashMap<ProductId, InventoryReservation>,
    pub shipping: Option<ShippingDetails>,
}

#[derive(Debug)]
pub struct OrderFulfillmentStreamSet;

impl CommandStreams for OrderFulfillmentSaga {
    type StreamSet = OrderFulfillmentStreamSet;

    fn read_streams(&self) -> Vec<StreamId> {
        let saga_id = SagaId::generate(); // In practice, this would be deterministic
        vec![
            StreamId::try_new(format!("saga-{}", saga_id)).unwrap(),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
            StreamId::try_new(format!("customer-{}", self.customer_id)).unwrap(),
        ]
    }
}

#[async_trait]
impl CommandLogic for OrderFulfillmentSaga {
    type State = OrderFulfillmentState;
    type Event = SagaEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            SagaEvent::SagaStarted {
                saga_id,
                order_id,
                customer_id,
                total_amount: _,
                started_at,
            } => {
                state.saga = Some(SagaState {
                    saga_id: saga_id.clone(),
                    order_id: order_id.clone(),
                    customer_id: customer_id.clone(),
                    status: SagaStatus::Started,
                    payment_id: None,
                    shipment_id: None,
                    reservations: Vec::new(),
                    compensation_actions: Vec::new(),
                    started_at: *started_at,
                    completed_at: None,
                });
            }
            SagaEvent::PaymentInitiated { payment_id, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::PaymentProcessing;
                    saga.payment_id = Some(payment_id.clone());
                }
            }
            SagaEvent::PaymentCompleted { .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::PaymentCompleted;
                }
            }
            SagaEvent::InventoryReserved { reservations, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::InventoryReserved;
                    saga.reservations = reservations.clone();
                }
            }
            SagaEvent::ShippingArranged { shipment_id, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::ShippingArranged;
                    saga.shipment_id = Some(shipment_id.clone());
                }
            }
            SagaEvent::SagaCompleted { completed_at, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::Completed;
                    saga.completed_at = Some(*completed_at);
                }
            }
            SagaEvent::SagaFailed {
                reason, failed_at, ..
            } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::Failed {
                        reason: reason.clone(),
                    };
                    saga.completed_at = Some(*failed_at);
                }
            }
            SagaEvent::CompensationStarted { actions, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::Compensating;
                    saga.compensation_actions = actions.clone();
                }
            }
            SagaEvent::CompensationCompleted { completed_at, .. } => {
                if let Some(ref mut saga) = state.saga {
                    saga.status = SagaStatus::Compensated;
                    saga.completed_at = Some(*completed_at);
                }
            }
            _ => {} // Other events don't affect saga state directly
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let saga_id = SagaId::generate();
        let saga_stream = StreamId::try_new(format!("saga-{}", saga_id))
            .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

        let mut events = Vec::new();
        let now = chrono::Utc::now();

        // If saga doesn't exist, start it
        if state.saga.is_none() {
            let total_amount = self
                .items
                .iter()
                .map(|item| item.total_price())
                .fold(Money::new(0), |acc, price| acc.add(&price));

            // Add the saga stream to read from
            stream_resolver.add_streams(vec![saga_stream.clone()]);

            events.push(StreamWrite::new(
                &read_streams,
                saga_stream.clone(),
                SagaEvent::SagaStarted {
                    saga_id: saga_id.clone(),
                    order_id: self.order_id.clone(),
                    customer_id: self.customer_id.clone(),
                    total_amount,
                    started_at: now,
                },
            )?);

            // Initiate payment processing
            let payment_id = PaymentId::generate();
            events.push(StreamWrite::new(
                &read_streams,
                saga_stream.clone(),
                SagaEvent::PaymentInitiated {
                    saga_id: saga_id.clone(),
                    payment_id: payment_id.clone(),
                    amount: total_amount,
                    method: self.payment_method.clone(),
                },
            )?);

            return Ok(events);
        }

        // Handle saga progression based on current state
        let saga = state.saga.as_ref().unwrap();
        match &saga.status {
            SagaStatus::Started => {
                // This should be handled above, but included for completeness
                Err(CommandError::BusinessRuleViolation(
                    "Saga already started but no progression events".to_string(),
                ))
            }
            SagaStatus::PaymentProcessing => {
                // In a real implementation, this would check payment status
                // For this example, we'll simulate successful payment
                if saga.payment_id.is_some() {
                    events.push(StreamWrite::new(
                        &read_streams,
                        saga_stream,
                        SagaEvent::PaymentCompleted {
                            saga_id: saga_id.clone(),
                            payment_id: saga.payment_id.as_ref().unwrap().clone(),
                            amount: self
                                .items
                                .iter()
                                .map(|item| item.total_price())
                                .fold(Money::new(0), |acc, price| acc.add(&price)),
                        },
                    )?);
                }
                Ok(events)
            }
            SagaStatus::PaymentCompleted => {
                // Start inventory reservation
                events.push(StreamWrite::new(
                    &read_streams,
                    saga_stream,
                    SagaEvent::InventoryReservationStarted {
                        saga_id: saga_id.clone(),
                        items: self.items.clone(),
                    },
                )?);
                Ok(events)
            }
            SagaStatus::InventoryReserved => {
                // Arrange shipping
                let shipment_id = ShipmentId::generate();
                events.push(StreamWrite::new(
                    &read_streams,
                    saga_stream,
                    SagaEvent::ShippingArranged {
                        saga_id: saga_id.clone(),
                        shipment_id: shipment_id.clone(),
                        address: self.shipping_address.clone(),
                        carrier: "Standard Carrier".to_string(),
                    },
                )?);
                Ok(events)
            }
            SagaStatus::ShippingArranged => {
                // Complete the saga
                events.push(StreamWrite::new(
                    &read_streams,
                    saga_stream,
                    SagaEvent::SagaCompleted {
                        saga_id: saga_id.clone(),
                        completed_at: now,
                    },
                )?);
                Ok(events)
            }
            SagaStatus::Failed { .. } => {
                // Start compensation if not already started
                let compensation_actions = vec![
                    CompensationAction::RefundPayment {
                        payment_id: saga.payment_id.clone().unwrap_or_else(PaymentId::generate),
                    },
                    CompensationAction::ReleaseInventory {
                        reservations: saga.reservations.clone(),
                    },
                ];

                events.push(StreamWrite::new(
                    &read_streams,
                    saga_stream,
                    SagaEvent::CompensationStarted {
                        saga_id: saga_id.clone(),
                        actions: compensation_actions,
                    },
                )?);
                Ok(events)
            }
            SagaStatus::Compensating => {
                // Complete compensation
                events.push(StreamWrite::new(
                    &read_streams,
                    saga_stream,
                    SagaEvent::CompensationCompleted {
                        saga_id: saga_id.clone(),
                        completed_at: now,
                    },
                )?);
                Ok(events)
            }
            SagaStatus::Completed | SagaStatus::Compensated => {
                // Saga is already complete
                Ok(vec![])
            }
        }
    }
}

// ============================================================================
// Payment Service Command
// ============================================================================

#[derive(Debug, Clone)]
pub struct ProcessPaymentCommand {
    pub payment_id: PaymentId,
    pub order_id: OrderId,
    pub amount: Money,
    pub method: PaymentMethod,
}

#[derive(Debug, Default)]
pub struct PaymentState {
    pub payment: Option<PaymentDetails>,
}

#[derive(Debug)]
pub struct PaymentStreamSet;

impl CommandStreams for ProcessPaymentCommand {
    type StreamSet = PaymentStreamSet;

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("payment-{}", self.payment_id)).unwrap(),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait]
impl CommandLogic for ProcessPaymentCommand {
    type State = PaymentState;
    type Event = PaymentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            PaymentEvent::PaymentAuthorized {
                payment_id,
                amount,
                method,
                ..
            } => {
                state.payment = Some(PaymentDetails {
                    payment_id: payment_id.clone(),
                    amount: *amount,
                    method: method.clone(),
                    status: PaymentStatus::Authorized,
                });
            }
            PaymentEvent::PaymentCaptured { .. } => {
                if let Some(ref mut payment) = state.payment {
                    payment.status = PaymentStatus::Captured;
                }
            }
            PaymentEvent::PaymentDeclined { reason, .. } => {
                if let Some(ref mut payment) = state.payment {
                    payment.status = PaymentStatus::Failed {
                        reason: reason.clone(),
                    };
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let payment_stream = StreamId::try_new(format!("payment-{}", self.payment_id))
            .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

        let mut events = Vec::new();
        let now = chrono::Utc::now();

        if state.payment.is_none() {
            // Authorize payment
            events.push(StreamWrite::new(
                &read_streams,
                payment_stream.clone(),
                PaymentEvent::PaymentAuthorized {
                    payment_id: self.payment_id.clone(),
                    order_id: self.order_id.clone(),
                    amount: self.amount,
                    method: self.method.clone(),
                    authorized_at: now,
                },
            )?);

            // Capture payment (in real implementation, this might be a separate step)
            events.push(StreamWrite::new(
                &read_streams,
                payment_stream,
                PaymentEvent::PaymentCaptured {
                    payment_id: self.payment_id.clone(),
                    amount: self.amount,
                    captured_at: now,
                },
            )?);
        }

        Ok(events)
    }
}

// ============================================================================
// Inventory Service Command
// ============================================================================

#[derive(Debug, Clone)]
pub struct ReserveInventoryCommand {
    pub order_id: OrderId,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Default)]
pub struct InventoryState {
    pub reservations: HashMap<ProductId, InventoryReservation>,
    pub available_stock: HashMap<ProductId, Quantity>,
}

#[derive(Debug)]
pub struct InventoryStreamSet;

impl CommandStreams for ReserveInventoryCommand {
    type StreamSet = InventoryStreamSet;

    fn read_streams(&self) -> Vec<StreamId> {
        let mut streams = vec![StreamId::try_new(format!("order-{}", self.order_id)).unwrap()];

        for item in &self.items {
            streams.push(StreamId::try_new(format!("inventory-{}", item.product_id)).unwrap());
        }

        streams
    }
}

#[async_trait]
impl CommandLogic for ReserveInventoryCommand {
    type State = InventoryState;
    type Event = InventoryEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            InventoryEvent::InventoryReserved {
                product_id,
                quantity,
                order_id: _,
                reserved_at,
            } => {
                state.reservations.insert(
                    product_id.clone(),
                    InventoryReservation {
                        product_id: product_id.clone(),
                        quantity: *quantity,
                        reserved_at: reserved_at.clone(),
                    },
                );
            }
            InventoryEvent::InventoryChecked {
                product_id,
                available_quantity,
                ..
            } => {
                state
                    .available_stock
                    .insert(product_id.clone(), *available_quantity);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let mut events = Vec::new();
        let now = chrono::Utc::now();

        for item in &self.items {
            let inventory_stream = StreamId::try_new(format!("inventory-{}", item.product_id))
                .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

            // Check if already reserved
            if !state.reservations.contains_key(&item.product_id) {
                // Simulate available stock (in practice, this would be read from state)
                let available = Quantity::try_new(100).unwrap(); // Simulate adequate stock

                if available >= item.quantity {
                    // Reserve inventory
                    events.push(StreamWrite::new(
                        &read_streams,
                        inventory_stream,
                        InventoryEvent::InventoryReserved {
                            product_id: item.product_id.clone(),
                            quantity: item.quantity,
                            order_id: self.order_id.clone(),
                            reserved_at: now,
                        },
                    )?);
                } else {
                    // Insufficient inventory
                    events.push(StreamWrite::new(
                        &read_streams,
                        inventory_stream,
                        InventoryEvent::InventoryInsufficient {
                            product_id: item.product_id.clone(),
                            requested: item.quantity,
                            available,
                            checked_at: now,
                        },
                    )?);
                }
            }
        }

        Ok(events)
    }
}

// ============================================================================
// Shipping Service Command
// ============================================================================

#[derive(Debug, Clone)]
pub struct ArrangeShippingCommand {
    pub order_id: OrderId,
    pub shipment_id: ShipmentId,
    pub address: ShippingAddress,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Default)]
pub struct ShippingState {
    pub shipment: Option<ShippingDetails>,
}

#[derive(Debug)]
pub struct ShippingStreamSet;

impl CommandStreams for ArrangeShippingCommand {
    type StreamSet = ShippingStreamSet;

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("shipment-{}", self.shipment_id)).unwrap(),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ]
    }
}

#[async_trait]
impl CommandLogic for ArrangeShippingCommand {
    type State = ShippingState;
    type Event = ShippingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ShippingEvent::ShipmentCreated {
                shipment_id,
                address,
                carrier,
                ..
            } => {
                state.shipment = Some(ShippingDetails {
                    shipment_id: shipment_id.clone(),
                    address: address.clone(),
                    carrier: carrier.clone(),
                    tracking_number: None,
                    status: ShippingStatus::Pending,
                });
            }
            ShippingEvent::ShipmentDispatched {
                tracking_number, ..
            } => {
                if let Some(ref mut shipment) = state.shipment {
                    shipment.tracking_number = Some(tracking_number.clone());
                    shipment.status = ShippingStatus::InTransit;
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let shipment_stream = StreamId::try_new(format!("shipment-{}", self.shipment_id))
            .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

        let mut events = Vec::new();
        let now = chrono::Utc::now();

        if state.shipment.is_none() {
            // Create shipment
            events.push(StreamWrite::new(
                &read_streams,
                shipment_stream.clone(),
                ShippingEvent::ShipmentCreated {
                    shipment_id: self.shipment_id.clone(),
                    order_id: self.order_id.clone(),
                    address: self.address.clone(),
                    carrier: "Standard Carrier".to_string(),
                    created_at: now,
                },
            )?);

            // Dispatch shipment (simulate immediate dispatch)
            events.push(StreamWrite::new(
                &read_streams,
                shipment_stream,
                ShippingEvent::ShipmentDispatched {
                    shipment_id: self.shipment_id.clone(),
                    tracking_number: format!(
                        "TRK-{}",
                        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
                    ),
                    dispatched_at: now,
                },
            )?);
        }

        Ok(events)
    }
}
