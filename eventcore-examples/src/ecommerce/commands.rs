//! E-commerce command implementations
//!
//! This module implements commands for the e-commerce order system,
//! following the aggregate-per-command pattern where each command
//! defines its own consistency boundaries.

use crate::ecommerce::{
    events::{
        EcommerceEvent, InventoryUpdatedEvent, ItemAddedToOrderEvent, OrderCancelledEvent,
        OrderCreatedEvent, OrderPlacedEvent, ProductAddedEvent,
    },
    types::{Customer, Money, OrderId, OrderItem, Product, ProductId, Quantity},
};
use async_trait::async_trait;
use eventcore::{
    Command, CommandError, CommandResult, ReadStreams, StoredEvent, StreamId, StreamWrite,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Command to add a new product to the catalog
#[derive(Debug, Clone)]
pub struct AddProductCommand;

/// Input for adding a product to the catalog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProductInput {
    /// The product to add
    pub product: Product,
    /// Initial inventory quantity
    pub initial_inventory: Quantity,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl AddProductInput {
    /// Create new input for adding a product
    pub fn new(product: Product, initial_inventory: Quantity, catalog_stream: StreamId) -> Self {
        Self {
            product,
            initial_inventory,
            catalog_stream,
        }
    }
}

/// State for product catalog and inventory
#[derive(Debug, Default, Clone)]
pub struct ProductCatalogState {
    /// Products in the catalog
    pub products: HashMap<ProductId, Product>,
    /// Current inventory levels
    pub inventory: HashMap<ProductId, Quantity>,
}

#[async_trait]
impl Command for AddProductCommand {
    type Input = AddProductInput;
    type State = ProductCatalogState;
    type Event = EcommerceEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("product-{}", input.product.id)).unwrap(),
            input.catalog_stream.clone(),
        ]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        match &stored_event.payload {
            EcommerceEvent::ProductAdded(event) => {
                state
                    .products
                    .insert(event.product.id.clone(), event.product.clone());
                state
                    .inventory
                    .insert(event.product.id.clone(), event.initial_inventory);
            }
            EcommerceEvent::InventoryUpdated(event) => {
                state
                    .inventory
                    .insert(event.product_id.clone(), event.new_quantity);
            }
            _ => {} // Ignore other events
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if product already exists
        if state.products.contains_key(&input.product.id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} already exists in catalog",
                input.product.id
            )));
        }

        let event = EcommerceEvent::ProductAdded(ProductAddedEvent {
            product: input.product.clone(),
            initial_inventory: input.initial_inventory,
        });

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("product-{}", input.product.id)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(&read_streams, input.catalog_stream.clone(), event)?,
        ])
    }
}

/// Command to create a new order
#[derive(Debug, Clone)]
pub struct CreateOrderCommand;

/// Input for creating an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderInput {
    /// The order identifier
    pub order_id: OrderId,
    /// Customer information
    pub customer: Customer,
}

impl CreateOrderInput {
    /// Create new input for creating an order
    pub fn new(order_id: OrderId, customer: Customer) -> Self {
        Self { order_id, customer }
    }
}

/// State for order management
#[derive(Debug, Default, Clone)]
pub struct OrderState {
    /// Order exists flag
    pub order_exists: bool,
    /// Customer information
    pub customer: Option<Customer>,
    /// Items in the order
    pub items: HashMap<ProductId, OrderItem>,
    /// Order status
    pub status: Option<crate::ecommerce::types::OrderStatus>,
}

#[async_trait]
impl Command for CreateOrderCommand {
    type Input = CreateOrderInput;
    type State = OrderState;
    type Event = EcommerceEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("order-{}", input.order_id)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        match &stored_event.payload {
            EcommerceEvent::OrderCreated(event) => {
                state.order_exists = true;
                state.customer = Some(event.customer.clone());
                state.status = Some(crate::ecommerce::types::OrderStatus::Draft);
            }
            EcommerceEvent::ItemAddedToOrder(event) => {
                state
                    .items
                    .insert(event.item.product_id.clone(), event.item.clone());
            }
            EcommerceEvent::ItemRemovedFromOrder(event) => {
                state.items.remove(&event.product_id);
            }
            EcommerceEvent::ItemQuantityUpdated(event) => {
                if let Some(item) = state.items.get_mut(&event.product_id) {
                    item.quantity = event.new_quantity;
                }
            }
            EcommerceEvent::OrderPlaced(_) => {
                state.status = Some(crate::ecommerce::types::OrderStatus::Placed);
            }
            EcommerceEvent::OrderCancelled(_) => {
                state.status = Some(crate::ecommerce::types::OrderStatus::Cancelled);
            }
            EcommerceEvent::OrderShipped(_) => {
                state.status = Some(crate::ecommerce::types::OrderStatus::Shipped);
            }
            EcommerceEvent::OrderDelivered(_) => {
                state.status = Some(crate::ecommerce::types::OrderStatus::Delivered);
            }
            _ => {} // Ignore other events
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order already exists
        if state.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} already exists",
                input.order_id
            )));
        }

        let event = EcommerceEvent::OrderCreated(OrderCreatedEvent {
            order_id: input.order_id.clone(),
            customer: input.customer,
        });

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            event,
        )?])
    }
}

/// Command to add an item to an order
#[derive(Debug, Clone)]
pub struct AddItemToOrderCommand;

/// Input for adding an item to an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddItemToOrderInput {
    /// The order identifier
    pub order_id: OrderId,
    /// The item to add
    pub item: OrderItem,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl AddItemToOrderInput {
    /// Create new input for adding an item to an order
    pub fn new(order_id: OrderId, item: OrderItem, catalog_stream: StreamId) -> Self {
        Self {
            order_id,
            item,
            catalog_stream,
        }
    }
}

/// Combined state for order and product catalog operations
#[derive(Debug, Default, Clone)]
pub struct OrderWithCatalogState {
    /// Order state
    pub order: OrderState,
    /// Product catalog state
    pub catalog: ProductCatalogState,
}

#[async_trait]
impl Command for AddItemToOrderCommand {
    type Input = AddItemToOrderInput;
    type State = OrderWithCatalogState;
    type Event = EcommerceEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            StreamId::try_new(format!("product-{}", input.item.product_id)).unwrap(),
            input.catalog_stream.clone(),
        ]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        // Apply order-related events to order state
        match &stored_event.payload {
            EcommerceEvent::OrderCreated(event) => {
                state.order.order_exists = true;
                state.order.customer = Some(event.customer.clone());
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Draft);
            }
            EcommerceEvent::ItemAddedToOrder(event) => {
                state
                    .order
                    .items
                    .insert(event.item.product_id.clone(), event.item.clone());
            }
            EcommerceEvent::ItemRemovedFromOrder(event) => {
                state.order.items.remove(&event.product_id);
            }
            EcommerceEvent::ItemQuantityUpdated(event) => {
                if let Some(item) = state.order.items.get_mut(&event.product_id) {
                    item.quantity = event.new_quantity;
                }
            }
            EcommerceEvent::OrderPlaced(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Placed);
            }
            EcommerceEvent::OrderCancelled(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Cancelled);
            }
            EcommerceEvent::OrderShipped(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Shipped);
            }
            EcommerceEvent::OrderDelivered(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Delivered);
            }
            _ => {}
        }

        // Apply catalog-related events to catalog state
        match &stored_event.payload {
            EcommerceEvent::ProductAdded(event) => {
                state
                    .catalog
                    .products
                    .insert(event.product.id.clone(), event.product.clone());
                state
                    .catalog
                    .inventory
                    .insert(event.product.id.clone(), event.initial_inventory);
            }
            EcommerceEvent::InventoryUpdated(event) => {
                state
                    .catalog
                    .inventory
                    .insert(event.product_id.clone(), event.new_quantity);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order exists and is in draft status
        if !state.order.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                input.order_id
            )));
        }

        if state.order.status != Some(crate::ecommerce::types::OrderStatus::Draft) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot add items to order {} - order is not in draft status",
                input.order_id
            )));
        }

        // Check if product exists in catalog
        if !state.catalog.products.contains_key(&input.item.product_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} does not exist in catalog",
                input.item.product_id
            )));
        }

        // Check inventory availability
        let available_quantity_value = state
            .catalog
            .inventory
            .get(&input.item.product_id)
            .map_or(0, Quantity::value);

        if input.item.quantity.value() > available_quantity_value {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Insufficient inventory for product {}: requested {}, available {}",
                input.item.product_id, input.item.quantity, available_quantity_value
            )));
        }

        // Check if item already exists in order
        if state.order.items.contains_key(&input.item.product_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} is already in order {}",
                input.item.product_id, input.order_id
            )));
        }

        let event = EcommerceEvent::ItemAddedToOrder(ItemAddedToOrderEvent {
            order_id: input.order_id.clone(),
            item: input.item.clone(),
        });

        // Reserve inventory
        let new_inventory_value = available_quantity_value - input.item.quantity.value();
        let new_inventory = Quantity::for_inventory(new_inventory_value)
            .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

        let previous_quantity = state
            .catalog
            .inventory
            .get(&input.item.product_id)
            .copied()
            .unwrap_or_else(|| Quantity::for_inventory(0).unwrap());

        let inventory_event = EcommerceEvent::InventoryUpdated(InventoryUpdatedEvent {
            product_id: input.item.product_id.clone(),
            previous_quantity,
            new_quantity: new_inventory,
            reason: format!("Reserved for order {}", input.order_id),
        });

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
                event,
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("product-{}", input.item.product_id)).unwrap(),
                inventory_event.clone(),
            )?,
            StreamWrite::new(&read_streams, input.catalog_stream.clone(), inventory_event)?,
        ])
    }
}

/// Command to place an order
#[derive(Debug, Clone)]
pub struct PlaceOrderCommand;

/// Input for placing an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderInput {
    /// The order identifier
    pub order_id: OrderId,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl PlaceOrderInput {
    /// Create new input for placing an order
    pub fn new(order_id: OrderId, catalog_stream: StreamId) -> Self {
        Self {
            order_id,
            catalog_stream,
        }
    }
}

#[async_trait]
impl Command for PlaceOrderCommand {
    type Input = PlaceOrderInput;
    type State = OrderState;
    type Event = EcommerceEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("order-{}", input.order_id)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        CreateOrderCommand.apply(state, stored_event);
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order exists and is in draft status
        if !state.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                input.order_id
            )));
        }

        if state.status != Some(crate::ecommerce::types::OrderStatus::Draft) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot place order {} - order is not in draft status",
                input.order_id
            )));
        }

        // Check if order has items
        if state.items.is_empty() {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot place order {} - order has no items",
                input.order_id
            )));
        }

        // Calculate total amount
        let mut total_amount = Money::from_cents(0).unwrap();
        for item in state.items.values() {
            let item_total = item
                .total_price()
                .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;
            total_amount = total_amount
                .checked_add(item_total)
                .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;
        }

        let event = EcommerceEvent::OrderPlaced(OrderPlacedEvent {
            order_id: input.order_id.clone(),
            total_amount,
            placed_at: chrono::Utc::now(),
        });

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            event,
        )?])
    }
}

/// Command to cancel an order
#[derive(Debug, Clone)]
pub struct CancelOrderCommand;

/// Input for cancelling an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderInput {
    /// The order identifier
    pub order_id: OrderId,
    /// Reason for cancellation
    pub reason: String,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl CancelOrderInput {
    /// Create new input for cancelling an order
    pub fn new(order_id: OrderId, reason: String, catalog_stream: StreamId) -> Self {
        Self {
            order_id,
            reason,
            catalog_stream,
        }
    }
}

#[async_trait]
impl Command for CancelOrderCommand {
    type Input = CancelOrderInput;
    type State = OrderWithCatalogState;
    type Event = EcommerceEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            input.catalog_stream.clone(),
        ]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        // Apply order-related events to order state
        match &stored_event.payload {
            EcommerceEvent::OrderCreated(event) => {
                state.order.order_exists = true;
                state.order.customer = Some(event.customer.clone());
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Draft);
            }
            EcommerceEvent::ItemAddedToOrder(event) => {
                state
                    .order
                    .items
                    .insert(event.item.product_id.clone(), event.item.clone());
            }
            EcommerceEvent::ItemRemovedFromOrder(event) => {
                state.order.items.remove(&event.product_id);
            }
            EcommerceEvent::ItemQuantityUpdated(event) => {
                if let Some(item) = state.order.items.get_mut(&event.product_id) {
                    item.quantity = event.new_quantity;
                }
            }
            EcommerceEvent::OrderPlaced(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Placed);
            }
            EcommerceEvent::OrderCancelled(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Cancelled);
            }
            EcommerceEvent::OrderShipped(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Shipped);
            }
            EcommerceEvent::OrderDelivered(_) => {
                state.order.status = Some(crate::ecommerce::types::OrderStatus::Delivered);
            }
            _ => {}
        }

        // Apply catalog-related events to catalog state
        match &stored_event.payload {
            EcommerceEvent::ProductAdded(event) => {
                state
                    .catalog
                    .products
                    .insert(event.product.id.clone(), event.product.clone());
                state
                    .catalog
                    .inventory
                    .insert(event.product.id.clone(), event.initial_inventory);
            }
            EcommerceEvent::InventoryUpdated(event) => {
                state
                    .catalog
                    .inventory
                    .insert(event.product_id.clone(), event.new_quantity);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order exists
        if !state.order.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                input.order_id
            )));
        }

        // Check if order can be cancelled
        match state.order.status {
            Some(crate::ecommerce::types::OrderStatus::Cancelled) => {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Order {} is already cancelled",
                    input.order_id
                )));
            }
            Some(crate::ecommerce::types::OrderStatus::Delivered) => {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Cannot cancel order {} - order has already been delivered",
                    input.order_id
                )));
            }
            _ => {} // Order can be cancelled
        }

        let mut events: Vec<StreamWrite<Self::StreamSet, Self::Event>> = vec![];

        // If order is in draft or placed status, we need to release inventory
        if matches!(
            state.order.status,
            Some(
                crate::ecommerce::types::OrderStatus::Draft
                    | crate::ecommerce::types::OrderStatus::Placed
            )
        ) {
            for item in state.order.items.values() {
                let current_inventory = state
                    .catalog
                    .inventory
                    .get(&item.product_id)
                    .copied()
                    .unwrap_or_else(|| Quantity::for_inventory(0).unwrap());

                let new_inventory = current_inventory
                    .checked_add(item.quantity)
                    .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

                let inventory_event = EcommerceEvent::InventoryUpdated(InventoryUpdatedEvent {
                    product_id: item.product_id.clone(),
                    previous_quantity: current_inventory,
                    new_quantity: new_inventory,
                    reason: format!("Released from cancelled order {}", input.order_id),
                });

                events.push(StreamWrite::new(
                    &read_streams,
                    StreamId::try_new(format!("product-{}", item.product_id)).unwrap(),
                    inventory_event.clone(),
                )?);
                events.push(StreamWrite::new(
                    &read_streams,
                    input.catalog_stream.clone(),
                    inventory_event,
                )?);
            }
        }

        let cancel_event = EcommerceEvent::OrderCancelled(OrderCancelledEvent {
            order_id: input.order_id.clone(),
            reason: input.reason,
            cancelled_at: chrono::Utc::now(),
        });

        events.push(StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            cancel_event,
        )?);

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecommerce::types::*;

    #[test]
    fn test_add_product_input_creation() {
        let product = Product::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Sku::try_new("LAPTOP-15".to_string()).unwrap(),
            ProductName::try_new("Gaming Laptop".to_string()).unwrap(),
            Money::from_cents(99999).unwrap(),
            Some("High-performance gaming laptop".to_string()),
        );

        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let input =
            AddProductInput::new(product.clone(), Quantity::new(10).unwrap(), catalog_stream);
        assert_eq!(input.product, product);
        assert_eq!(input.initial_inventory.value(), 10);
    }

    #[test]
    fn test_create_order_input_creation() {
        let order_id = OrderId::generate();
        let customer = Customer::new(
            CustomerEmail::try_new("customer@example.com".to_string()).unwrap(),
            "John Doe".to_string(),
            Some("123 Main St".to_string()),
        );

        let input = CreateOrderInput::new(order_id.clone(), customer.clone());
        assert_eq!(input.order_id, order_id);
        assert_eq!(input.customer, customer);
    }

    #[test]
    fn test_add_item_to_order_input_creation() {
        let order_id = OrderId::generate();
        let item = OrderItem::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Quantity::new(1).unwrap(),
            Money::from_cents(99999).unwrap(),
        );

        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let input = AddItemToOrderInput::new(order_id.clone(), item.clone(), catalog_stream);
        assert_eq!(input.order_id, order_id);
        assert_eq!(input.item, item);
    }

    #[test]
    fn test_place_order_input_creation() {
        let order_id = OrderId::generate();
        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let input = PlaceOrderInput::new(order_id.clone(), catalog_stream);
        assert_eq!(input.order_id, order_id);
    }

    #[test]
    fn test_cancel_order_input_creation() {
        let order_id = OrderId::generate();
        let reason = "Customer request".to_string();
        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let input = CancelOrderInput::new(order_id.clone(), reason.clone(), catalog_stream);
        assert_eq!(input.order_id, order_id);
        assert_eq!(input.reason, reason);
    }
}
