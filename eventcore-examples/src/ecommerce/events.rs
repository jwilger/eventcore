//! E-commerce domain events
//!
//! This module defines all domain events for the e-commerce order system,
//! representing state changes that have occurred in the system.

use crate::ecommerce::types::{Customer, Money, OrderId, OrderItem, Product, ProductId, Quantity};
use serde::{Deserialize, Serialize};

/// E-commerce domain events representing state changes in the order system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EcommerceEvent {
    /// A new product was added to the catalog
    ProductAdded(ProductAddedEvent),
    /// Product inventory was updated
    InventoryUpdated(InventoryUpdatedEvent),
    /// A new order was created
    OrderCreated(OrderCreatedEvent),
    /// An item was added to an order
    ItemAddedToOrder(ItemAddedToOrderEvent),
    /// An item was removed from an order
    ItemRemovedFromOrder(ItemRemovedFromOrderEvent),
    /// Item quantity was updated in an order
    ItemQuantityUpdated(ItemQuantityUpdatedEvent),
    /// An order was placed (moved from draft to placed status)
    OrderPlaced(OrderPlacedEvent),
    /// An order was cancelled
    OrderCancelled(OrderCancelledEvent),
    /// An order was shipped
    OrderShipped(OrderShippedEvent),
    /// An order was delivered
    OrderDelivered(OrderDeliveredEvent),
}

/// Event: A product was added to the catalog
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductAddedEvent {
    /// The product that was added
    pub product: Product,
    /// Initial inventory quantity
    pub initial_inventory: Quantity,
}

/// Event: Product inventory was updated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryUpdatedEvent {
    /// Product whose inventory was updated
    pub product_id: ProductId,
    /// Previous quantity in stock
    pub previous_quantity: Quantity,
    /// New quantity in stock
    pub new_quantity: Quantity,
    /// Reason for the inventory change
    pub reason: String,
}

/// Event: A new order was created
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderCreatedEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Customer information
    pub customer: Customer,
}

/// Event: An item was added to an order
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemAddedToOrderEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// The item that was added
    pub item: OrderItem,
}

/// Event: An item was removed from an order
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRemovedFromOrderEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Product ID of the item that was removed
    pub product_id: ProductId,
}

/// Event: Item quantity was updated in an order
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemQuantityUpdatedEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Product ID of the item whose quantity was updated
    pub product_id: ProductId,
    /// Previous quantity
    pub previous_quantity: Quantity,
    /// New quantity
    pub new_quantity: Quantity,
}

/// Event: An order was placed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderPlacedEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Total order amount
    pub total_amount: Money,
    /// Timestamp when the order was placed
    pub placed_at: chrono::DateTime<chrono::Utc>,
}

/// Event: An order was cancelled
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderCancelledEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Reason for cancellation
    pub reason: String,
    /// Timestamp when the order was cancelled
    pub cancelled_at: chrono::DateTime<chrono::Utc>,
}

/// Event: An order was shipped
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderShippedEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Tracking number for the shipment
    pub tracking_number: Option<String>,
    /// Timestamp when the order was shipped
    pub shipped_at: chrono::DateTime<chrono::Utc>,
}

/// Event: An order was delivered
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderDeliveredEvent {
    /// The order identifier
    pub order_id: OrderId,
    /// Timestamp when the order was delivered
    pub delivered_at: chrono::DateTime<chrono::Utc>,
}

// Implement TryFrom for compatibility with EventStore
impl TryFrom<&EcommerceEvent> for EcommerceEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecommerce::types::*;

    #[test]
    fn test_product_added_event_serialization() {
        let product = Product::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Sku::try_new("LAPTOP-15".to_string()).unwrap(),
            ProductName::try_new("Gaming Laptop".to_string()).unwrap(),
            Money::from_cents(99999).unwrap(),
            Some("High-performance gaming laptop".to_string()),
        );

        let event = EcommerceEvent::ProductAdded(ProductAddedEvent {
            product,
            initial_inventory: Quantity::new(50).unwrap(),
        });

        // Test serialization
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: EcommerceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_order_created_event_serialization() {
        let customer = Customer::new(
            CustomerEmail::try_new("customer@example.com".to_string()).unwrap(),
            "John Doe".to_string(),
            Some("123 Main St, City, State 12345".to_string()),
        );

        let event = EcommerceEvent::OrderCreated(OrderCreatedEvent {
            order_id: OrderId::generate(),
            customer,
        });

        // Test serialization
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: EcommerceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_order_placed_event_serialization() {
        let event = EcommerceEvent::OrderPlaced(OrderPlacedEvent {
            order_id: OrderId::generate(),
            total_amount: Money::from_cents(25999).unwrap(),
            placed_at: chrono::Utc::now(),
        });

        // Test serialization
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: EcommerceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_inventory_updated_event_serialization() {
        let event = EcommerceEvent::InventoryUpdated(InventoryUpdatedEvent {
            product_id: ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            previous_quantity: Quantity::new(50).unwrap(),
            new_quantity: Quantity::new(45).unwrap(),
            reason: "Order fulfilled".to_string(),
        });

        // Test serialization
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: EcommerceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_item_added_to_order_event_serialization() {
        let item = OrderItem::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Quantity::new(1).unwrap(),
            Money::from_cents(99999).unwrap(),
        );

        let event = EcommerceEvent::ItemAddedToOrder(ItemAddedToOrderEvent {
            order_id: OrderId::generate(),
            item,
        });

        // Test serialization
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: EcommerceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_event_try_from_implementation() {
        let event = EcommerceEvent::OrderCancelled(OrderCancelledEvent {
            order_id: OrderId::generate(),
            reason: "Customer request".to_string(),
            cancelled_at: chrono::Utc::now(),
        });

        let converted = EcommerceEvent::try_from(&event).unwrap();
        assert_eq!(event, converted);
    }
}
