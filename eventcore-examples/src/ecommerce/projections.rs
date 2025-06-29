//! E-commerce projections for read models
//!
//! This module implements projections that maintain read models
//! for querying e-commerce data efficiently.

use crate::ecommerce::{
    events::EcommerceEvent,
    types::{CustomerEmail, Money, OrderId, OrderStatus, Product, ProductId, Quantity},
};
use eventcore::{
    Event, Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionError, ProjectionResult,
    ProjectionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Inventory projection state tracking product availability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryProjectionState {
    /// Current inventory levels by product ID
    pub inventory: HashMap<ProductId, Quantity>,
    /// Product catalog information
    pub products: HashMap<ProductId, Product>,
    /// Total number of products in catalog
    pub total_products: usize,
    /// Total value of inventory
    pub total_inventory_value: Money,
}

impl InventoryProjectionState {
    /// Get available quantity for a product
    pub fn get_available_quantity(&self, product_id: &ProductId) -> Option<Quantity> {
        self.inventory.get(product_id).copied()
    }

    /// Get product information
    pub fn get_product(&self, product_id: &ProductId) -> Option<&Product> {
        self.products.get(product_id)
    }

    /// Get all products with their current inventory
    pub fn get_products_with_inventory(&self) -> Vec<(Product, Quantity)> {
        self.products
            .values()
            .filter_map(|product| {
                self.inventory
                    .get(&product.id)
                    .map(|&quantity| (product.clone(), quantity))
            })
            .collect()
    }

    /// Get products with low inventory (less than threshold)
    pub fn get_low_inventory_products(&self, threshold: u32) -> Vec<(Product, Quantity)> {
        self.get_products_with_inventory()
            .into_iter()
            .filter(|(_, quantity)| quantity.value() < threshold)
            .collect()
    }

    /// Check if a product is in stock
    pub fn is_in_stock(&self, product_id: &ProductId) -> bool {
        self.inventory
            .get(product_id)
            .is_some_and(|qty| qty.value() > 0)
    }
}

/// Inventory projection implementation
#[derive(Debug)]
pub struct InventoryProjectionImpl {
    config: ProjectionConfig,
}

impl InventoryProjectionImpl {
    /// Create a new inventory projection
    pub fn new() -> Self {
        Self {
            config: ProjectionConfig::new("inventory"),
        }
    }

    /// Recalculate total inventory value
    fn recalculate_total_value(state: &mut InventoryProjectionState) -> ProjectionResult<()> {
        let mut total_value = Money::from_cents(0)
            .map_err(|e| ProjectionError::Internal(format!("Failed to initialize money: {}", e)))?;

        for (product_id, quantity) in &state.inventory {
            if let Some(product) = state.products.get(product_id) {
                let item_value = product.price.multiply_by_quantity(*quantity).map_err(|e| {
                    ProjectionError::Internal(format!("Failed to calculate item value: {}", e))
                })?;
                total_value = total_value.checked_add(item_value).map_err(|e| {
                    ProjectionError::Internal(format!("Failed to add to total value: {}", e))
                })?;
            }
        }

        state.total_inventory_value = total_value;
        Ok(())
    }
}

impl Default for InventoryProjectionImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Projection for InventoryProjectionImpl {
    type Event = EcommerceEvent;
    type State = InventoryProjectionState;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        // This method is not used in the new design, but we need to provide a default
        Ok(InventoryProjectionState::default())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        Ok(ProjectionStatus::Running)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        Ok(ProjectionCheckpoint::initial())
    }

    async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        Ok(())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        match &event.payload {
            EcommerceEvent::ProductAdded(e) => {
                state
                    .products
                    .insert(e.product.id.clone(), e.product.clone());
                state
                    .inventory
                    .insert(e.product.id.clone(), e.initial_inventory);
                state.total_products = state.products.len();
                Self::recalculate_total_value(state)?;
            }
            EcommerceEvent::InventoryUpdated(e) => {
                state.inventory.insert(e.product_id.clone(), e.new_quantity);
                Self::recalculate_total_value(state)?;
            }
            _ => {
                // Ignore other events - this projection only cares about product and inventory events
            }
        }
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(InventoryProjectionState::default())
    }
}

/// Order summary projection state for order analytics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderSummaryProjectionState {
    /// Orders by status
    pub orders_by_status: HashMap<OrderStatus, usize>,
    /// Total orders created
    pub total_orders: usize,
    /// Total revenue from placed orders
    pub total_revenue: Money,
    /// Orders by customer email
    pub orders_by_customer: HashMap<CustomerEmail, Vec<OrderId>>,
    /// Average order value
    pub average_order_value: Money,
}

impl OrderSummaryProjectionState {
    /// Get number of orders in a specific status
    pub fn get_orders_count_by_status(&self, status: &OrderStatus) -> usize {
        self.orders_by_status.get(status).copied().unwrap_or(0)
    }

    /// Get orders for a specific customer
    pub fn get_customer_orders(&self, email: &CustomerEmail) -> Vec<OrderId> {
        self.orders_by_customer
            .get(email)
            .cloned()
            .unwrap_or_default()
    }

    /// Get total number of customers
    pub fn get_total_customers(&self) -> usize {
        self.orders_by_customer.len()
    }
}

/// Order summary projection implementation  
#[derive(Debug)]
pub struct OrderSummaryProjectionImpl {
    config: ProjectionConfig,
}

impl OrderSummaryProjectionImpl {
    /// Create a new order summary projection
    pub fn new() -> Self {
        Self {
            config: ProjectionConfig::new("order_summary"),
        }
    }

    /// Recalculate average order value
    fn recalculate_average_order_value(
        state: &mut OrderSummaryProjectionState,
        placed_order_count: usize,
    ) -> ProjectionResult<()> {
        if placed_order_count > 0 {
            let total_cents = state.total_revenue.to_cents();
            let average_cents = total_cents / placed_order_count as u64;
            state.average_order_value = Money::from_cents(average_cents).map_err(|e| {
                ProjectionError::Internal(format!("Failed to calculate average order value: {}", e))
            })?;
        } else {
            state.average_order_value = Money::from_cents(0).map_err(|e| {
                ProjectionError::Internal(format!(
                    "Failed to initialize average order value: {}",
                    e
                ))
            })?;
        }
        Ok(())
    }
}

impl Default for OrderSummaryProjectionImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Projection for OrderSummaryProjectionImpl {
    type Event = EcommerceEvent;
    type State = OrderSummaryProjectionState;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        // This method is not used in the new design, but we need to provide a default
        Ok(OrderSummaryProjectionState::default())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        Ok(ProjectionStatus::Running)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        Ok(ProjectionCheckpoint::initial())
    }

    async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        Ok(())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        // We need to track placed order count for average calculation
        // Since we can't store it in self, we'll calculate it from state
        let _placed_order_count = state.get_orders_count_by_status(&OrderStatus::Placed);

        match &event.payload {
            EcommerceEvent::OrderCreated(e) => {
                state.total_orders += 1;
                *state
                    .orders_by_status
                    .entry(OrderStatus::Draft)
                    .or_insert(0) += 1;

                state
                    .orders_by_customer
                    .entry(e.customer.email.clone())
                    .or_default()
                    .push(e.order_id.clone());
            }
            EcommerceEvent::OrderPlaced(e) => {
                // Update status counts
                if let Some(draft_count) = state.orders_by_status.get_mut(&OrderStatus::Draft) {
                    *draft_count = draft_count.saturating_sub(1);
                }
                *state
                    .orders_by_status
                    .entry(OrderStatus::Placed)
                    .or_insert(0) += 1;

                // Update revenue
                state.total_revenue =
                    state
                        .total_revenue
                        .checked_add(e.total_amount)
                        .map_err(|e| {
                            ProjectionError::Internal(format!(
                                "Failed to add to total revenue: {}",
                                e
                            ))
                        })?;

                let new_placed_count = state.get_orders_count_by_status(&OrderStatus::Placed);
                Self::recalculate_average_order_value(state, new_placed_count)?;
            }
            EcommerceEvent::OrderCancelled(_) => {
                // Update status counts - we don't know which status it came from,
                // so we'll just increment cancelled count
                *state
                    .orders_by_status
                    .entry(OrderStatus::Cancelled)
                    .or_insert(0) += 1;
            }
            EcommerceEvent::OrderShipped(_) => {
                // Update status counts
                if let Some(placed_count) = state.orders_by_status.get_mut(&OrderStatus::Placed) {
                    *placed_count = placed_count.saturating_sub(1);
                }
                *state
                    .orders_by_status
                    .entry(OrderStatus::Shipped)
                    .or_insert(0) += 1;
            }
            EcommerceEvent::OrderDelivered(_) => {
                // Update status counts
                if let Some(shipped_count) = state.orders_by_status.get_mut(&OrderStatus::Shipped) {
                    *shipped_count = shipped_count.saturating_sub(1);
                }
                *state
                    .orders_by_status
                    .entry(OrderStatus::Delivered)
                    .or_insert(0) += 1;
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(OrderSummaryProjectionState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecommerce::types::{ProductName, Sku};
    use eventcore::EventMetadata;

    fn create_test_event<T>(payload: T) -> Event<T>
    where
        T: PartialEq + Eq,
    {
        Event::new(
            eventcore::StreamId::try_new("test-stream".to_string()).unwrap(),
            payload,
            EventMetadata::default(),
        )
    }

    #[tokio::test]
    async fn test_inventory_projection_product_added() {
        let projection = InventoryProjectionImpl::new();
        let mut state = projection.initialize_state().await.unwrap();

        let product = Product::new(
            ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap(),
            Sku::try_new("LAPTOP-15".to_string()).unwrap(),
            ProductName::try_new("Gaming Laptop".to_string()).unwrap(),
            Money::from_cents(99999).unwrap(),
            Some("High-performance gaming laptop".to_string()),
        );

        let event = create_test_event(EcommerceEvent::ProductAdded(
            crate::ecommerce::events::ProductAddedEvent {
                product: product.clone(),
                initial_inventory: Quantity::new(10).unwrap(),
            },
        ));

        projection.apply_event(&mut state, &event).await.unwrap();

        assert_eq!(state.total_products, 1);
        assert_eq!(
            state.get_available_quantity(&product.id),
            Some(Quantity::new(10).unwrap())
        );
        assert_eq!(state.get_product(&product.id), Some(&product));
        assert!(state.is_in_stock(&product.id));
    }

    #[tokio::test]
    async fn test_inventory_projection_inventory_updated() {
        let projection = InventoryProjectionImpl::new();
        let mut state = projection.initialize_state().await.unwrap();

        let product_id = ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap();

        // First add a product
        let product = Product::new(
            product_id.clone(),
            Sku::try_new("LAPTOP-15".to_string()).unwrap(),
            ProductName::try_new("Gaming Laptop".to_string()).unwrap(),
            Money::from_cents(99999).unwrap(),
            Some("High-performance gaming laptop".to_string()),
        );

        let add_event = create_test_event(EcommerceEvent::ProductAdded(
            crate::ecommerce::events::ProductAddedEvent {
                product,
                initial_inventory: Quantity::new(10).unwrap(),
            },
        ));

        projection
            .apply_event(&mut state, &add_event)
            .await
            .unwrap();

        // Now update inventory
        let update_event = create_test_event(EcommerceEvent::InventoryUpdated(
            crate::ecommerce::events::InventoryUpdatedEvent {
                product_id: product_id.clone(),
                previous_quantity: Quantity::new(10).unwrap(),
                new_quantity: Quantity::new(5).unwrap(),
                reason: "Order fulfilled".to_string(),
            },
        ));

        projection
            .apply_event(&mut state, &update_event)
            .await
            .unwrap();

        assert_eq!(
            state.get_available_quantity(&product_id),
            Some(Quantity::new(5).unwrap())
        );
    }

    // Additional projection tests would go here
    // For now, the main tests are in the integration test suite
}
