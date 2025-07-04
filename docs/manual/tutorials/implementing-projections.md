# Tutorial: Implementing Projections

Projections are read models that transform event streams into queryable views of your data. This tutorial shows you how to build efficient, maintainable projections in EventCore.

## What Are Projections?

Projections solve the query problem in event sourcing:
- **Events** are optimized for writes and represent changes over time
- **Projections** are optimized for reads and represent current state
- **Multiple projections** can exist for the same events, optimized for different query patterns

## Prerequisites

```toml
[dependencies]
eventcore = "0.1"
eventcore-memory = "0.1"  # For testing
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
```

## Step 1: Define Your Events

Let's start with e-commerce events to demonstrate different projection patterns:

```rust
use serde::{Serialize, Deserialize};
use eventcore::types::{StreamId, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcommerceEvent {
    ProductAdded {
        name: String,
        price: u64,
        category: String,
        inventory: u32,
    },
    ProductPriceChanged {
        old_price: u64,
        new_price: u64,
    },
    OrderPlaced {
        customer_id: String,
        items: Vec<OrderItem>,
        total: u64,
    },
    OrderShipped {
        tracking_number: String,
        carrier: String,
    },
    InventoryUpdated {
        product_id: String,
        old_quantity: u32,
        new_quantity: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: String,
    pub quantity: u32,
    pub unit_price: u64,
}

impl TryFrom<&EcommerceEvent> for EcommerceEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &EcommerceEvent) -> Result<Self, Self::Error> { Ok(value.clone()) }
}
```

## Step 2: Simple Projection - Product Catalog

Let's start with a simple projection that maintains a product catalog:

```rust
use eventcore::prelude::*;
use async_trait::async_trait;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInfo {
    pub id: String,
    pub name: String,
    pub price: u64,
    pub category: String,
    pub inventory: u32,
    pub created_at: Timestamp,
    pub last_updated: Timestamp,
}

pub struct ProductCatalogProjection {
    products: HashMap<String, ProductInfo>,
    checkpoint: Option<ProjectionCheckpoint>,
}

impl ProductCatalogProjection {
    pub fn new() -> Self {
        Self {
            products: HashMap::new(),
            checkpoint: None,
        }
    }
    
    // Query methods
    pub fn get_product(&self, product_id: &str) -> Option<&ProductInfo> {
        self.products.get(product_id)
    }
    
    pub fn products_by_category(&self, category: &str) -> Vec<&ProductInfo> {
        self.products.values()
            .filter(|p| p.category == category)
            .collect()
    }
    
    pub fn products_in_stock(&self) -> Vec<&ProductInfo> {
        self.products.values()
            .filter(|p| p.inventory > 0)
            .collect()
    }
    
    pub fn products_by_price_range(&self, min: u64, max: u64) -> Vec<&ProductInfo> {
        self.products.values()
            .filter(|p| p.price >= min && p.price <= max)
            .collect()
    }
}

#[async_trait]
impl Projection for ProductCatalogProjection {
    type Event = EcommerceEvent;
    type Checkpoint = ProjectionCheckpoint;
    type Error = ProjectionError;

    async fn handle_event(&mut self, event: &StoredEvent<Self::Event>) -> ProjectionResult<()> {
        match &event.payload {
            EcommerceEvent::ProductAdded { name, price, category, inventory } => {
                let product_id = extract_product_id(&event.stream_id);
                let product = ProductInfo {
                    id: product_id.clone(),
                    name: name.clone(),
                    price: *price,
                    category: category.clone(),
                    inventory: *inventory,
                    created_at: event.timestamp,
                    last_updated: event.timestamp,
                };
                self.products.insert(product_id, product);
            }
            
            EcommerceEvent::ProductPriceChanged { new_price, .. } => {
                let product_id = extract_product_id(&event.stream_id);
                if let Some(product) = self.products.get_mut(&product_id) {
                    product.price = *new_price;
                    product.last_updated = event.timestamp;
                }
            }
            
            EcommerceEvent::InventoryUpdated { new_quantity, .. } => {
                let product_id = extract_product_id(&event.stream_id);
                if let Some(product) = self.products.get_mut(&product_id) {
                    product.inventory = *new_quantity;
                    product.last_updated = event.timestamp;
                }
            }
            
            // Ignore order events in this projection
            EcommerceEvent::OrderPlaced { .. } |
            EcommerceEvent::OrderShipped { .. } => {}
        }
        
        // Update checkpoint
        self.checkpoint = Some(ProjectionCheckpoint::new(event.id));
        Ok(())
    }

    fn checkpoint(&self) -> Option<&Self::Checkpoint> {
        self.checkpoint.as_ref()
    }

    async fn reset(&mut self) -> ProjectionResult<()> {
        self.products.clear();
        self.checkpoint = None;
        Ok(())
    }
}

// Helper function to extract product ID from stream ID
fn extract_product_id(stream_id: &StreamId) -> String {
    stream_id.as_ref().strip_prefix("product-")
        .unwrap_or(stream_id.as_ref())
        .to_string()
}
```

## Step 3: Aggregating Projection - Sales Analytics

Now let's build a more complex projection that aggregates data across multiple streams:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalesMetrics {
    pub total_orders: u64,
    pub total_revenue: u64,
    pub orders_by_status: HashMap<String, u64>,
    pub revenue_by_category: HashMap<String, u64>,
    pub top_customers: Vec<(String, u64)>, // (customer_id, total_spent)
    pub daily_revenue: HashMap<String, u64>, // date -> revenue
}

pub struct SalesAnalyticsProjection {
    metrics: SalesMetrics,
    customer_totals: HashMap<String, u64>,
    checkpoint: Option<ProjectionCheckpoint>,
}

impl SalesAnalyticsProjection {
    pub fn new() -> Self {
        Self {
            metrics: SalesMetrics {
                total_orders: 0,
                total_revenue: 0,
                orders_by_status: HashMap::new(),
                revenue_by_category: HashMap::new(),
                top_customers: Vec::new(),
                daily_revenue: HashMap::new(),
            },
            customer_totals: HashMap::new(),
            checkpoint: None,
        }
    }
    
    pub fn metrics(&self) -> &SalesMetrics {
        &self.metrics
    }
    
    pub fn revenue_for_period(&self, start_date: &str, end_date: &str) -> u64 {
        self.metrics.daily_revenue
            .iter()
            .filter(|(date, _)| *date >= start_date && *date <= end_date)
            .map(|(_, revenue)| *revenue)
            .sum()
    }
    
    fn update_top_customers(&mut self) {
        let mut customers: Vec<_> = self.customer_totals.iter()
            .map(|(id, total)| (id.clone(), *total))
            .collect();
        customers.sort_by(|a, b| b.1.cmp(&a.1));
        customers.truncate(10); // Keep top 10
        self.metrics.top_customers = customers;
    }
}

#[async_trait]
impl Projection for SalesAnalyticsProjection {
    type Event = EcommerceEvent;
    type Checkpoint = ProjectionCheckpoint;
    type Error = ProjectionError;

    async fn handle_event(&mut self, event: &StoredEvent<Self::Event>) -> ProjectionResult<()> {
        match &event.payload {
            EcommerceEvent::OrderPlaced { customer_id, items, total } => {
                // Update overall metrics
                self.metrics.total_orders += 1;
                self.metrics.total_revenue += total;
                
                // Update order status tracking
                *self.metrics.orders_by_status.entry("placed".to_string()).or_insert(0) += 1;
                
                // Update customer totals
                *self.customer_totals.entry(customer_id.clone()).or_insert(0) += total;
                self.update_top_customers();
                
                // Update daily revenue
                let date = event.timestamp.as_datetime().format("%Y-%m-%d").to_string();
                *self.metrics.daily_revenue.entry(date).or_insert(0) += total;
                
                // Update revenue by category (requires product lookup)
                for item in items {
                    // In a real system, you'd need to look up product categories
                    // This is simplified for the tutorial
                    let estimated_category = "general".to_string();
                    let item_revenue = item.quantity as u64 * item.unit_price;
                    *self.metrics.revenue_by_category.entry(estimated_category).or_insert(0) += item_revenue;
                }
            }
            
            EcommerceEvent::OrderShipped { .. } => {
                // Update order status
                *self.metrics.orders_by_status.entry("shipped".to_string()).or_insert(0) += 1;
                if let Some(placed_count) = self.metrics.orders_by_status.get_mut("placed") {
                    *placed_count = placed_count.saturating_sub(1);
                }
            }
            
            // Ignore product events in this projection
            EcommerceEvent::ProductAdded { .. } |
            EcommerceEvent::ProductPriceChanged { .. } |
            EcommerceEvent::InventoryUpdated { .. } => {}
        }
        
        self.checkpoint = Some(ProjectionCheckpoint::new(event.id));
        Ok(())
    }

    fn checkpoint(&self) -> Option<&Self::Checkpoint> {
        self.checkpoint.as_ref()
    }

    async fn reset(&mut self) -> ProjectionResult<()> {
        self.metrics = SalesMetrics {
            total_orders: 0,
            total_revenue: 0,
            orders_by_status: HashMap::new(),
            revenue_by_category: HashMap::new(),
            top_customers: Vec::new(),
            daily_revenue: HashMap::new(),
        };
        self.customer_totals.clear();
        self.checkpoint = None;
        Ok(())
    }
}
```

## Step 4: Time-Window Projection

Some projections need to maintain sliding time windows or expire old data:

```rust
use std::collections::BTreeMap;
use chrono::{DateTime, Utc, Duration};

#[derive(Debug, Clone)]
pub struct RecentActivity {
    pub event_type: String,
    pub stream_id: String,
    pub timestamp: Timestamp,
    pub data: serde_json::Value,
}

pub struct RecentActivityProjection {
    activities: BTreeMap<Timestamp, RecentActivity>,
    window_duration: Duration,
    checkpoint: Option<ProjectionCheckpoint>,
}

impl RecentActivityProjection {
    pub fn new(window_hours: i64) -> Self {
        Self {
            activities: BTreeMap::new(),
            window_duration: Duration::hours(window_hours),
            checkpoint: None,
        }
    }
    
    pub fn recent_activities(&self, limit: usize) -> Vec<&RecentActivity> {
        self.activities.values()
            .rev() // Most recent first
            .take(limit)
            .collect()
    }
    
    pub fn activities_for_stream(&self, stream_id: &str) -> Vec<&RecentActivity> {
        self.activities.values()
            .filter(|activity| activity.stream_id == stream_id)
            .collect()
    }
    
    fn cleanup_old_activities(&mut self, current_time: DateTime<Utc>) {
        let cutoff = Timestamp::new(current_time - self.window_duration);
        
        // Remove activities older than the window
        let keys_to_remove: Vec<_> = self.activities
            .range(..cutoff)
            .map(|(k, _)| *k)
            .collect();
            
        for key in keys_to_remove {
            self.activities.remove(&key);
        }
    }
}

#[async_trait]
impl Projection for RecentActivityProjection {
    type Event = EcommerceEvent;
    type Checkpoint = ProjectionCheckpoint;
    type Error = ProjectionError;

    async fn handle_event(&mut self, event: &StoredEvent<Self::Event>) -> ProjectionResult<()> {
        // Cleanup old activities first
        self.cleanup_old_activities(*event.timestamp.as_datetime());
        
        // Convert event to activity
        let event_type = match &event.payload {
            EcommerceEvent::ProductAdded { .. } => "product_added",
            EcommerceEvent::ProductPriceChanged { .. } => "price_changed",
            EcommerceEvent::OrderPlaced { .. } => "order_placed",
            EcommerceEvent::OrderShipped { .. } => "order_shipped",
            EcommerceEvent::InventoryUpdated { .. } => "inventory_updated",
        }.to_string();
        
        let activity = RecentActivity {
            event_type,
            stream_id: event.stream_id.to_string(),
            timestamp: event.timestamp,
            data: serde_json::to_value(&event.payload)
                .map_err(|e| ProjectionError::ProcessingError(e.to_string()))?,
        };
        
        self.activities.insert(event.timestamp, activity);
        self.checkpoint = Some(ProjectionCheckpoint::new(event.id));
        Ok(())
    }

    fn checkpoint(&self) -> Option<&Self::Checkpoint> {
        self.checkpoint.as_ref()
    }

    async fn reset(&mut self) -> ProjectionResult<()> {
        self.activities.clear();
        self.checkpoint = None;
        Ok(())
    }
}
```

## Step 5: Managing Projections with ProjectionManager

The `ProjectionManager` handles running projections, checkpointing, and error recovery:

```rust
use eventcore::{ProjectionManager, ProjectionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up event store
    let event_store = InMemoryEventStore::<EcommerceEvent>::new();
    
    // Create projections
    let product_catalog = ProductCatalogProjection::new();
    let sales_analytics = SalesAnalyticsProjection::new();
    let recent_activity = RecentActivityProjection::new(24); // 24-hour window
    
    // Configure projection manager
    let mut manager = ProjectionManager::new();
    
    // Add projections with different configurations
    manager.add_projection(
        "product_catalog",
        product_catalog,
        ProjectionConfig {
            batch_size: 100,
            checkpoint_frequency: 50,
            error_retry_attempts: 3,
            ..Default::default()
        }
    ).await?;
    
    manager.add_projection(
        "sales_analytics", 
        sales_analytics,
        ProjectionConfig {
            batch_size: 50,
            checkpoint_frequency: 25,
            error_retry_attempts: 5,
            ..Default::default()
        }
    ).await?;
    
    manager.add_projection(
        "recent_activity",
        recent_activity,
        ProjectionConfig {
            batch_size: 200,
            checkpoint_frequency: 100,
            error_retry_attempts: 2,
            ..Default::default()
        }
    ).await?;
    
    // Start all projections
    manager.start_all().await?;
    
    // Projections now run in the background, processing events as they arrive
    
    // Query projections
    if let Some(catalog) = manager.get_projection::<ProductCatalogProjection>("product_catalog").await {
        let products_in_electronics = catalog.products_by_category("electronics");
        println!("Electronics products: {}", products_in_electronics.len());
    }
    
    if let Some(analytics) = manager.get_projection::<SalesAnalyticsProjection>("sales_analytics").await {
        let metrics = analytics.metrics();
        println!("Total revenue: ${}", metrics.total_revenue);
        println!("Total orders: {}", metrics.total_orders);
    }
    
    // Shutdown gracefully
    manager.shutdown().await?;
    
    Ok(())
}
```

## Step 6: Testing Projections

Always test your projections to ensure they handle events correctly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::types::EventId;
    
    #[tokio::test]
    async fn test_product_catalog_projection() {
        let mut projection = ProductCatalogProjection::new();
        
        // Create test event
        let event = StoredEvent {
            id: EventId::new(),
            stream_id: StreamId::try_new("product-123").unwrap(),
            version: EventVersion::initial(),
            timestamp: Timestamp::now(),
            metadata: EventMetadata::new(),
            payload: EcommerceEvent::ProductAdded {
                name: "Test Product".to_string(),
                price: 1999,
                category: "electronics".to_string(),
                inventory: 10,
            },
        };
        
        // Process event
        projection.handle_event(&event).await.unwrap();
        
        // Verify projection state
        let product = projection.get_product("123").unwrap();
        assert_eq!(product.name, "Test Product");
        assert_eq!(product.price, 1999);
        assert_eq!(product.category, "electronics");
        assert_eq!(product.inventory, 10);
        
        // Test queries
        let electronics = projection.products_by_category("electronics");
        assert_eq!(electronics.len(), 1);
        
        let in_stock = projection.products_in_stock();
        assert_eq!(in_stock.len(), 1);
    }
    
    #[tokio::test]
    async fn test_sales_analytics_projection() {
        let mut projection = SalesAnalyticsProjection::new();
        
        // Test order placed event
        let event = StoredEvent {
            id: EventId::new(),
            stream_id: StreamId::try_new("order-456").unwrap(),
            version: EventVersion::initial(),
            timestamp: Timestamp::now(),
            metadata: EventMetadata::new(),
            payload: EcommerceEvent::OrderPlaced {
                customer_id: "customer-789".to_string(),
                items: vec![
                    OrderItem {
                        product_id: "product-123".to_string(),
                        quantity: 2,
                        unit_price: 1999,
                    }
                ],
                total: 3998,
            },
        };
        
        projection.handle_event(&event).await.unwrap();
        
        // Verify metrics
        let metrics = projection.metrics();
        assert_eq!(metrics.total_orders, 1);
        assert_eq!(metrics.total_revenue, 3998);
        assert_eq!(metrics.orders_by_status.get("placed"), Some(&1));
    }
    
    #[test]
    fn test_projection_reset() {
        tokio_test::block_on(async {
            let mut projection = ProductCatalogProjection::new();
            
            // Add some data
            let event = create_test_product_event();
            projection.handle_event(&event).await.unwrap();
            assert_eq!(projection.products.len(), 1);
            
            // Reset should clear everything
            projection.reset().await.unwrap();
            assert_eq!(projection.products.len(), 0);
            assert!(projection.checkpoint().is_none());
        });
    }
}
```

## Advanced Patterns

### Composite Projections

Sometimes you need projections that combine data from multiple other projections:

```rust
pub struct ProductRecommendationProjection {
    product_catalog: Arc<RwLock<ProductCatalogProjection>>,
    sales_analytics: Arc<RwLock<SalesAnalyticsProjection>>,
    recommendations: HashMap<String, Vec<String>>, // customer_id -> product_ids
}

impl ProductRecommendationProjection {
    pub fn recommend_products(&self, customer_id: &str) -> Vec<String> {
        // Use data from both projections to generate recommendations
        // This is a simplified example
        self.recommendations.get(customer_id)
            .cloned()
            .unwrap_or_default()
    }
}
```

### Snapshotting Large Projections

For projections with large amounts of data, implement snapshotting:

```rust
impl ProductCatalogProjection {
    pub async fn save_snapshot(&self, path: &str) -> Result<(), std::io::Error> {
        let snapshot = serde_json::to_string_pretty(&self.products)?;
        tokio::fs::write(path, snapshot).await?;
        Ok(())
    }
    
    pub async fn load_snapshot(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = tokio::fs::read_to_string(path).await?;
        self.products = serde_json::from_str(&snapshot)?;
        Ok(())
    }
}
```

## Best Practices

1. **Single Responsibility**: Each projection should serve a specific query pattern
2. **Idempotent Processing**: Handle duplicate events gracefully
3. **Error Handling**: Implement proper error recovery and retry logic
4. **Performance**: Use appropriate data structures for your query patterns
5. **Checkpointing**: Save progress frequently to enable fast restarts
6. **Testing**: Test with realistic event sequences and edge cases
7. **Monitoring**: Track projection lag and processing performance

Projections are a powerful tool for building queryable views of your event-sourced data. They enable you to optimize for different read patterns while keeping your write side simple and focused on business logic.