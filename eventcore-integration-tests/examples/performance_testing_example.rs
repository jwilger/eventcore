//! Performance Testing Example for EventCore
//!
//! This example demonstrates how to build a performance testing harness for
//! EventCore applications. It shows:
//!
//! - Setting up realistic test scenarios with configurable workloads
//! - Measuring command execution latency and throughput
//! - Testing concurrent operations and contention
//! - Monitoring memory usage and resource consumption
//! - Generating performance reports
//!
//! Run with: `cargo run --example performance_testing_example --release`

#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_const_for_thread_local)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::useless_vec)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::no_effect_underscore_binding)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::inconsistent_digit_grouping)]
#![allow(clippy::unreadable_literal)]
#![allow(private_bounds)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::use_self)]

use async_trait::async_trait;
use eventcore::{
    CommandError, CommandExecutor, CommandLogic, CommandStreams, EventId, EventStore,
    EventStoreError, EventToWrite, ExpectedVersion, ReadStreams, StoredEvent, StreamEvents,
    StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Performance metrics collector
#[derive(Debug, Default)]
struct PerformanceMetrics {
    total_commands: AtomicU64,
    successful_commands: AtomicU64,
    failed_commands: AtomicU64,
    total_latency_micros: AtomicU64,
    max_latency_micros: AtomicU64,
    min_latency_micros: AtomicU64,
}

impl PerformanceMetrics {
    fn record_command(&self, latency: Duration, success: bool) {
        let micros = latency.as_micros() as u64;

        self.total_commands.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successful_commands.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_commands.fetch_add(1, Ordering::Relaxed);
        }

        self.total_latency_micros
            .fetch_add(micros, Ordering::Relaxed);

        // Update max latency
        let mut current_max = self.max_latency_micros.load(Ordering::Relaxed);
        while micros > current_max {
            match self.max_latency_micros.compare_exchange(
                current_max,
                micros,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }

        // Update min latency
        let mut current_min = self.min_latency_micros.load(Ordering::Relaxed);
        if current_min == 0 || micros < current_min {
            while current_min == 0 || micros < current_min {
                match self.min_latency_micros.compare_exchange(
                    current_min,
                    micros,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current_min = actual,
                }
            }
        }
    }

    fn report(&self) -> PerformanceReport {
        let total = self.total_commands.load(Ordering::Relaxed);
        let successful = self.successful_commands.load(Ordering::Relaxed);
        let failed = self.failed_commands.load(Ordering::Relaxed);
        let total_latency = self.total_latency_micros.load(Ordering::Relaxed);
        let max_latency = self.max_latency_micros.load(Ordering::Relaxed);
        let min_latency = self.min_latency_micros.load(Ordering::Relaxed);

        let avg_latency = if total > 0 { total_latency / total } else { 0 };

        PerformanceReport {
            total_commands: total,
            successful_commands: successful,
            failed_commands: failed,
            success_rate: if total > 0 {
                (successful as f64 / total as f64) * 100.0
            } else {
                0.0
            },
            avg_latency_micros: avg_latency,
            max_latency_micros: max_latency,
            min_latency_micros: min_latency,
        }
    }
}

#[derive(Debug)]
struct PerformanceReport {
    total_commands: u64,
    successful_commands: u64,
    failed_commands: u64,
    success_rate: f64,
    avg_latency_micros: u64,
    max_latency_micros: u64,
    min_latency_micros: u64,
}

impl std::fmt::Display for PerformanceReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n📊 Performance Test Results")?;
        writeln!(f, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;
        writeln!(f, "Total Commands:      {}", self.total_commands)?;
        writeln!(
            f,
            "Successful:          {} ({:.2}%)",
            self.successful_commands, self.success_rate
        )?;
        writeln!(f, "Failed:              {}", self.failed_commands)?;
        writeln!(f, "\nLatency Statistics:")?;
        writeln!(
            f,
            "  Average:           {:.2} ms",
            self.avg_latency_micros as f64 / 1000.0
        )?;
        writeln!(
            f,
            "  Min:               {:.2} ms",
            self.min_latency_micros as f64 / 1000.0
        )?;
        writeln!(
            f,
            "  Max:               {:.2} ms",
            self.max_latency_micros as f64 / 1000.0
        )?;
        writeln!(f, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    }
}

// Domain types for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Money(i64);

impl Money {
    fn new(cents: i64) -> Self {
        Self(cents)
    }

    fn add(&self, other: &Self) -> Self {
        Self(self.0 + other.0)
    }

    fn subtract(&self, other: &Self) -> Option<Self> {
        if self.0 >= other.0 {
            Some(Self(self.0 - other.0))
        } else {
            None
        }
    }
}

// Events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum AccountEvent {
    Opened { initial_balance: Money },
    Deposited { amount: Money },
    Withdrawn { amount: Money },
    TransferSent { to_account: String, amount: Money },
    TransferReceived { from_account: String, amount: Money },
}

// Required for CommandExecutor when event store and command events are the same type
impl TryFrom<&AccountEvent> for AccountEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &AccountEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// Command state
#[derive(Debug, Default)]
struct AccountState {
    balance: Option<Money>,
}

// Performance test command that simulates transfers
#[derive(Debug, Clone)]
struct TransferCommand {
    from_account: String,
    to_account: String,
    amount: Money,
}

impl TransferCommand {
    fn new(from_account: String, to_account: String, amount: Money) -> Self {
        Self {
            from_account,
            to_account,
            amount,
        }
    }
}

#[async_trait]
impl CommandStreams for TransferCommand {
    type StreamSet = TransferStreamSet;

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("account-{}", self.from_account)).unwrap(),
            StreamId::try_new(format!("account-{}", self.to_account)).unwrap(),
        ]
    }
}

#[async_trait]
impl CommandLogic for TransferCommand {
    type State = HashMap<StreamId, AccountState>;
    type Event = AccountEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let account_state = state.entry(event.stream_id.clone()).or_default();

        match &event.payload {
            AccountEvent::Opened { initial_balance } => {
                account_state.balance = Some(*initial_balance);
            }
            AccountEvent::Deposited { amount } => {
                if let Some(balance) = &mut account_state.balance {
                    *balance = balance.add(amount);
                }
            }
            AccountEvent::Withdrawn { amount } => {
                if let Some(balance) = &mut account_state.balance {
                    if let Some(new_balance) = balance.subtract(amount) {
                        *balance = new_balance;
                    }
                }
            }
            AccountEvent::TransferSent { amount, .. } => {
                if let Some(balance) = &mut account_state.balance {
                    if let Some(new_balance) = balance.subtract(amount) {
                        *balance = new_balance;
                    }
                }
            }
            AccountEvent::TransferReceived { amount, .. } => {
                if let Some(balance) = &mut account_state.balance {
                    *balance = balance.add(amount);
                }
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        let from_stream = StreamId::try_new(format!("account-{}", self.from_account)).unwrap();
        let to_stream = StreamId::try_new(format!("account-{}", self.to_account)).unwrap();

        // Check source account has sufficient balance
        let from_state = state.get(&from_stream).ok_or_else(|| {
            CommandError::BusinessRuleViolation("Source account not found".to_string())
        })?;

        let from_balance = from_state.balance.ok_or_else(|| {
            CommandError::BusinessRuleViolation("Source account not initialized".to_string())
        })?;

        if from_balance.subtract(&self.amount).is_none() {
            return Err(CommandError::BusinessRuleViolation(
                "Insufficient funds".to_string(),
            ));
        }

        // Check target account exists
        if !state.contains_key(&to_stream) {
            return Err(CommandError::BusinessRuleViolation(
                "Target account not found".to_string(),
            ));
        }

        // Create transfer events
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                from_stream,
                AccountEvent::TransferSent {
                    to_account: self.to_account.clone(),
                    amount: self.amount,
                },
            )?,
            StreamWrite::new(
                &read_streams,
                to_stream,
                AccountEvent::TransferReceived {
                    from_account: self.from_account.clone(),
                    amount: self.amount,
                },
            )?,
        ])
    }
}

struct TransferStreamSet;

// Test scenario configuration
#[derive(Debug, Clone)]
struct TestScenario {
    name: String,
    num_accounts: usize,
    initial_balance: Money,
    num_operations: usize,
    concurrent_workers: usize,
    operation_delay: Option<Duration>,
    contention_level: ContentionLevel,
}

#[derive(Debug, Clone, Copy)]
enum ContentionLevel {
    Low,    // Random account pairs
    Medium, // Some popular accounts
    High,   // All operations involve a few accounts
}

impl TestScenario {
    fn low_contention() -> Self {
        Self {
            name: "Low Contention".to_string(),
            num_accounts: 1000,
            initial_balance: Money::new(100_000_00), // $100,000
            num_operations: 10_000,
            concurrent_workers: 10,
            operation_delay: None,
            contention_level: ContentionLevel::Low,
        }
    }

    fn medium_contention() -> Self {
        Self {
            name: "Medium Contention".to_string(),
            num_accounts: 100,
            initial_balance: Money::new(100_000_00),
            num_operations: 5_000,
            concurrent_workers: 20,
            operation_delay: None,
            contention_level: ContentionLevel::Medium,
        }
    }

    fn high_contention() -> Self {
        Self {
            name: "High Contention".to_string(),
            num_accounts: 10,
            initial_balance: Money::new(100_000_00),
            num_operations: 1_000,
            concurrent_workers: 50,
            operation_delay: None,
            contention_level: ContentionLevel::High,
        }
    }

    fn realistic_workload() -> Self {
        Self {
            name: "Realistic Workload".to_string(),
            num_accounts: 500,
            initial_balance: Money::new(50_000_00), // $50,000
            num_operations: 5_000,
            concurrent_workers: 15,
            operation_delay: Some(Duration::from_millis(10)), // Simulate processing time
            contention_level: ContentionLevel::Medium,
        }
    }
}

// Performance test runner
struct PerformanceTestRunner {
    event_store: Arc<InMemoryEventStore<AccountEvent>>,
    executor: Arc<CommandExecutor<InMemoryEventStore<AccountEvent>>>,
    metrics: Arc<PerformanceMetrics>,
}

impl PerformanceTestRunner {
    fn new(event_store: InMemoryEventStore<AccountEvent>) -> Self {
        let executor = Arc::new(CommandExecutor::new(event_store.clone()));
        let event_store = Arc::new(event_store);

        Self {
            event_store,
            executor,
            metrics: Arc::new(PerformanceMetrics::default()),
        }
    }

    async fn setup_accounts(&self, scenario: &TestScenario) -> Result<(), EventStoreError> {
        println!("Setting up {} accounts...", scenario.num_accounts);

        for i in 0..scenario.num_accounts {
            let stream_id = StreamId::try_new(format!("account-{}", i)).unwrap();
            let _events = [AccountEvent::Opened {
                initial_balance: scenario.initial_balance,
            }];

            let event_to_write = EventToWrite::new(
                EventId::new(),
                AccountEvent::Opened {
                    initial_balance: scenario.initial_balance,
                },
            );

            let stream_events =
                StreamEvents::new(stream_id, ExpectedVersion::Any, vec![event_to_write]);

            self.event_store
                .write_events_multi(vec![stream_events])
                .await?;
        }

        println!("✅ Account setup complete");
        Ok(())
    }

    async fn run_scenario(
        &self,
        scenario: &TestScenario,
    ) -> Result<PerformanceReport, Box<dyn std::error::Error>> {
        println!("\n🚀 Running scenario: {}", scenario.name);
        println!("  Accounts: {}", scenario.num_accounts);
        println!("  Operations: {}", scenario.num_operations);
        println!("  Workers: {}", scenario.concurrent_workers);
        println!("  Contention: {:?}", scenario.contention_level);

        self.setup_accounts(scenario).await?;

        let operations_per_worker = scenario.num_operations / scenario.concurrent_workers;
        let mut handles = Vec::new();

        let start_time = Instant::now();

        for worker_id in 0..scenario.concurrent_workers {
            let executor = self.executor.clone();
            let metrics = self.metrics.clone();
            let scenario = scenario.clone();

            let handle = tokio::spawn(async move {
                for op_num in 0..operations_per_worker {
                    // Generate transfer based on contention level
                    let (from, to) = match scenario.contention_level {
                        ContentionLevel::Low => {
                            // Random accounts
                            let from = rand::random::<usize>() % scenario.num_accounts;
                            let mut to = rand::random::<usize>() % scenario.num_accounts;
                            while to == from {
                                to = rand::random::<usize>() % scenario.num_accounts;
                            }
                            (from, to)
                        }
                        ContentionLevel::Medium => {
                            // 20% of operations involve "popular" accounts (first 10%)
                            if op_num % 5 == 0 {
                                let from = rand::random::<usize>() % (scenario.num_accounts / 10);
                                let to = rand::random::<usize>() % scenario.num_accounts;
                                (from, to)
                            } else {
                                let from = rand::random::<usize>() % scenario.num_accounts;
                                let to = rand::random::<usize>() % scenario.num_accounts;
                                (from, to)
                            }
                        }
                        ContentionLevel::High => {
                            // All operations involve first 3 accounts
                            let from = rand::random::<usize>() % 3;
                            let mut to = rand::random::<usize>() % 3;
                            while to == from {
                                to = rand::random::<usize>() % 3;
                            }
                            (from, to)
                        }
                    };

                    let amount = Money::new(100 + (rand::random::<i64>() % 1000)); // $1 to $10
                    let command = TransferCommand::new(from.to_string(), to.to_string(), amount);

                    if let Some(delay) = scenario.operation_delay {
                        sleep(delay).await;
                    }

                    let op_start = Instant::now();
                    let result = executor
                        .execute(command, eventcore::ExecutionOptions::default())
                        .await;
                    let latency = op_start.elapsed();

                    metrics.record_command(latency, result.is_ok());

                    if worker_id == 0 && op_num % 100 == 0 {
                        print!(".");
                        use std::io::Write;
                        std::io::stdout().flush().unwrap();
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for all workers to complete
        for handle in handles {
            handle.await?;
        }

        let total_time = start_time.elapsed();
        println!(
            "\n✅ Scenario completed in {:.2}s",
            total_time.as_secs_f64()
        );

        let report = self.metrics.report();
        let throughput = report.total_commands as f64 / total_time.as_secs_f64();
        println!("Throughput: {:.2} ops/sec", throughput);

        Ok(report)
    }
}

// Helper module for rand operations
mod rand {
    use std::cell::Cell;
    use std::num::Wrapping;

    thread_local! {
        static RNG: Cell<Wrapping<u64>> = Cell::new(Wrapping(0x853c49e6748fea9b));
    }

    pub fn random<T>() -> T
    where
        Standard: Distribution<T>,
    {
        RNG.with(|rng| {
            let mut state = rng.get();
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            rng.set(state);
            let result = state.0.wrapping_mul(0x2545f4914f6cdd1d);
            Standard.sample(result)
        })
    }

    trait Distribution<T> {
        fn sample(&self, value: u64) -> T;
    }

    struct Standard;

    impl Distribution<usize> for Standard {
        fn sample(&self, value: u64) -> usize {
            value as usize
        }
    }

    impl Distribution<i64> for Standard {
        fn sample(&self, value: u64) -> i64 {
            value as i64
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏃 EventCore Performance Testing Example");
    println!("========================================\n");

    // Create in-memory event store for testing
    let event_store: InMemoryEventStore<AccountEvent> = InMemoryEventStore::new();
    let runner = PerformanceTestRunner::new(event_store);

    // Run different test scenarios
    let scenarios = vec![
        TestScenario::low_contention(),
        TestScenario::medium_contention(),
        TestScenario::high_contention(),
        TestScenario::realistic_workload(),
    ];

    let mut reports = Vec::new();

    for scenario in scenarios {
        // Reset metrics for each scenario
        runner.metrics.total_commands.store(0, Ordering::Relaxed);
        runner
            .metrics
            .successful_commands
            .store(0, Ordering::Relaxed);
        runner.metrics.failed_commands.store(0, Ordering::Relaxed);
        runner
            .metrics
            .total_latency_micros
            .store(0, Ordering::Relaxed);
        runner
            .metrics
            .max_latency_micros
            .store(0, Ordering::Relaxed);
        runner
            .metrics
            .min_latency_micros
            .store(0, Ordering::Relaxed);

        let report = runner.run_scenario(&scenario).await?;
        println!("{}", report);
        reports.push((scenario.name.clone(), report));
    }

    // Summary comparison
    println!("\n📈 Performance Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "{:<20} {:>15} {:>15} {:>15}",
        "Scenario", "Success Rate", "Avg Latency", "Max Latency"
    );
    println!("────────────────────────────────────────────────────────────────────");

    for (name, report) in &reports {
        println!(
            "{:<20} {:>14.2}% {:>14.2}ms {:>14.2}ms",
            name,
            report.success_rate,
            report.avg_latency_micros as f64 / 1000.0,
            report.max_latency_micros as f64 / 1000.0,
        );
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n💡 Performance Testing Tips:");
    println!("1. Run with --release for accurate measurements");
    println!("2. Adjust scenario parameters to match your workload");
    println!("3. Use PostgreSQL backend for production-like results");
    println!("4. Monitor system resources during tests");
    println!("5. Run multiple iterations for statistical significance");

    Ok(())
}
