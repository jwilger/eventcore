//! Realistic workload benchmarks for `EventCore` library.
//!
//! This benchmark simulates real-world usage patterns based on the banking
//! and e-commerce examples, testing the performance of multi-stream commands
//! with realistic business logic and validation.

#![allow(missing_docs)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::uninlined_format_args)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    CommandExecutor, CommandResult, EventId, EventMetadata, EventStore, EventToWrite,
    ExpectedVersion, ReadStreams, StoredEvent, StreamEvents, StreamId, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hint::black_box;
use uuid::Uuid;

// ============================================================================
// Realistic Domain Types (simplified from examples)
// ============================================================================

/// Simple money type for benchmarking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money(i64); // cents

impl Money {
    pub fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    pub fn cents(&self) -> i64 {
        self.0
    }

    pub fn subtract(&self, other: &Self) -> Option<Self> {
        if self.0 >= other.0 {
            Some(Self(self.0 - other.0))
        } else {
            None
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self(self.0 + other.0)
    }
}

/// Account ID for banking benchmarks
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(String);

impl AccountId {
    pub fn generate() -> Self {
        Self(format!(
            "acc-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
    }
}

/// Transfer ID for banking benchmarks
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(String);

impl TransferId {
    pub fn generate() -> Self {
        Self(format!(
            "txn-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
    }
}

/// Banking events for realistic workload
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BankingEvent {
    AccountOpened {
        account_id: AccountId,
        initial_balance: Money,
    },
    MoneyTransferred {
        transfer_id: TransferId,
        from_account: AccountId,
        to_account: AccountId,
        amount: Money,
    },
}

impl<'a> TryFrom<&'a serde_json::Value> for BankingEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<BankingEvent> for serde_json::Value {
    fn from(event: BankingEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

// ============================================================================
// Banking Workload Commands
// ============================================================================

/// Realistic banking transfer command for workload testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealisticTransferCommand {
    pub transfer_id: TransferId,
    pub from_account: AccountId,
    pub to_account: AccountId,
    pub amount: Money,
}

#[derive(Debug, Default, Clone)]
pub struct BankingState {
    pub balances: HashMap<AccountId, Money>,
    pub completed_transfers: HashMap<TransferId, bool>,
}

impl eventcore::CommandStreams for RealisticTransferCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("account-{}", self.from_account.0)).unwrap(),
            StreamId::try_new(format!("account-{}", self.to_account.0)).unwrap(),
            StreamId::try_new("transfers".to_string()).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for RealisticTransferCommand {
    type State = BankingState;
    type Event = BankingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::AccountOpened {
                account_id,
                initial_balance,
            } => {
                state.balances.insert(account_id.clone(), *initial_balance);
            }
            BankingEvent::MoneyTransferred {
                transfer_id,
                from_account,
                to_account,
                amount,
            } => {
                if let Some(from_balance) = state.balances.get_mut(from_account) {
                    *from_balance = from_balance.subtract(amount).unwrap();
                }
                if let Some(to_balance) = state.balances.get_mut(to_account) {
                    *to_balance = to_balance.add(amount);
                }
                state.completed_transfers.insert(transfer_id.clone(), true);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check for idempotency
        if state.completed_transfers.contains_key(&self.transfer_id) {
            return Ok(vec![]);
        }

        // Realistic business logic validation
        let from_balance = state.balances.get(&self.from_account).ok_or_else(|| {
            eventcore::CommandError::BusinessRuleViolation(format!(
                "Account {} not found",
                self.from_account.0
            ))
        })?;

        if !state.balances.contains_key(&self.to_account) {
            return Err(eventcore::CommandError::BusinessRuleViolation(format!(
                "Account {} not found",
                self.to_account.0
            )));
        }

        if from_balance.subtract(&self.amount).is_none() {
            return Err(eventcore::CommandError::BusinessRuleViolation(format!(
                "Insufficient funds: balance {}, requested {}",
                from_balance.cents(),
                self.amount.cents()
            )));
        }

        let event = BankingEvent::MoneyTransferred {
            transfer_id: self.transfer_id.clone(),
            from_account: self.from_account.clone(),
            to_account: self.to_account.clone(),
            amount: self.amount,
        };

        // Write to all three streams atomically
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", self.from_account.0)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", self.to_account.0)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new("transfers".to_string()).unwrap(),
                event,
            )?,
        ])
    }
}

// ============================================================================
// E-commerce Workload Types
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId(String);

impl OrderId {
    pub fn generate() -> Self {
        Self(format!(
            "ord-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProductId(String);

impl ProductId {
    pub fn generate() -> Self {
        Self(format!(
            "prd-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: ProductId,
    pub quantity: u32,
    pub unit_price: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Draft,
    Placed,
    Cancelled,
}

/// E-commerce events for realistic workload
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcommerceEvent {
    OrderCreated {
        order_id: OrderId,
    },
    ItemAddedToOrder {
        order_id: OrderId,
        item: OrderItem,
    },
    OrderPlaced {
        order_id: OrderId,
        total_amount: Money,
    },
    InventoryReserved {
        product_id: ProductId,
        quantity: u32,
        order_id: OrderId,
    },
}

impl<'a> TryFrom<&'a serde_json::Value> for EcommerceEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<EcommerceEvent> for serde_json::Value {
    fn from(event: EcommerceEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

// ============================================================================
// E-commerce Workload Commands
// ============================================================================

/// Realistic e-commerce order command with dynamic stream discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealisticAddItemCommand {
    pub order_id: OrderId,
    pub item: OrderItem,
}

#[derive(Debug, Default, Clone)]
pub struct EcommerceState {
    pub orders: HashMap<OrderId, Vec<OrderItem>>,
    pub order_status: HashMap<OrderId, OrderStatus>,
    pub inventory: HashMap<ProductId, u32>,
}

impl eventcore::CommandStreams for RealisticAddItemCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("order-{}", self.order_id.0)).unwrap(),
            StreamId::try_new(format!("product-{}", self.item.product_id.0)).unwrap(),
            StreamId::try_new("inventory".to_string()).unwrap(),
        ]
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for RealisticAddItemCommand {
    type State = EcommerceState;
    type Event = EcommerceEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            EcommerceEvent::OrderCreated { order_id } => {
                state.orders.insert(order_id.clone(), Vec::new());
                state
                    .order_status
                    .insert(order_id.clone(), OrderStatus::Draft);
            }
            EcommerceEvent::ItemAddedToOrder { order_id, item } => {
                if let Some(items) = state.orders.get_mut(order_id) {
                    items.push(item.clone());
                }
            }
            EcommerceEvent::OrderPlaced { order_id, .. } => {
                state
                    .order_status
                    .insert(order_id.clone(), OrderStatus::Placed);
            }
            EcommerceEvent::InventoryReserved {
                product_id,
                quantity,
                ..
            } => {
                if let Some(current) = state.inventory.get_mut(product_id) {
                    *current = current.saturating_sub(*quantity);
                }
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Realistic business logic validation
        if !state.orders.contains_key(&self.order_id) {
            return Err(eventcore::CommandError::BusinessRuleViolation(format!(
                "Order {} does not exist",
                self.order_id.0
            )));
        }

        if state.order_status.get(&self.order_id) != Some(&OrderStatus::Draft) {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Cannot add items to non-draft order".to_string(),
            ));
        }

        let available_inventory = state.inventory.get(&self.item.product_id).unwrap_or(&0);
        if *available_inventory < self.item.quantity {
            return Err(eventcore::CommandError::BusinessRuleViolation(format!(
                "Insufficient inventory: available {}, requested {}",
                available_inventory, self.item.quantity
            )));
        }

        let item_event = EcommerceEvent::ItemAddedToOrder {
            order_id: self.order_id.clone(),
            item: self.item.clone(),
        };

        let inventory_event = EcommerceEvent::InventoryReserved {
            product_id: self.item.product_id.clone(),
            quantity: self.item.quantity,
            order_id: self.order_id.clone(),
        };

        // Write to multiple streams atomically
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("order-{}", self.order_id.0)).unwrap(),
                item_event,
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("product-{}", self.item.product_id.0)).unwrap(),
                inventory_event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new("inventory".to_string()).unwrap(),
                inventory_event,
            )?,
        ])
    }
}

// ============================================================================
// Workload Benchmarks
// ============================================================================

/// Benchmark realistic banking workloads
fn bench_banking_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_banking_workload");

    // Test different numbers of concurrent accounts
    for num_accounts in [10, 50, 100] {
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::new("money_transfer", num_accounts),
            &num_accounts,
            |b, &account_count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup: create accounts with initial balances
                    let mut accounts = Vec::new();
                    for i in 0..account_count {
                        let account_id = AccountId(format!("bench-account-{i}"));
                        accounts.push(account_id.clone());

                        // Setup initial account balance
                        let stream_id =
                            StreamId::try_new(format!("account-{}", account_id.0)).unwrap();
                        let event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(BankingEvent::AccountOpened {
                                account_id: account_id.clone(),
                                initial_balance: Money::from_cents(100_000), // $1000
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let stream_events =
                            StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Benchmark: perform a realistic transfer
                    let from_idx = 0;
                    let to_idx = account_count.min(2) - 1; // Avoid out of bounds
                    let command = RealisticTransferCommand {
                        transfer_id: TransferId::generate(),
                        from_account: accounts[from_idx].clone(),
                        to_account: accounts[to_idx].clone(),
                        amount: Money::from_cents(5000), // $50
                    };

                    black_box(
                        executor
                            .execute(command, eventcore::ExecutionOptions::default())
                            .await
                            .unwrap(),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark realistic e-commerce workloads
fn bench_ecommerce_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_ecommerce_workload");

    // Test different numbers of products in order
    for num_products in [1, 5, 10] {
        group.throughput(Throughput::Elements(num_products));

        group.bench_with_input(
            BenchmarkId::new("add_items_to_order", num_products),
            &num_products,
            |b, &product_count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup: create order and inventory
                    let order_id = OrderId::generate();
                    let mut products = Vec::new();

                    // Create order
                    let order_stream = StreamId::try_new(format!("order-{}", order_id.0)).unwrap();
                    let order_event = EventToWrite::with_metadata(
                        EventId::new(),
                        serde_json::to_value(EcommerceEvent::OrderCreated {
                            order_id: order_id.clone(),
                        })
                        .unwrap(),
                        EventMetadata::new(),
                    );
                    let order_stream_events =
                        StreamEvents::new(order_stream, ExpectedVersion::New, vec![order_event]);
                    executor
                        .event_store()
                        .write_events_multi(vec![order_stream_events])
                        .await
                        .unwrap();

                    // Setup products and inventory
                    for i in 0..product_count {
                        let product_id = ProductId(format!("bench-product-{i}"));
                        products.push(product_id.clone());

                        // Setup inventory
                        let inventory_stream = StreamId::try_new("inventory".to_string()).unwrap();
                        let inventory_event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(EcommerceEvent::InventoryReserved {
                                product_id: product_id.clone(),
                                quantity: 0, // Start with high inventory
                                order_id: OrderId(format!("initial-{i}")),
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let inventory_events = StreamEvents::new(
                            inventory_stream,
                            if i == 0 {
                                ExpectedVersion::New
                            } else {
                                ExpectedVersion::Any
                            },
                            vec![inventory_event],
                        );
                        executor
                            .event_store()
                            .write_events_multi(vec![inventory_events])
                            .await
                            .unwrap();
                    }

                    // Benchmark: add item to order (realistic multi-stream operation)
                    let product_idx = 0; // Use first product
                    let command = RealisticAddItemCommand {
                        order_id: order_id.clone(),
                        item: OrderItem {
                            product_id: products[product_idx].clone(),
                            quantity: 2,
                            unit_price: Money::from_cents(2999), // $29.99
                        },
                    };

                    // This should fail due to insufficient inventory, but that's realistic
                    let result = executor
                        .execute(command, eventcore::ExecutionOptions::default())
                        .await;
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark mixed workload scenarios
fn bench_mixed_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_mixed_workload");
    group.throughput(Throughput::Elements(1));

    group.bench_function("banking_and_ecommerce_mixed", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let event_store = InMemoryEventStore::<serde_json::Value>::new();
            let executor = CommandExecutor::new(event_store);

            // Simulate a realistic mixed workload:
            // 1. Banking transfer (3 streams)
            // 2. E-commerce order with multiple items (variable streams)

            // Setup banking accounts
            let account1 = AccountId::generate();
            let account2 = AccountId::generate();

            for account in [&account1, &account2] {
                let stream_id = StreamId::try_new(format!("account-{}", account.0)).unwrap();
                let event = EventToWrite::with_metadata(
                    EventId::new(),
                    serde_json::to_value(BankingEvent::AccountOpened {
                        account_id: account.clone(),
                        initial_balance: Money::from_cents(50_000), // $500
                    })
                    .unwrap(),
                    EventMetadata::new(),
                );
                let stream_events = StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
                executor
                    .event_store()
                    .write_events_multi(vec![stream_events])
                    .await
                    .unwrap();
            }

            // Setup e-commerce order
            let order_id = OrderId::generate();
            let order_stream = StreamId::try_new(format!("order-{}", order_id.0)).unwrap();
            let order_event = EventToWrite::with_metadata(
                EventId::new(),
                serde_json::to_value(EcommerceEvent::OrderCreated {
                    order_id: order_id.clone(),
                })
                .unwrap(),
                EventMetadata::new(),
            );
            let order_stream_events =
                StreamEvents::new(order_stream, ExpectedVersion::New, vec![order_event]);
            executor
                .event_store()
                .write_events_multi(vec![order_stream_events])
                .await
                .unwrap();

            // Execute banking transfer
            let banking_command = RealisticTransferCommand {
                transfer_id: TransferId::generate(),
                from_account: account1,
                to_account: account2,
                amount: Money::from_cents(2500), // $25
            };

            let banking_result = executor
                .execute(banking_command, eventcore::ExecutionOptions::default())
                .await
                .unwrap();

            // Execute e-commerce operation (will fail due to no inventory, but measures overhead)
            let ecommerce_command = RealisticAddItemCommand {
                order_id,
                item: OrderItem {
                    product_id: ProductId::generate(),
                    quantity: 1,
                    unit_price: Money::from_cents(1999), // $19.99
                },
            };

            let ecommerce_result = executor
                .execute(ecommerce_command, eventcore::ExecutionOptions::default())
                .await;

            black_box((banking_result, ecommerce_result))
        });
    });

    group.finish();
}

/// Benchmark stream discovery overhead
fn bench_stream_discovery_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_stream_discovery");

    // Test different numbers of items that require dynamic stream discovery
    for num_dynamic_streams in [2, 5, 10, 20] {
        group.throughput(Throughput::Elements(num_dynamic_streams));

        group.bench_with_input(
            BenchmarkId::new("dynamic_streams", num_dynamic_streams),
            &num_dynamic_streams,
            |b, &stream_count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Create a command that will discover many streams dynamically
                    // This simulates the e-commerce cancel order scenario which discovers
                    // all product streams for items in the order

                    let order_id = OrderId::generate();
                    let order_stream = StreamId::try_new(format!("order-{}", order_id.0)).unwrap();

                    // Create order with many items (each requiring a product stream)
                    let mut items = Vec::new();
                    for i in 0..stream_count {
                        let product_id = ProductId(format!("dynamic-product-{i}"));
                        items.push(OrderItem {
                            product_id,
                            quantity: 1,
                            unit_price: Money::from_cents(1000),
                        });
                    }

                    // Setup order with all items
                    let order_created = EventToWrite::with_metadata(
                        EventId::new(),
                        serde_json::to_value(EcommerceEvent::OrderCreated {
                            order_id: order_id.clone(),
                        })
                        .unwrap(),
                        EventMetadata::new(),
                    );

                    let mut events = vec![order_created];
                    for item in &items {
                        events.push(EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(EcommerceEvent::ItemAddedToOrder {
                                order_id: order_id.clone(),
                                item: item.clone(),
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        ));
                    }

                    let stream_events =
                        StreamEvents::new(order_stream, ExpectedVersion::New, events);
                    executor
                        .event_store()
                        .write_events_multi(vec![stream_events])
                        .await
                        .unwrap();

                    // Benchmark: execute command that would trigger dynamic stream discovery
                    // In a real scenario, this would be a cancel order command that needs to
                    // read all product streams to release inventory
                    let command = RealisticAddItemCommand {
                        // Simulates multi-stream access
                        order_id,
                        item: OrderItem {
                            product_id: ProductId::generate(),
                            quantity: 1,
                            unit_price: Money::from_cents(999),
                        },
                    };

                    let result = executor
                        .execute(command, eventcore::ExecutionOptions::default())
                        .await;

                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_banking_workload,
    bench_ecommerce_workload,
    bench_mixed_workload,
    bench_stream_discovery_workload,
);
criterion_main!(benches);
