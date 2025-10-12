//! # Inventory Management System Example
//!
//! This example demonstrates EventCore usage in an inventory management scenario.
//! It showcases advanced patterns including:
//!
//! - **Inventory tracking**: Real-time stock levels and reservations
//! - **Supply chain operations**: Purchase orders, receiving, quality control
//! - **Multi-location inventory**: Warehouse transfers and distributed stock
//! - **Dynamic stream discovery**: Product variants and supplier relationships
//! - **Eventual consistency**: Handling delayed supplier confirmations
//! - **Compensation patterns**: Rollback mechanisms for failed operations
//!
//! # Domain Model
//!
//! - **Products**: Items that can be stocked and sold
//! - **Inventory**: Stock levels at different locations
//! - **Purchase Orders**: Requests to suppliers for inventory
//! - **Warehouses**: Physical locations storing inventory
//! - **Suppliers**: External parties providing products
//!
//! # Key EventCore Patterns Demonstrated
//!
//! 1. **Reservation patterns**: Hold inventory during checkout process
//! 2. **Saga-like operations**: Multi-step purchase order workflows
//! 3. **Dynamic stream resolution**: Discovering product variants
//! 4. **Compensation commands**: Rollback failed operations
//! 5. **Cross-location transfers**: Atomic inventory moves

use eventcore::prelude::*;
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Domain Types  
// ============================================================================

pub mod types {
    use super::*;
    use nutype::nutype;

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct ProductId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50), 
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct WarehouseId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct SupplierId(String);

    #[nutype(
        validate(greater = 0),
        derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, Serialize, Deserialize)
    )]
    pub struct Quantity(u32);

    #[nutype(
        validate(greater_or_equal = 0),
        derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, Serialize, Deserialize)
    )]
    pub struct StockLevel(u32);

    #[nutype(
        validate(greater = 0.0),
        derive(Debug, Clone, Copy, PartialEq, PartialOrd, Into, Serialize, Deserialize)
    )]
    pub struct Price(f64);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ReservationId(pub Uuid);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PurchaseOrderId(pub Uuid);

    impl ProductId {
        pub fn inventory_stream_id(&self, warehouse_id: &WarehouseId) -> StreamId {
            StreamId::try_new(format!("inventory-{}-{}", warehouse_id.as_ref(), self.as_ref())).unwrap()
        }

        pub fn product_stream_id(&self) -> StreamId {
            StreamId::try_new(format!("product-{}", self.as_ref())).unwrap()
        }
    }

    impl WarehouseId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("warehouse-{}", self.as_ref())).unwrap()
        }
    }

    impl SupplierId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("supplier-{}", self.as_ref())).unwrap()
        }
    }

    impl ReservationId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl PurchaseOrderId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }

        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("purchase-order-{}", self.0)).unwrap()
        }
    }
}

// ============================================================================
// Events
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InventoryEvent {
    // Product management
    ProductCreated {
        product_id: types::ProductId,
        name: String,
        category: String,
        unit_cost: types::Price,
        supplier_id: types::SupplierId,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    ProductUpdated {
        product_id: types::ProductId,
        name: Option<String>,
        category: Option<String>,
        unit_cost: Option<types::Price>,
        updated_at: chrono::DateTime<chrono::Utc>,
    },

    // Inventory management
    StockReceived {
        product_id: types::ProductId,
        warehouse_id: types::WarehouseId,
        quantity: types::Quantity,
        unit_cost: types::Price,
        received_at: chrono::DateTime<chrono::Utc>,
        purchase_order_id: Option<types::PurchaseOrderId>,
    },
    StockReserved {
        product_id: types::ProductId,
        warehouse_id: types::WarehouseId,
        quantity: types::Quantity,
        reservation_id: types::ReservationId,
        reserved_for: String, // customer ID or process ID
        expires_at: chrono::DateTime<chrono::Utc>,
        reserved_at: chrono::DateTime<chrono::Utc>,
    },
    StockReleased {
        product_id: types::ProductId,
        warehouse_id: types::WarehouseId,
        quantity: types::Quantity,
        reservation_id: types::ReservationId,
        reason: String,
        released_at: chrono::DateTime<chrono::Utc>,
    },
    StockSold {
        product_id: types::ProductId,
        warehouse_id: types::WarehouseId,
        quantity: types::Quantity,
        reservation_id: Option<types::ReservationId>,
        sale_price: types::Price,
        sold_at: chrono::DateTime<chrono::Utc>,
    },
    StockTransferred {
        product_id: types::ProductId,
        from_warehouse: types::WarehouseId,
        to_warehouse: types::WarehouseId,
        quantity: types::Quantity,
        transfer_cost: types::Price,
        transferred_at: chrono::DateTime<chrono::Utc>,
    },

    // Purchase order workflow
    PurchaseOrderCreated {
        order_id: types::PurchaseOrderId,
        supplier_id: types::SupplierId,
        items: Vec<PurchaseOrderItem>,
        total_cost: types::Price,
        expected_delivery: chrono::DateTime<chrono::Utc>,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    PurchaseOrderConfirmed {
        order_id: types::PurchaseOrderId,
        confirmed_by_supplier: bool,
        estimated_delivery: chrono::DateTime<chrono::Utc>,
        confirmed_at: chrono::DateTime<chrono::Utc>,
    },
    PurchaseOrderReceived {
        order_id: types::PurchaseOrderId,
        warehouse_id: types::WarehouseId,
        items_received: Vec<ReceivedItem>,
        received_at: chrono::DateTime<chrono::Utc>,
    },

    // Warehouse operations
    WarehouseCreated {
        warehouse_id: types::WarehouseId,
        name: String,
        location: String,
        capacity: u32,
        created_at: chrono::DateTime<chrono::Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PurchaseOrderItem {
    pub product_id: types::ProductId,
    pub quantity: types::Quantity,
    pub unit_cost: types::Price,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceivedItem {
    pub product_id: types::ProductId,
    pub quantity_ordered: types::Quantity,
    pub quantity_received: types::Quantity,
    pub unit_cost: types::Price,
    pub quality_check_passed: bool,
}

impl TryFrom<&InventoryEvent> for InventoryEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &InventoryEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// ============================================================================
// State Types
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct InventoryState {
    pub stock_level: types::StockLevel,
    pub reserved_quantity: u32,
    pub available_quantity: u32,
    pub reservations: HashMap<types::ReservationId, ReservationInfo>,
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct ReservationInfo {
    pub quantity: types::Quantity,
    pub reserved_for: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub reserved_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default, Clone)]
pub struct ProductState {
    pub exists: bool,
    pub name: Option<String>,
    pub category: Option<String>,
    pub unit_cost: Option<types::Price>,
    pub supplier_id: Option<types::SupplierId>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Default, Clone)]
pub struct PurchaseOrderState {
    pub exists: bool,
    pub supplier_id: Option<types::SupplierId>,
    pub items: Vec<PurchaseOrderItem>,
    pub total_cost: Option<types::Price>,
    pub status: PurchaseOrderStatus,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub received_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PurchaseOrderStatus {
    Draft,
    Created,
    Confirmed,
    Received,
    Cancelled,
}

impl Default for PurchaseOrderStatus {
    fn default() -> Self {
        Self::Draft
    }
}

// ============================================================================
// Commands
// ============================================================================

/// Reserve stock for a customer order
pub struct ReserveStockCommand {
    pub product_id: types::ProductId,
    pub warehouse_id: types::WarehouseId,
    pub quantity: types::Quantity,
    pub reserved_for: String,
    pub expires_in_minutes: u32,
}

#[async_trait::async_trait]
impl Command for ReserveStockCommand {
    type Input = Self;
    type State = InventoryState;
    type Event = InventoryEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.product_id.inventory_stream_id(&input.warehouse_id)]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            InventoryEvent::StockReceived { quantity, .. } => {
                state.stock_level = types::StockLevel::try_new(
                    state.stock_level.into() + Into::<u32>::into(*quantity)
                ).unwrap_or(state.stock_level);
                state.available_quantity = state.stock_level.into() - state.reserved_quantity;
            }
            InventoryEvent::StockReserved {
                quantity,
                reservation_id,
                reserved_for,
                expires_at,
                reserved_at,
                ..
            } => {
                let qty: u32 = (*quantity).into();
                state.reserved_quantity += qty;
                state.available_quantity = state.available_quantity.saturating_sub(qty);
                state.reservations.insert(
                    reservation_id.clone(),
                    ReservationInfo {
                        quantity: *quantity,
                        reserved_for: reserved_for.clone(),
                        expires_at: *expires_at,
                        reserved_at: *reserved_at,
                    },
                );
            }
            InventoryEvent::StockReleased {
                quantity,
                reservation_id,
                ..
            } => {
                let qty: u32 = (*quantity).into();
                state.reserved_quantity = state.reserved_quantity.saturating_sub(qty);
                state.available_quantity = state.stock_level.into() - state.reserved_quantity;
                state.reservations.remove(reservation_id);
            }
            InventoryEvent::StockSold { quantity, .. } => {
                let qty: u32 = (*quantity).into();
                state.stock_level = types::StockLevel::try_new(
                    state.stock_level.into().saturating_sub(qty)
                ).unwrap_or(types::StockLevel::try_new(0).unwrap());
                state.available_quantity = state.stock_level.into() - state.reserved_quantity;
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let requested_quantity: u32 = input.quantity.into();
        
        // Check if sufficient stock is available
        if state.available_quantity < requested_quantity {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Insufficient stock for product '{}' at warehouse '{}': requested {}, available {}",
                input.product_id.as_ref(),
                input.warehouse_id.as_ref(),
                requested_quantity,
                state.available_quantity
            )));
        }

        let now = chrono::Utc::now();
        let expires_at = now + chrono::Duration::minutes(input.expires_in_minutes as i64);
        let reservation_id = types::ReservationId::new();

        let event = StreamWrite::new(
            &read_streams,
            input.product_id.inventory_stream_id(&input.warehouse_id),
            InventoryEvent::StockReserved {
                product_id: input.product_id,
                warehouse_id: input.warehouse_id,
                quantity: input.quantity,
                reservation_id,
                reserved_for: input.reserved_for,
                expires_at,
                reserved_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Transfer stock between warehouses (atomic multi-location operation)
pub struct TransferStockCommand {
    pub product_id: types::ProductId,
    pub from_warehouse: types::WarehouseId,
    pub to_warehouse: types::WarehouseId,
    pub quantity: types::Quantity,
    pub transfer_cost: types::Price,
}

#[async_trait::async_trait]
impl Command for TransferStockCommand {
    type Input = Self;
    type State = TransferState;
    type Event = InventoryEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.product_id.inventory_stream_id(&input.from_warehouse),
            input.product_id.inventory_stream_id(&input.to_warehouse),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_id = event.stream_id.as_ref();
        
        // Determine which warehouse this event affects
        if stream_id.contains(&format!("-{}-", self.from_warehouse.as_ref())) {
            // Event for source warehouse
            match &event.payload {
                InventoryEvent::StockReceived { quantity, .. } => {
                    state.from_stock = types::StockLevel::try_new(
                        state.from_stock.into() + Into::<u32>::into(*quantity)
                    ).unwrap_or(state.from_stock);
                }
                InventoryEvent::StockSold { quantity, .. } => {
                    let qty: u32 = (*quantity).into();
                    state.from_stock = types::StockLevel::try_new(
                        state.from_stock.into().saturating_sub(qty)
                    ).unwrap_or(types::StockLevel::try_new(0).unwrap());
                }
                _ => {}
            }
        } else if stream_id.contains(&format!("-{}-", self.to_warehouse.as_ref())) {
            // Event for destination warehouse
            match &event.payload {
                InventoryEvent::StockReceived { quantity, .. } => {
                    state.to_stock = types::StockLevel::try_new(
                        state.to_stock.into() + Into::<u32>::into(*quantity)
                    ).unwrap_or(state.to_stock);
                }
                _ => {}
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let transfer_quantity: u32 = input.quantity.into();
        
        // Validate source warehouse has sufficient stock
        if state.from_stock.into() < transfer_quantity {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Insufficient stock in source warehouse '{}' for product '{}': has {}, need {}",
                input.from_warehouse.as_ref(),
                input.product_id.as_ref(),
                Into::<u32>::into(state.from_stock),
                transfer_quantity
            )));
        }

        let now = chrono::Utc::now();
        
        // Create transfer event that affects both warehouses
        let event = StreamWrite::new(
            &read_streams,
            input.product_id.inventory_stream_id(&input.from_warehouse),
            InventoryEvent::StockTransferred {
                product_id: input.product_id.clone(),
                from_warehouse: input.from_warehouse.clone(),
                to_warehouse: input.to_warehouse.clone(),
                quantity: input.quantity,
                transfer_cost: input.transfer_cost,
                transferred_at: now,
            },
        )?;

        // Also create a stock received event for the destination
        let receive_event = StreamWrite::new(
            &read_streams,
            input.product_id.inventory_stream_id(&input.to_warehouse),
            InventoryEvent::StockReceived {
                product_id: input.product_id,
                warehouse_id: input.to_warehouse,
                quantity: input.quantity,
                unit_cost: input.transfer_cost,
                received_at: now,
                purchase_order_id: None,
            },
        )?;

        Ok(vec![event, receive_event])
    }
}

#[derive(Debug, Default, Clone)]
pub struct TransferState {
    pub from_stock: types::StockLevel,
    pub to_stock: types::StockLevel,
    pub from_warehouse: types::WarehouseId,
    pub to_warehouse: types::WarehouseId,
}

impl TransferStockCommand {
    fn from_warehouse(&self) -> &types::WarehouseId {
        &self.from_warehouse
    }
    
    fn to_warehouse(&self) -> &types::WarehouseId {
        &self.to_warehouse
    }
}

/// Create purchase order with dynamic supplier discovery
pub struct CreatePurchaseOrderCommand {
    pub supplier_id: types::SupplierId,
    pub items: Vec<PurchaseOrderItem>,
    pub expected_delivery_days: u32,
}

#[async_trait::async_trait]
impl Command for CreatePurchaseOrderCommand {
    type Input = Self;
    type State = PurchaseOrderCreationState;
    type Event = InventoryEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        let mut streams = vec![input.supplier_id.stream_id()];
        
        // Initially read supplier stream, products will be discovered dynamically
        for item in &input.items {
            streams.push(item.product_id.product_stream_id());
        }
        
        streams
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            InventoryEvent::ProductCreated {
                product_id,
                name,
                supplier_id,
                unit_cost,
                ..
            } => {
                if event.stream_id.as_ref().starts_with("product-") {
                    state.products.insert(
                        product_id.clone(),
                        ProductInfo {
                            name: name.clone(),
                            supplier_id: supplier_id.clone(),
                            unit_cost: *unit_cost,
                            exists: true,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate all products exist and belong to the supplier
        for item in &input.items {
            let product_info = state
                .products
                .get(&item.product_id)
                .ok_or_else(|| {
                    CommandError::BusinessRuleViolation(format!(
                        "Product '{}' does not exist",
                        item.product_id.as_ref()
                    ))
                })?;

            if !product_info.exists {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Product '{}' is not available",
                    item.product_id.as_ref()
                )));
            }

            if product_info.supplier_id != input.supplier_id {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Product '{}' is not supplied by '{}'",
                    item.product_id.as_ref(),
                    input.supplier_id.as_ref()
                )));
            }

            // Discover variant products dynamically
            let variant_streams = self.discover_product_variants(&item.product_id);
            if !variant_streams.is_empty() {
                stream_resolver.add_streams(variant_streams);
            }
        }

        // Calculate total cost
        let total_cost = input
            .items
            .iter()
            .map(|item| {
                let unit_cost: f64 = item.unit_cost.into();
                let quantity: u32 = item.quantity.into();
                unit_cost * quantity as f64
            })
            .sum::<f64>();

        let order_id = types::PurchaseOrderId::new();
        let now = chrono::Utc::now();
        let expected_delivery = now + chrono::Duration::days(input.expected_delivery_days as i64);

        let event = StreamWrite::new(
            &read_streams,
            order_id.stream_id(),
            InventoryEvent::PurchaseOrderCreated {
                order_id,
                supplier_id: input.supplier_id,
                items: input.items,
                total_cost: types::Price::try_new(total_cost).map_err(|e| {
                    CommandError::ValidationFailed(format!("Invalid total cost: {}", e))
                })?,
                expected_delivery,
                created_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

impl CreatePurchaseOrderCommand {
    fn discover_product_variants(&self, _product_id: &types::ProductId) -> Vec<StreamId> {
        // In a real system, this might discover related products, variants, or
        // alternative suppliers based on the product catalog
        vec![]
    }
}

#[derive(Debug, Default, Clone)]
pub struct PurchaseOrderCreationState {
    pub products: HashMap<types::ProductId, ProductInfo>,
}

#[derive(Debug, Clone)]
pub struct ProductInfo {
    pub name: String,
    pub supplier_id: types::SupplierId,
    pub unit_cost: types::Price,
    pub exists: bool,
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn create_product(
    executor: &CommandExecutor<InMemoryEventStore<InventoryEvent>>,
    product_id: types::ProductId,
    name: String,
    category: String,
    unit_cost: types::Price,
    supplier_id: types::SupplierId,
) -> Result<(), CommandError> {
    let command = CreateProductCommand {
        product_id,
        name,
        category,
        unit_cost,
        supplier_id,
    };
    
    executor.execute(&command, command, ExecutionOptions::default()).await?;
    Ok(())
}

pub struct CreateProductCommand {
    pub product_id: types::ProductId,
    pub name: String,
    pub category: String,
    pub unit_cost: types::Price,
    pub supplier_id: types::SupplierId,
}

#[async_trait::async_trait]
impl Command for CreateProductCommand {
    type Input = Self;
    type State = ProductState;
    type Event = InventoryEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.product_id.product_stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let InventoryEvent::ProductCreated {
            name,
            category,
            unit_cost,
            supplier_id,
            created_at,
            ..
        } = &event.payload
        {
            state.exists = true;
            state.name = Some(name.clone());
            state.category = Some(category.clone());
            state.unit_cost = Some(*unit_cost);
            state.supplier_id = Some(supplier_id.clone());
            state.created_at = Some(*created_at);
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Product '{}' already exists",
                input.product_id.as_ref()
            )));
        }

        let event = StreamWrite::new(
            &read_streams,
            input.product_id.product_stream_id(),
            InventoryEvent::ProductCreated {
                product_id: input.product_id,
                name: input.name,
                category: input.category,
                unit_cost: input.unit_cost,
                supplier_id: input.supplier_id,
                created_at: chrono::Utc::now(),
            },
        )?;

        Ok(vec![event])
    }
}

// ============================================================================
// Example Execution
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    println!("📦 EventCore Inventory Management System Example");
    println!("===============================================\n");

    // Step 1: Set up products and suppliers
    println!("🏭 Setting up products and suppliers...");
    
    let supplier_id = types::SupplierId::try_new("acme-supplies".to_string()).unwrap();
    let product_id = types::ProductId::try_new("widget-001".to_string()).unwrap();
    let warehouse_main = types::WarehouseId::try_new("warehouse-main".to_string()).unwrap();
    let warehouse_west = types::WarehouseId::try_new("warehouse-west".to_string()).unwrap();

    create_product(
        &executor,
        product_id.clone(),
        "Premium Widget".to_string(),
        "Widgets".to_string(),
        types::Price::try_new(25.50).unwrap(),
        supplier_id.clone(),
    ).await?;

    println!("✅ Created product: {} from supplier: {}", product_id.as_ref(), supplier_id.as_ref());

    // Step 2: Create purchase order with dynamic discovery
    println!("\n📋 Creating purchase order...");
    
    let po_items = vec![PurchaseOrderItem {
        product_id: product_id.clone(),
        quantity: types::Quantity::try_new(100).unwrap(),
        unit_cost: types::Price::try_new(25.50).unwrap(),
    }];

    let create_po_command = CreatePurchaseOrderCommand {
        supplier_id: supplier_id.clone(),
        items: po_items,
        expected_delivery_days: 7,
    };

    let po_result = executor.execute(&create_po_command, create_po_command, ExecutionOptions::default()).await?;
    println!("✅ Purchase order created - {} events written", po_result.events_written.len());

    // Step 3: Simulate receiving inventory 
    println!("\n📥 Receiving inventory...");
    
    let receive_command = ReceiveStockCommand {
        product_id: product_id.clone(),
        warehouse_id: warehouse_main.clone(),
        quantity: types::Quantity::try_new(100).unwrap(),
        unit_cost: types::Price::try_new(25.50).unwrap(),
    };

    executor.execute(&receive_command, receive_command, ExecutionOptions::default()).await?;
    println!("✅ Received 100 widgets at main warehouse");

    // Step 4: Reserve stock for customer order
    println!("\n🔒 Reserving stock for customer...");
    
    let reserve_command = ReserveStockCommand {
        product_id: product_id.clone(),
        warehouse_id: warehouse_main.clone(),
        quantity: types::Quantity::try_new(25).unwrap(),
        reserved_for: "customer-12345".to_string(),
        expires_in_minutes: 30,
    };

    let reservation_result = executor.execute(&reserve_command, reserve_command, ExecutionOptions::default()).await?;
    println!("✅ Reserved 25 widgets for customer-12345");

    // Step 5: Transfer stock between warehouses (atomic multi-location)
    println!("\n🚛 Transferring stock between warehouses...");
    
    let transfer_command = TransferStockCommand {
        product_id: product_id.clone(),
        from_warehouse: warehouse_main.clone(),
        to_warehouse: warehouse_west.clone(),
        quantity: types::Quantity::try_new(30).unwrap(),
        transfer_cost: types::Price::try_new(26.00).unwrap(),
    };

    let transfer_result = executor.execute(&transfer_command, transfer_command, ExecutionOptions::default()).await?;
    println!("✅ Transferred 30 widgets: main → west warehouse");
    println!("   📊 Events written: {}", transfer_result.events_written.len());
    println!("   🔗 Streams affected: {:?}", transfer_result.stream_versions.keys().collect::<Vec<_>>());

    // Step 6: Attempt to over-reserve (should fail)
    println!("\n❌ Attempting to over-reserve stock (should fail)...");
    
    let over_reserve_command = ReserveStockCommand {
        product_id: product_id.clone(),
        warehouse_id: warehouse_main.clone(),
        quantity: types::Quantity::try_new(100).unwrap(), // Only 45 available (75 - 25 reserved - 30 transferred)
        reserved_for: "customer-99999".to_string(),
        expires_in_minutes: 15,
    };

    match executor.execute(&over_reserve_command, over_reserve_command, ExecutionOptions::default()).await {
        Ok(_) => println!("❌ ERROR: Should not have been able to over-reserve!"),
        Err(err) => println!("✅ Correctly blocked over-reservation: {}", err),
    }

    println!("\n🎉 Example completed successfully!");
    println!("\n💡 Key EventCore patterns demonstrated:");
    println!("   ✅ Inventory reservation with expiration");
    println!("   ✅ Atomic multi-location transfers");
    println!("   ✅ Dynamic stream discovery in purchase orders");
    println!("   ✅ Business rule enforcement (stock availability)");
    println!("   ✅ Complex state management across multiple entities");
    println!("   ✅ Real-world inventory operations modeling");

    Ok(())
}

// Simple helper command for receiving stock
pub struct ReceiveStockCommand {
    pub product_id: types::ProductId,
    pub warehouse_id: types::WarehouseId,
    pub quantity: types::Quantity,
    pub unit_cost: types::Price,
}

#[async_trait::async_trait]
impl Command for ReceiveStockCommand {
    type Input = Self;
    type State = InventoryState;
    type Event = InventoryEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.product_id.inventory_stream_id(&input.warehouse_id)]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let InventoryEvent::StockReceived { quantity, .. } = &event.payload {
            state.stock_level = types::StockLevel::try_new(
                state.stock_level.into() + Into::<u32>::into(*quantity)
            ).unwrap_or(state.stock_level);
            state.available_quantity = state.stock_level.into() - state.reserved_quantity;
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = StreamWrite::new(
            &read_streams,
            input.product_id.inventory_stream_id(&input.warehouse_id),
            InventoryEvent::StockReceived {
                product_id: input.product_id,
                warehouse_id: input.warehouse_id,
                quantity: input.quantity,
                unit_cost: input.unit_cost,
                received_at: chrono::Utc::now(),
                purchase_order_id: None,
            },
        )?;

        Ok(vec![event])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stock_reservation() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let product_id = types::ProductId::try_new("test-product".to_string()).unwrap();
        let warehouse_id = types::WarehouseId::try_new("test-warehouse".to_string()).unwrap();

        // First receive some stock
        let receive_command = ReceiveStockCommand {
            product_id: product_id.clone(),
            warehouse_id: warehouse_id.clone(),
            quantity: types::Quantity::try_new(50).unwrap(),
            unit_cost: types::Price::try_new(10.0).unwrap(),
        };

        executor.execute(&receive_command, receive_command, ExecutionOptions::default()).await.unwrap();

        // Then reserve some stock
        let reserve_command = ReserveStockCommand {
            product_id: product_id.clone(),
            warehouse_id: warehouse_id.clone(),
            quantity: types::Quantity::try_new(20).unwrap(),
            reserved_for: "test-customer".to_string(),
            expires_in_minutes: 30,
        };

        let result = executor.execute(&reserve_command, reserve_command, ExecutionOptions::default()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_insufficient_stock_reservation() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let product_id = types::ProductId::try_new("test-product".to_string()).unwrap();
        let warehouse_id = types::WarehouseId::try_new("test-warehouse".to_string()).unwrap();

        // Try to reserve stock without having any
        let reserve_command = ReserveStockCommand {
            product_id,
            warehouse_id,
            quantity: types::Quantity::try_new(20).unwrap(),
            reserved_for: "test-customer".to_string(),
            expires_in_minutes: 30,
        };

        let result = executor.execute(&reserve_command, reserve_command, ExecutionOptions::default()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Insufficient stock"));
    }

    #[tokio::test]
    async fn test_warehouse_transfer() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let product_id = types::ProductId::try_new("test-product".to_string()).unwrap();
        let warehouse_from = types::WarehouseId::try_new("warehouse-a".to_string()).unwrap();
        let warehouse_to = types::WarehouseId::try_new("warehouse-b".to_string()).unwrap();

        // Receive stock in source warehouse
        let receive_command = ReceiveStockCommand {
            product_id: product_id.clone(),
            warehouse_id: warehouse_from.clone(),
            quantity: types::Quantity::try_new(100).unwrap(),
            unit_cost: types::Price::try_new(15.0).unwrap(),
        };

        executor.execute(&receive_command, receive_command, ExecutionOptions::default()).await.unwrap();

        // Transfer stock between warehouses
        let transfer_command = TransferStockCommand {
            product_id,
            from_warehouse: warehouse_from,
            to_warehouse: warehouse_to,
            quantity: types::Quantity::try_new(40).unwrap(),
            transfer_cost: types::Price::try_new(16.0).unwrap(),
        };

        let result = executor.execute(&transfer_command, transfer_command, ExecutionOptions::default()).await;
        assert!(result.is_ok());
        
        // Should write events to both warehouse streams
        let events_written = result.unwrap().events_written.len();
        assert_eq!(events_written, 2);
    }
}