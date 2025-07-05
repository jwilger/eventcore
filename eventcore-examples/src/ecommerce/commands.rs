//! E-commerce command implementations
//!
//! This module implements commands for the e-commerce order system,
//! using multi-stream event sourcing where each command defines
//! its own consistency boundaries across multiple streams.

use crate::ecommerce::{
    events::{
        EcommerceEvent, InventoryUpdatedEvent, ItemAddedToOrderEvent, OrderCancelledEvent,
        OrderCreatedEvent, OrderPlacedEvent, ProductAddedEvent,
    },
    types::{Customer, Money, OrderId, OrderItem, Product, ProductId, Quantity},
};
use async_trait::async_trait;
use eventcore::{
    CommandError, CommandLogic, CommandResult, CommandStreams, ReadStreams, StoredEvent, StreamId,
    StreamWrite,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Command to add a new product to the catalog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProductCommand {
    /// The product to add
    pub product: Product,
    /// Initial inventory quantity
    pub initial_inventory: Quantity,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl AddProductCommand {
    /// Create new command for adding a product
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

impl CommandStreams for AddProductCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("product-{}", self.product.id)).unwrap(),
            self.catalog_stream.clone(),
        ]
    }
}

#[async_trait]
impl CommandLogic for AddProductCommand {
    type State = ProductCatalogState;
    type Event = EcommerceEvent;

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
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if product already exists
        if state.products.contains_key(&self.product.id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} already exists in catalog",
                self.product.id
            )));
        }

        let event = EcommerceEvent::ProductAdded(ProductAddedEvent {
            product: self.product.clone(),
            initial_inventory: self.initial_inventory,
        });

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("product-{}", self.product.id)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(&read_streams, self.catalog_stream.clone(), event)?,
        ])
    }
}

/// Command to create a new order
///
/// This command demonstrates the simplified pattern where the command struct
/// serves as its own input type, with all fields being validated domain types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderCommand {
    /// The order identifier
    pub order_id: OrderId,
    /// Customer information
    pub customer: Customer,
}

impl CreateOrderCommand {
    /// Create new command for creating an order
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

impl CommandStreams for CreateOrderCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("order-{}", self.order_id)).unwrap()]
    }
}

#[async_trait]
impl CommandLogic for CreateOrderCommand {
    type State = OrderState;
    type Event = EcommerceEvent;

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
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order already exists
        if state.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} already exists",
                self.order_id
            )));
        }

        let event = EcommerceEvent::OrderCreated(OrderCreatedEvent {
            order_id: self.order_id.clone(),
            customer: self.customer.clone(),
        });

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
            event,
        )?])
    }
}

/// Command to add an item to an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddItemToOrderCommand {
    /// The order identifier
    pub order_id: OrderId,
    /// The item to add
    pub item: OrderItem,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl AddItemToOrderCommand {
    /// Create new command for adding an item to an order
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

impl CommandStreams for AddItemToOrderCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
            StreamId::try_new(format!("product-{}", self.item.product_id)).unwrap(),
            self.catalog_stream.clone(),
        ]
    }
}

#[async_trait]
impl CommandLogic for AddItemToOrderCommand {
    type State = OrderWithCatalogState;
    type Event = EcommerceEvent;

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
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order exists and is in draft status
        if !state.order.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                self.order_id
            )));
        }

        if state.order.status != Some(crate::ecommerce::types::OrderStatus::Draft) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot add items to order {} - order is not in draft status",
                self.order_id
            )));
        }

        // Check if product exists in catalog
        if !state.catalog.products.contains_key(&self.item.product_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} does not exist in catalog",
                self.item.product_id
            )));
        }

        // Check inventory availability
        let available_quantity_value = state
            .catalog
            .inventory
            .get(&self.item.product_id)
            .map_or(0, Quantity::value);

        if self.item.quantity.value() > available_quantity_value {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Insufficient inventory for product {}: requested {}, available {}",
                self.item.product_id, self.item.quantity, available_quantity_value
            )));
        }

        // Check if item already exists in order
        if state.order.items.contains_key(&self.item.product_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product {} is already in order {}",
                self.item.product_id, self.order_id
            )));
        }

        let event = EcommerceEvent::ItemAddedToOrder(ItemAddedToOrderEvent {
            order_id: self.order_id.clone(),
            item: self.item.clone(),
        });

        // Reserve inventory
        let new_inventory_value = available_quantity_value - self.item.quantity.value();
        let new_inventory = Quantity::for_inventory(new_inventory_value)
            .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;

        let previous_quantity = state
            .catalog
            .inventory
            .get(&self.item.product_id)
            .copied()
            .unwrap_or_else(|| Quantity::for_inventory(0).unwrap());

        let inventory_event = EcommerceEvent::InventoryUpdated(InventoryUpdatedEvent {
            product_id: self.item.product_id.clone(),
            previous_quantity,
            new_quantity: new_inventory,
            reason: format!("Reserved for order {}", self.order_id),
        });

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
                event,
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("product-{}", self.item.product_id)).unwrap(),
                inventory_event.clone(),
            )?,
            StreamWrite::new(&read_streams, self.catalog_stream.clone(), inventory_event)?,
        ])
    }
}

/// Command to place an order
///
/// This command demonstrates the simplified pattern. Note that while the input
/// includes a catalog_stream for consistency with the example, it's not actually
/// used in read_streams since we only need to read/write the order stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderCommand {
    /// The order identifier
    pub order_id: OrderId,
    /// The catalog stream to use (kept for API compatibility but not used as a read stream)
    pub catalog_stream: StreamId,
}

impl PlaceOrderCommand {
    /// Create new command for placing an order
    pub fn new(order_id: OrderId, catalog_stream: StreamId) -> Self {
        Self {
            order_id,
            catalog_stream,
        }
    }
}

impl CommandStreams for PlaceOrderCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("order-{}", self.order_id)).unwrap()]
    }
}

#[async_trait]
impl CommandLogic for PlaceOrderCommand {
    type State = OrderState;
    type Event = EcommerceEvent;

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
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if order exists and is in draft status
        if !state.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                self.order_id
            )));
        }

        if state.status != Some(crate::ecommerce::types::OrderStatus::Draft) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot place order {} - order is not in draft status",
                self.order_id
            )));
        }

        // Check if order has items
        if state.items.is_empty() {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot place order {} - order has no items",
                self.order_id
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
            order_id: self.order_id.clone(),
            total_amount,
            placed_at: chrono::Utc::now(),
        });

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
            event,
        )?])
    }
}

/// Command to cancel an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderCommand {
    /// The order identifier
    pub order_id: OrderId,
    /// Reason for cancellation
    pub reason: String,
    /// The catalog stream to use
    pub catalog_stream: StreamId,
}

impl CancelOrderCommand {
    /// Create new command for cancelling an order
    pub fn new(order_id: OrderId, reason: String, catalog_stream: StreamId) -> Self {
        Self {
            order_id,
            reason,
            catalog_stream,
        }
    }
}

impl CommandStreams for CancelOrderCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        // Initial streams: order and catalog
        vec![
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
            self.catalog_stream.clone(),
        ]
    }
}

#[async_trait]
impl CommandLogic for CancelOrderCommand {
    type State = OrderWithCatalogState;
    type Event = EcommerceEvent;

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
        stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Dynamically discover and request product streams for items in the order
        if !state.order.items.is_empty() {
            // Check if we already have all the product streams in our read_streams
            let missing_streams: Vec<_> = state
                .order
                .items
                .keys()
                .map(|product_id| StreamId::try_new(format!("product-{}", product_id)).unwrap())
                .filter(|stream| !read_streams.stream_ids().contains(stream))
                .collect();

            if !missing_streams.is_empty() {
                // Request additional product streams - executor will re-read and rebuild state
                stream_resolver.add_streams(missing_streams);
                return Ok(vec![]); // Return early to trigger re-read
            }
        }

        // Check if order exists
        if !state.order.order_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                self.order_id
            )));
        }

        // Check if order can be cancelled
        match state.order.status {
            Some(crate::ecommerce::types::OrderStatus::Cancelled) => {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Order {} is already cancelled",
                    self.order_id
                )));
            }
            Some(crate::ecommerce::types::OrderStatus::Delivered) => {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Cannot cancel order {} - order has already been delivered",
                    self.order_id
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
                    reason: format!("Released from cancelled order {}", self.order_id),
                });

                // Write to both product stream and catalog stream since we now declare both
                events.push(StreamWrite::new(
                    &read_streams,
                    StreamId::try_new(format!("product-{}", item.product_id)).unwrap(),
                    inventory_event.clone(),
                )?);
                events.push(StreamWrite::new(
                    &read_streams,
                    self.catalog_stream.clone(),
                    inventory_event,
                )?);
            }
        }

        let cancel_event = EcommerceEvent::OrderCancelled(OrderCancelledEvent {
            order_id: self.order_id.clone(),
            reason: self.reason.clone(),
            cancelled_at: chrono::Utc::now(),
        });

        events.push(StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
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
    fn test_add_product_command_creation() {
        let product = Product::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Sku::try_new("LAPTOP-15".to_string()).unwrap(),
            ProductName::try_new("Gaming Laptop".to_string()).unwrap(),
            Money::from_cents(99999).unwrap(),
            Some("High-performance gaming laptop".to_string()),
        );

        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let command =
            AddProductCommand::new(product.clone(), Quantity::new(10).unwrap(), catalog_stream);
        assert_eq!(command.product, product);
        assert_eq!(command.initial_inventory.value(), 10);
    }

    #[test]
    fn test_create_order_command_creation() {
        let order_id = OrderId::generate();
        let customer = Customer::new(
            CustomerEmail::try_new("customer@example.com".to_string()).unwrap(),
            "John Doe".to_string(),
            Some("123 Main St".to_string()),
        );

        let command = CreateOrderCommand::new(order_id.clone(), customer.clone());
        assert_eq!(command.order_id, order_id);
        assert_eq!(command.customer, customer);
    }

    #[test]
    fn test_add_item_to_order_command_creation() {
        let order_id = OrderId::generate();
        let item = OrderItem::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Quantity::new(1).unwrap(),
            Money::from_cents(99999).unwrap(),
        );

        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let command = AddItemToOrderCommand::new(order_id.clone(), item.clone(), catalog_stream);
        assert_eq!(command.order_id, order_id);
        assert_eq!(command.item, item);
    }

    #[test]
    fn test_place_order_command_creation() {
        let order_id = OrderId::generate();
        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let command = PlaceOrderCommand::new(order_id.clone(), catalog_stream);
        assert_eq!(command.order_id, order_id);
    }

    #[test]
    fn test_cancel_order_command_creation() {
        let order_id = OrderId::generate();
        let reason = "Customer request".to_string();
        let catalog_stream = StreamId::try_new("test-catalog".to_string()).unwrap();
        let command = CancelOrderCommand::new(order_id.clone(), reason.clone(), catalog_stream);
        assert_eq!(command.order_id, order_id);
        assert_eq!(command.reason, reason);
    }
}
