//! Performance target validation tests for `EventCore`
//!
//! This test suite validates `EventCore`'s performance against the original PRD targets:
//! - Single-stream commands: 5,000-10,000 ops/sec
//! - Multi-stream commands: 2,000-5,000 ops/sec  
//! - Event store writes: 20,000+ events/sec (batched)
//! - P95 command latency: < 10ms

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::if_not_else)]
#![allow(clippy::use_self)]
#![allow(dead_code)]

use eventcore::{
    CommandError, CommandExecutor, CommandResult, EventId, EventStore, EventToWrite,
    ExecutionOptions, ExpectedVersion, ReadStreams, StoredEvent, StreamEvents, StreamId,
    StreamResolver, StreamWrite,
};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Timestamp;

// PRD Performance Targets
const TARGET_SINGLE_STREAM_OPS_MIN: f64 = 5_000.0;
const TARGET_SINGLE_STREAM_OPS_MAX: f64 = 10_000.0;
const TARGET_MULTI_STREAM_OPS_MIN: f64 = 2_000.0;
const TARGET_MULTI_STREAM_OPS_MAX: f64 = 5_000.0;
const TARGET_BATCH_EVENTS_PER_SEC: f64 = 20_000.0;
const TARGET_P95_LATENCY_MS: f64 = 10.0;

#[derive(Debug, Clone)]
struct PerformanceMetrics {
    total_operations: usize,
    successful_operations: usize,
    failed_operations: usize,
    total_duration: Duration,
    operations_per_second: f64,
    latencies_ms: Vec<f64>,
    p50_latency_ms: f64,
    p75_latency_ms: f64,
    p90_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
    min_latency_ms: f64,
    max_latency_ms: f64,
    avg_latency_ms: f64,
}

impl PerformanceMetrics {
    fn calculate(operations: usize, duration: Duration, mut latencies_ms: Vec<f64>) -> Self {
        latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let successful = latencies_ms.len();
        let failed = operations.saturating_sub(successful);
        let ops_per_second = successful as f64 / duration.as_secs_f64();

        let p50 = Self::percentile(&latencies_ms, 0.50);
        let p75 = Self::percentile(&latencies_ms, 0.75);
        let p90 = Self::percentile(&latencies_ms, 0.90);
        let p95 = Self::percentile(&latencies_ms, 0.95);
        let p99 = Self::percentile(&latencies_ms, 0.99);

        let min = latencies_ms.first().copied().unwrap_or(0.0);
        let max = latencies_ms.last().copied().unwrap_or(0.0);
        let avg = if !latencies_ms.is_empty() {
            latencies_ms.iter().sum::<f64>() / latencies_ms.len() as f64
        } else {
            0.0
        };

        Self {
            total_operations: operations,
            successful_operations: successful,
            failed_operations: failed,
            total_duration: duration,
            operations_per_second: ops_per_second,
            latencies_ms,
            p50_latency_ms: p50,
            p75_latency_ms: p75,
            p90_latency_ms: p90,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            min_latency_ms: min,
            max_latency_ms: max,
            avg_latency_ms: avg,
        }
    }

    fn percentile(sorted_data: &[f64], percentile: f64) -> f64 {
        if sorted_data.is_empty() {
            return 0.0;
        }
        let index = ((sorted_data.len() - 1) as f64 * percentile) as usize;
        sorted_data[index]
    }
}

#[derive(Debug)]
struct PerformanceReport {
    test_name: String,
    scenario: String,
    metrics: PerformanceMetrics,
    target_met: bool,
    target_details: Vec<TargetValidation>,
}

#[derive(Debug)]
struct TargetValidation {
    metric: String,
    actual: f64,
    target_min: Option<f64>,
    target_max: Option<f64>,
    passed: bool,
    reason: String,
}

impl PerformanceReport {
    fn display(&self) {
        println!("\n{}", "=".repeat(100));
        println!("Performance Test: {}", self.test_name);
        println!("Scenario: {}", self.scenario);
        println!("{}", "-".repeat(100));

        // Summary statistics
        println!("Total Operations:     {}", self.metrics.total_operations);
        println!(
            "Successful:           {} ({:.1}%)",
            self.metrics.successful_operations,
            (self.metrics.successful_operations as f64 / self.metrics.total_operations as f64)
                * 100.0
        );
        println!("Failed:               {}", self.metrics.failed_operations);
        println!(
            "Duration:             {:.2}s",
            self.metrics.total_duration.as_secs_f64()
        );
        println!(
            "Throughput:           {:.2} ops/sec",
            self.metrics.operations_per_second
        );

        println!("\nLatency Distribution:");
        println!("  Min:                {:.2}ms", self.metrics.min_latency_ms);
        println!("  P50:                {:.2}ms", self.metrics.p50_latency_ms);
        println!("  P75:                {:.2}ms", self.metrics.p75_latency_ms);
        println!("  P90:                {:.2}ms", self.metrics.p90_latency_ms);
        println!("  P95:                {:.2}ms", self.metrics.p95_latency_ms);
        println!("  P99:                {:.2}ms", self.metrics.p99_latency_ms);
        println!("  Max:                {:.2}ms", self.metrics.max_latency_ms);
        println!("  Average:            {:.2}ms", self.metrics.avg_latency_ms);

        println!("\nTarget Validation:");
        for validation in &self.target_details {
            let status = if validation.passed {
                "✓ PASS"
            } else {
                "✗ FAIL"
            };
            print!("  {} {} - ", status, validation.metric);

            if let (Some(min), Some(max)) = (validation.target_min, validation.target_max) {
                print!(
                    "Actual: {:.2}, Target: {:.0}-{:.0}",
                    validation.actual, min, max
                );
            } else if let Some(min) = validation.target_min {
                print!("Actual: {:.2}, Target: ≥{:.0}", validation.actual, min);
            } else if let Some(max) = validation.target_max {
                print!("Actual: {:.2}, Target: ≤{:.0}", validation.actual, max);
            }

            if !validation.passed {
                print!(" ({})", validation.reason);
            }
            println!();
        }

        println!(
            "\nOverall Result: {}",
            if self.target_met {
                "✓ ALL TARGETS MET"
            } else {
                "✗ TARGETS NOT MET"
            }
        );
        println!("{}", "=".repeat(100));
    }
}

// Realistic event types based on common event sourcing patterns
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum RealisticEvent {
    // Account/User events
    AccountCreated {
        id: String,
        email: String,
    },
    AccountUpdated {
        id: String,
        field: String,
        value: String,
    },
    AccountClosed {
        id: String,
        reason: String,
    },

    // Financial events
    TransactionInitiated {
        id: String,
        from: String,
        to: String,
        amount: i64,
    },
    TransactionCompleted {
        id: String,
    },
    TransactionFailed {
        id: String,
        reason: String,
    },

    // Order/Shopping events
    OrderPlaced {
        id: String,
        customer: String,
        items: Vec<String>,
    },
    OrderShipped {
        id: String,
        tracking: String,
    },
    OrderCancelled {
        id: String,
        reason: String,
    },

    // Inventory events
    StockAdded {
        product: String,
        quantity: u32,
    },
    StockReserved {
        product: String,
        quantity: u32,
        order: String,
    },
    StockReleased {
        product: String,
        quantity: u32,
    },
}

impl TryFrom<&RealisticEvent> for RealisticEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &RealisticEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// Single-stream command for financial transactions
#[derive(Debug, Clone)]
struct FinancialTransactionCommand {
    account_id: String,
    transaction_id: String,
    amount: i64,
    transaction_type: TransactionType,
}

#[derive(Debug, Clone)]
enum TransactionType {
    Deposit,
    Withdrawal,
    Transfer { to_account: String },
}

#[derive(Debug, Default)]
struct AccountState {
    balance: i64,
    is_active: bool,
    pending_transactions: HashMap<String, i64>,
}

impl eventcore::CommandStreams for FinancialTransactionCommand {
    type StreamSet = (StreamId,);

    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("account-{}", self.account_id)).unwrap()]
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for FinancialTransactionCommand {
    type State = AccountState;
    type Event = RealisticEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            RealisticEvent::AccountCreated { .. } => {
                state.is_active = true;
                state.balance = 0;
            }
            RealisticEvent::TransactionInitiated { id, amount, .. } => {
                state.pending_transactions.insert(id.clone(), *amount);
            }
            RealisticEvent::TransactionCompleted { id } => {
                if let Some(amount) = state.pending_transactions.remove(id) {
                    state.balance += amount;
                }
            }
            RealisticEvent::AccountClosed { .. } => {
                state.is_active = false;
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
        // Validate business rules
        if !state.is_active {
            return Err(CommandError::BusinessRuleViolation(
                "Account is not active".to_string(),
            ));
        }

        match &self.transaction_type {
            TransactionType::Withdrawal | TransactionType::Transfer { .. } => {
                let pending_total: i64 = state.pending_transactions.values().sum();
                let available_balance = state.balance + pending_total;

                if available_balance < self.amount {
                    return Err(CommandError::BusinessRuleViolation(format!(
                        "Insufficient funds: available {}, requested {}",
                        available_balance, self.amount
                    )));
                }
            }
            TransactionType::Deposit => {}
        }

        // Create transaction events
        let mut events = vec![];
        let stream_id = StreamId::try_new(format!("account-{}", self.account_id)).unwrap();

        let (from, to, amount) = match &self.transaction_type {
            TransactionType::Deposit => {
                ("external".to_string(), self.account_id.clone(), self.amount)
            }
            TransactionType::Withdrawal => (
                self.account_id.clone(),
                "external".to_string(),
                -self.amount,
            ),
            TransactionType::Transfer { to_account } => {
                (self.account_id.clone(), to_account.clone(), -self.amount)
            }
        };

        events.push(StreamWrite::new(
            &read_streams,
            stream_id.clone(),
            RealisticEvent::TransactionInitiated {
                id: self.transaction_id.clone(),
                from,
                to,
                amount,
            },
        )?);

        // Simulate immediate completion for deposits
        if matches!(self.transaction_type, TransactionType::Deposit) {
            events.push(StreamWrite::new(
                &read_streams,
                stream_id,
                RealisticEvent::TransactionCompleted {
                    id: self.transaction_id,
                },
            )?);
        }

        Ok(events)
    }
}

// Multi-stream command for e-commerce orders
#[derive(Debug, Clone)]
struct EcommerceOrderCommand {
    order_id: String,
    customer_id: String,
    products: Vec<(String, u32)>, // (product_id, quantity)
}

#[derive(Debug, Default)]
struct OrderSystemState {
    customer_orders: HashMap<String, Vec<String>>,
    inventory: HashMap<String, u32>,
    order_status: HashMap<String, OrderStatus>,
}

#[derive(Debug, Clone, PartialEq)]
enum OrderStatus {
    Pending,
    Confirmed,
    Shipped,
    Cancelled,
}

impl eventcore::CommandStreams for EcommerceOrderCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        let mut streams = vec![
            StreamId::try_new(format!("customer-{}", self.customer_id)).unwrap(),
            StreamId::try_new(format!("order-{}", self.order_id)).unwrap(),
        ];

        // Add product streams
        for (product_id, _) in &self.products {
            streams.push(StreamId::try_new(format!("product-{}", product_id)).unwrap());
        }

        streams
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for EcommerceOrderCommand {
    type State = OrderSystemState;
    type Event = RealisticEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            RealisticEvent::OrderPlaced { id, customer, .. } => {
                state
                    .customer_orders
                    .entry(customer.clone())
                    .or_default()
                    .push(id.clone());
                state.order_status.insert(id.clone(), OrderStatus::Pending);
            }
            RealisticEvent::StockReserved {
                product,
                quantity,
                order,
            } => {
                let current = state.inventory.get(product).copied().unwrap_or(1000);
                state
                    .inventory
                    .insert(product.clone(), current.saturating_sub(*quantity));

                // Auto-confirm order when all stock is reserved
                if let Some(status) = state.order_status.get_mut(order) {
                    if *status == OrderStatus::Pending {
                        *status = OrderStatus::Confirmed;
                    }
                }
            }
            RealisticEvent::StockAdded { product, quantity } => {
                let current = state.inventory.get(product).copied().unwrap_or(0);
                state.inventory.insert(product.clone(), current + quantity);
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
        // Check if order already exists
        if state.order_status.contains_key(&self.order_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Order {} already exists",
                self.order_id
            )));
        }

        // Validate inventory
        for (product_id, quantity) in &self.products {
            let available = state.inventory.get(product_id).copied().unwrap_or(1000); // Default stock
            if available < *quantity {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Insufficient stock for product {}: available {}, requested {}",
                    product_id, available, quantity
                )));
            }
        }

        let mut events = vec![];

        // Create order placed event
        let order_stream = StreamId::try_new(format!("order-{}", self.order_id)).unwrap();
        let product_ids: Vec<String> = self.products.iter().map(|(id, _)| id.clone()).collect();

        events.push(StreamWrite::new(
            &read_streams,
            order_stream,
            RealisticEvent::OrderPlaced {
                id: self.order_id.clone(),
                customer: self.customer_id.clone(),
                items: product_ids,
            },
        )?);

        // Reserve inventory for each product
        for (product_id, quantity) in self.products {
            let product_stream = StreamId::try_new(format!("product-{}", product_id)).unwrap();
            events.push(StreamWrite::new(
                &read_streams,
                product_stream,
                RealisticEvent::StockReserved {
                    product: product_id,
                    quantity,
                    order: self.order_id.clone(),
                },
            )?);
        }

        Ok(events)
    }
}

// Performance test runner for production PostgreSQL store

// Performance test runner for PostgreSQL store
struct PostgresPerformanceTestRunner {
    executor: Arc<CommandExecutor<PostgresEventStore<RealisticEvent>>>,
    metrics: Arc<Mutex<Vec<f64>>>,
}

// Removed in-memory performance test runner - only test against production PostgreSQL

impl PostgresPerformanceTestRunner {
    fn new(store: PostgresEventStore<RealisticEvent>) -> Self {
        Self {
            executor: Arc::new(CommandExecutor::new(store)),
            metrics: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // Copy key methods from InMemoryPerformanceTestRunner
    async fn setup_test_data(&self, num_accounts: usize, num_products: usize) {
        // Create test accounts and products with sufficient funds/inventory

        // Create test accounts with initial funds
        for i in 0..num_accounts {
            let account_stream = StreamId::try_new(format!("account-acc{:04}", i)).unwrap();

            // Create account first
            let create_event = EventToWrite {
                event_id: EventId::new(),
                payload: RealisticEvent::AccountCreated {
                    id: format!("acc{:04}", i),
                    email: format!("user{}@example.com", i),
                },
                metadata: None,
            };

            // Give account initial funds (multiple deposits to simulate realistic balance)
            let deposit_event = EventToWrite {
                event_id: EventId::new(),
                payload: RealisticEvent::TransactionInitiated {
                    id: format!("init-deposit-{}", i),
                    from: "external".to_string(),
                    to: format!("acc{:04}", i),
                    amount: 100_000, // $1000 initial funds
                },
                metadata: None,
            };

            let completion_event = EventToWrite {
                event_id: EventId::new(),
                payload: RealisticEvent::TransactionCompleted {
                    id: format!("init-deposit-{}", i),
                },
                metadata: None,
            };

            let stream_events = StreamEvents {
                stream_id: account_stream,
                expected_version: ExpectedVersion::Any, // Handle existing streams gracefully
                events: vec![create_event, deposit_event, completion_event],
            };

            self.executor
                .event_store()
                .write_events_multi(vec![stream_events])
                .await
                .unwrap();
        }

        // Create test products with initial inventory
        for i in 0..num_products {
            let product_stream = StreamId::try_new(format!("product-prod{:04}", i)).unwrap();
            let event = EventToWrite {
                event_id: EventId::new(),
                payload: RealisticEvent::StockAdded {
                    product: format!("prod{:04}", i),
                    quantity: 10000, // High initial stock
                },
                metadata: None,
            };

            let stream_events = StreamEvents {
                stream_id: product_stream,
                expected_version: ExpectedVersion::Any, // Handle existing streams gracefully
                events: vec![event],
            };

            self.executor
                .event_store()
                .write_events_multi(vec![stream_events])
                .await
                .unwrap();
        }

        // Create test customers
        for i in 0..10 {
            let customer_stream = StreamId::try_new(format!("customer-cust{:04}", i)).unwrap();
            let event = EventToWrite {
                event_id: EventId::new(),
                payload: RealisticEvent::AccountCreated {
                    id: format!("cust{:04}", i),
                    email: format!("customer{}@example.com", i),
                },
                metadata: None,
            };

            let stream_events = StreamEvents {
                stream_id: customer_stream,
                expected_version: ExpectedVersion::Any,
                events: vec![event],
            };

            self.executor
                .event_store()
                .write_events_multi(vec![stream_events])
                .await
                .unwrap();
        }
    }

    async fn run_single_stream_test(&self, num_operations: usize) -> PerformanceMetrics {
        // Create fresh metrics collection for this test
        let mut latencies = Vec::with_capacity(num_operations);
        let start = Instant::now();

        for i in 0..num_operations {
            let op_start = Instant::now();

            let command = FinancialTransactionCommand {
                account_id: format!("acc{:04}", i % 20), // Rotate through 20 accounts (we only create 20)
                transaction_id: format!(
                    "txn-{}",
                    uuid::Uuid::new_v7(Timestamp::now(uuid::NoContext))
                ),
                amount: 100 + (i as i64 % 900), // Variable amounts 100-999
                transaction_type: if i % 3 == 0 {
                    TransactionType::Deposit
                } else if i % 3 == 1 {
                    TransactionType::Withdrawal
                } else {
                    TransactionType::Transfer {
                        to_account: format!("acc{:04}", (i + 1) % 20),
                    }
                },
            };

            let result = self
                .executor
                .execute(&command, ExecutionOptions::default())
                .await;
            let duration = op_start.elapsed();

            if result.is_ok() {
                latencies.push(duration.as_micros() as f64 / 1000.0);
            }
        }

        let duration = start.elapsed();
        PerformanceMetrics::calculate(num_operations, duration, latencies)
    }

    async fn run_multi_stream_test(&self, num_operations: usize) -> PerformanceMetrics {
        // Create fresh metrics collection for this test
        let mut latencies = Vec::with_capacity(num_operations);
        let start = Instant::now();

        for i in 0..num_operations {
            let op_start = Instant::now();

            // Create orders with 2-5 products each
            let num_products = 2 + (i % 4);
            let products: Vec<(String, u32)> = (0..num_products)
                .map(|j| (format!("prod{:04}", (i + j) % 10), 1 + (j % 3) as u32)) // Use 10 products (we only create 10)
                .collect();

            let command = EcommerceOrderCommand {
                order_id: format!(
                    "order-{}",
                    uuid::Uuid::new_v7(Timestamp::now(uuid::NoContext))
                ),
                customer_id: format!("cust{:04}", i % 10), // Use reasonable customer range
                products,
            };

            let result = self
                .executor
                .execute(&command, ExecutionOptions::default())
                .await;
            let duration = op_start.elapsed();

            if result.is_ok() {
                latencies.push(duration.as_micros() as f64 / 1000.0);
            }
        }

        let duration = start.elapsed();
        PerformanceMetrics::calculate(num_operations, duration, latencies)
    }

    async fn run_batch_write_test(
        &self,
        num_batches: usize,
        events_per_batch: usize,
    ) -> PerformanceMetrics {
        let start = Instant::now();
        let mut latencies = Vec::with_capacity(num_batches);

        for batch in 0..num_batches {
            let batch_start = Instant::now();

            let stream_id = StreamId::try_new(format!("batch-stream-{}", batch)).unwrap();
            let mut events = Vec::with_capacity(events_per_batch);

            for i in 0..events_per_batch {
                events.push(EventToWrite {
                    event_id: EventId::new(),
                    payload: RealisticEvent::TransactionInitiated {
                        id: format!("batch-{}-{}", batch, i),
                        from: "batch-source".to_string(),
                        to: "batch-target".to_string(),
                        amount: 100,
                    },
                    metadata: None,
                });
            }

            let stream_events = StreamEvents {
                stream_id,
                expected_version: ExpectedVersion::New,
                events,
            };

            let result = self
                .executor
                .event_store()
                .write_events_multi(vec![stream_events])
                .await;

            let duration = batch_start.elapsed();

            if result.is_ok() {
                latencies.push(duration.as_micros() as f64 / 1000.0);
            }
        }

        let duration = start.elapsed();
        let total_events = num_batches * events_per_batch;

        PerformanceMetrics::calculate(total_events, duration, latencies)
    }

    fn validate_single_stream_targets(metrics: &PerformanceMetrics) -> Vec<TargetValidation> {
        vec![
            TargetValidation {
                metric: "Throughput (ops/sec)".to_string(),
                actual: metrics.operations_per_second,
                target_min: Some(TARGET_SINGLE_STREAM_OPS_MIN),
                target_max: Some(TARGET_SINGLE_STREAM_OPS_MAX),
                passed: metrics.operations_per_second >= TARGET_SINGLE_STREAM_OPS_MIN,
                reason: format!(
                    "Expected {}-{} ops/sec",
                    TARGET_SINGLE_STREAM_OPS_MIN, TARGET_SINGLE_STREAM_OPS_MAX
                ),
            },
            TargetValidation {
                metric: "P95 Latency".to_string(),
                actual: metrics.p95_latency_ms,
                target_min: None,
                target_max: Some(TARGET_P95_LATENCY_MS),
                passed: metrics.p95_latency_ms <= TARGET_P95_LATENCY_MS,
                reason: format!("Expected ≤{}ms", TARGET_P95_LATENCY_MS),
            },
        ]
    }

    fn validate_multi_stream_targets(metrics: &PerformanceMetrics) -> Vec<TargetValidation> {
        vec![
            TargetValidation {
                metric: "Throughput (ops/sec)".to_string(),
                actual: metrics.operations_per_second,
                target_min: Some(TARGET_MULTI_STREAM_OPS_MIN),
                target_max: Some(TARGET_MULTI_STREAM_OPS_MAX),
                passed: metrics.operations_per_second >= TARGET_MULTI_STREAM_OPS_MIN,
                reason: format!(
                    "Expected {}-{} ops/sec",
                    TARGET_MULTI_STREAM_OPS_MIN, TARGET_MULTI_STREAM_OPS_MAX
                ),
            },
            TargetValidation {
                metric: "P95 Latency".to_string(),
                actual: metrics.p95_latency_ms,
                target_min: None,
                target_max: Some(TARGET_P95_LATENCY_MS),
                passed: metrics.p95_latency_ms <= TARGET_P95_LATENCY_MS,
                reason: format!("Expected ≤{}ms", TARGET_P95_LATENCY_MS),
            },
        ]
    }

    fn validate_batch_write_targets(metrics: &PerformanceMetrics) -> Vec<TargetValidation> {
        vec![TargetValidation {
            metric: "Throughput (events/sec)".to_string(),
            actual: metrics.operations_per_second,
            target_min: Some(TARGET_BATCH_EVENTS_PER_SEC),
            target_max: None,
            passed: metrics.operations_per_second >= TARGET_BATCH_EVENTS_PER_SEC,
            reason: format!("Expected ≥{} events/sec", TARGET_BATCH_EVENTS_PER_SEC),
        }]
    }
}

// Removed in-memory performance test - not representative of real-world performance

#[tokio::test]
#[ignore = "Requires PostgreSQL - run with 'cargo test test_performance_targets -- --ignored --nocapture'"]
async fn test_performance_targets() {
    println!("\n{}", "=".repeat(100));
    println!("EVENTCORE PERFORMANCE TARGET VALIDATION - PostgreSQL (Production Store)");
    println!("{}", "=".repeat(100));

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    println!("\nConnecting to PostgreSQL at: {}", database_url);

    let store = PostgresEventStore::<RealisticEvent>::new(eventcore_postgres::PostgresConfig::new(
        database_url,
    ))
    .await
    .expect("Failed to connect to PostgreSQL");

    store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    let runner = PostgresPerformanceTestRunner::new(store);

    // Setup test data (smaller for PostgreSQL)
    println!("\nSetting up test data...");
    runner.setup_test_data(20, 10).await;
    println!("✓ Created 20 test accounts and 10 test products");

    // Test 1: Single-stream performance (reduced for PostgreSQL)
    println!("\nRunning single-stream performance test...");
    let single_metrics = runner.run_single_stream_test(1_000).await;
    let single_validations =
        PostgresPerformanceTestRunner::validate_single_stream_targets(&single_metrics);
    let single_passed = single_validations.iter().all(|v| v.passed);

    let single_report = PerformanceReport {
        test_name: "Single-Stream Commands".to_string(),
        scenario: "Financial transactions (PostgreSQL)".to_string(),
        metrics: single_metrics,
        target_met: single_passed,
        target_details: single_validations,
    };
    single_report.display();

    // Test 2: Multi-stream performance (reduced for PostgreSQL)
    println!("\nRunning multi-stream performance test...");
    let multi_metrics = runner.run_multi_stream_test(500).await;
    let multi_validations =
        PostgresPerformanceTestRunner::validate_multi_stream_targets(&multi_metrics);
    let multi_passed = multi_validations.iter().all(|v| v.passed);

    let multi_report = PerformanceReport {
        test_name: "Multi-Stream Commands".to_string(),
        scenario: "E-commerce orders (PostgreSQL)".to_string(),
        metrics: multi_metrics,
        target_met: multi_passed,
        target_details: multi_validations,
    };
    multi_report.display();

    // Test 3: Batch write performance (reduced for PostgreSQL)
    println!("\nRunning batch write performance test...");
    let batch_metrics = runner.run_batch_write_test(20, 100).await;
    let batch_validations =
        PostgresPerformanceTestRunner::validate_batch_write_targets(&batch_metrics);
    let batch_passed = batch_validations.iter().all(|v| v.passed);

    let batch_report = PerformanceReport {
        test_name: "Batch Event Writes".to_string(),
        scenario: "PostgreSQL batch inserts".to_string(),
        metrics: batch_metrics,
        target_met: batch_passed,
        target_details: batch_validations,
    };
    batch_report.display();

    // Summary
    println!("\n{}", "=".repeat(100));
    println!("PERFORMANCE SUMMARY - PostgreSQL (Production Store)");
    println!("{}", "-".repeat(100));
    println!(
        "Single-Stream:  {}",
        if single_passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        }
    );
    println!(
        "Multi-Stream:   {}",
        if multi_passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        }
    );
    println!(
        "Batch Writes:   {}",
        if batch_passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        }
    );
    println!(
        "Overall:        {}",
        if single_passed && multi_passed && batch_passed {
            "✓ ALL TARGETS MET"
        } else {
            "✗ SOME TARGETS NOT MET"
        }
    );
    println!("{}", "=".repeat(100));

    // Don't automatically fail the test - just report results
    println!("\nPerformance validation complete. Results logged above.");
}
