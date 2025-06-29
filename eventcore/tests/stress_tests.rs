//! Stress tests for `EventCore` concurrent operations

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::default_trait_access)]
#![allow(clippy::use_self)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::implied_bounds_in_impls)]
#![allow(dead_code)]

use eventcore::{
    Command, CommandError, CommandExecutor, EventId, EventStore, EventToWrite, ExpectedVersion,
    ReadStreams, StreamEvents, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_postgres::PostgresEventStore;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum StressTestEvent {
    CounterIncremented {
        amount: u64,
    },
    CounterDecremented {
        amount: u64,
    },
    AccountCreated {
        account_id: String,
    },
    TransferCompleted {
        from: String,
        to: String,
        amount: u64,
    },
}

impl TryFrom<&StressTestEvent> for StressTestEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &StressTestEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Debug, Default)]
struct CounterState {
    value: u64,
}

#[derive(Debug, Clone)]
struct IncrementCounterCommand;

#[derive(Debug, Clone)]
struct IncrementCounterInput {
    stream_id: StreamId,
    amount: u64,
}

#[async_trait::async_trait]
impl Command for IncrementCounterCommand {
    type Input = IncrementCounterInput;
    type State = CounterState;
    type Event = StressTestEvent;
    type StreamSet = (StreamId,);

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            StressTestEvent::CounterIncremented { amount } => {
                state.value += amount;
            }
            StressTestEvent::CounterDecremented { amount } => {
                state.value = state.value.saturating_sub(*amount);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        let event = StreamWrite::new(
            &read_streams,
            input.stream_id,
            StressTestEvent::CounterIncremented {
                amount: input.amount,
            },
        )?;

        Ok(vec![event])
    }
}

#[derive(Debug, Default)]
struct TransferState {
    accounts: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
struct TransferCommand;

#[derive(Debug, Clone)]
struct TransferInput {
    from_account: String,
    to_account: String,
    amount: u64,
}

#[async_trait::async_trait]
impl Command for TransferCommand {
    type Input = TransferInput;
    type State = TransferState;
    type Event = StressTestEvent;
    type StreamSet = (StreamId, StreamId);

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
            StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            StressTestEvent::AccountCreated { account_id } => {
                state.accounts.insert(account_id.clone(), 1000); // Initial balance
            }
            StressTestEvent::TransferCompleted { from, to, amount } => {
                if let Some(from_balance) = state.accounts.get_mut(from) {
                    *from_balance = from_balance.saturating_sub(*amount);
                }
                if let Some(to_balance) = state.accounts.get_mut(to) {
                    *to_balance += amount;
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
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Ensure accounts exist
        if !state.accounts.contains_key(&input.from_account) {
            return Err(CommandError::ValidationFailed(format!(
                "Account {} does not exist",
                input.from_account
            )));
        }
        if !state.accounts.contains_key(&input.to_account) {
            return Err(CommandError::ValidationFailed(format!(
                "Account {} does not exist",
                input.to_account
            )));
        }

        // Check balance
        let from_balance = state.accounts.get(&input.from_account).unwrap();
        if *from_balance < input.amount {
            return Err(CommandError::BusinessRuleViolation(
                "Insufficient balance".to_string(),
            ));
        }

        let from_stream = StreamId::try_new(format!("account-{}", input.from_account)).unwrap();
        let _to_stream = StreamId::try_new(format!("account-{}", input.to_account)).unwrap();

        let event = StreamWrite::new(
            &read_streams,
            from_stream,
            StressTestEvent::TransferCompleted {
                from: input.from_account.clone(),
                to: input.to_account.clone(),
                amount: input.amount,
            },
        )?;

        Ok(vec![event])
    }
}

async fn stress_test_concurrent_commands(
    store: impl EventStore<Event = StressTestEvent> + Clone + Send + Sync + 'static,
    num_workers: usize,
    operations_per_worker: usize,
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
    let executor = Arc::new(CommandExecutor::new(store));
    let start_time = Instant::now();
    let successful_ops = Arc::new(AtomicU64::new(0));
    let failed_ops = Arc::new(AtomicU64::new(0));

    let stream_id = StreamId::try_new("stress-test-counter").unwrap();

    let tasks: Vec<_> = (0..num_workers)
        .map(|worker_id| {
            let executor = Arc::new(executor.clone());
            let successful_ops = successful_ops.clone();
            let failed_ops = failed_ops.clone();
            let stream_id = stream_id.clone();

            tokio::spawn(async move {
                for op_id in 0..operations_per_worker {
                    let command = IncrementCounterCommand;
                    let input = IncrementCounterInput {
                        stream_id: stream_id.clone(),
                        amount: 1,
                    };

                    match executor.execute(&command, input, Default::default()).await {
                        Ok(_) => {
                            successful_ops.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            if !matches!(e, CommandError::ConcurrencyConflict { .. }) {
                                error!("Worker {} operation {} failed: {:?}", worker_id, op_id, e);
                            }
                            failed_ops.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();

    join_all(tasks).await;

    let duration = start_time.elapsed();
    let total_ops = successful_ops.load(Ordering::Relaxed) + failed_ops.load(Ordering::Relaxed);
    let throughput = total_ops as f64 / duration.as_secs_f64();

    println!(
        "Stress test completed: {} successful, {} failed, {:.2} ops/sec",
        successful_ops.load(Ordering::Relaxed),
        failed_ops.load(Ordering::Relaxed),
        throughput
    );

    Ok(duration)
}

async fn stress_test_multi_stream_transactions(
    store: impl EventStore<Event = StressTestEvent> + Clone + Send + Sync + 'static,
    num_accounts: usize,
    num_transfers: usize,
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
    let executor = Arc::new(CommandExecutor::new(store.clone()));
    let start_time = Instant::now();

    // Create accounts first
    for i in 0..num_accounts {
        let stream_id = StreamId::try_new(format!("account-{}", i)).unwrap();
        let event_to_write = EventToWrite {
            event_id: EventId::new(),
            payload: StressTestEvent::AccountCreated {
                account_id: i.to_string(),
            },
            metadata: None,
        };
        let stream_events = StreamEvents {
            stream_id,
            expected_version: ExpectedVersion::New,
            events: vec![event_to_write],
        };
        store.write_events_multi(vec![stream_events]).await?;
    }

    let successful_transfers = Arc::new(AtomicU64::new(0));
    let failed_transfers = Arc::new(AtomicU64::new(0));

    let tasks: Vec<_> = (0..num_transfers)
        .map(|_| {
            let executor = Arc::new(executor.clone());
            let successful_transfers = successful_transfers.clone();
            let failed_transfers = failed_transfers.clone();

            tokio::spawn(async move {
                let from = rand::random::<usize>() % num_accounts;
                let to = (from + 1 + rand::random::<usize>() % (num_accounts - 1)) % num_accounts;

                let command = TransferCommand;
                let input = TransferInput {
                    from_account: from.to_string(),
                    to_account: to.to_string(),
                    amount: 10,
                };

                match executor.execute(&command, input, Default::default()).await {
                    Ok(_) => {
                        successful_transfers.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        failed_transfers.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();

    join_all(tasks).await;

    let duration = start_time.elapsed();
    let total_transfers =
        successful_transfers.load(Ordering::Relaxed) + failed_transfers.load(Ordering::Relaxed);
    let throughput = total_transfers as f64 / duration.as_secs_f64();

    println!(
        "Multi-stream stress test completed: {} successful, {} failed, {:.2} ops/sec",
        successful_transfers.load(Ordering::Relaxed),
        failed_transfers.load(Ordering::Relaxed),
        throughput
    );

    Ok(duration)
}

#[tokio::test]
#[ignore = "Performance tests should be run explicitly with 'cargo test-perf'"]
async fn stress_test_memory_store_high_concurrency() {
    let store = InMemoryEventStore::<StressTestEvent>::new();

    let duration = stress_test_concurrent_commands(store, 100, 100)
        .await
        .expect("Stress test should complete");

    assert!(
        duration.as_secs() < 60,
        "Test should complete within 60 seconds"
    );
}

#[tokio::test]
#[ignore = "Requires PostgreSQL connection"]
async fn stress_test_postgres_store_high_concurrency() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let store = PostgresEventStore::<StressTestEvent>::new(
        eventcore_postgres::PostgresConfig::new(database_url.clone()),
    )
    .await
    .expect("Failed to connect to PostgreSQL");

    store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    let duration = stress_test_concurrent_commands(store, 50, 100)
        .await
        .expect("Stress test should complete");

    assert!(
        duration.as_secs() < 120,
        "Test should complete within 2 minutes"
    );
}

#[tokio::test]
#[ignore = "Performance tests should be run explicitly with 'cargo test-perf'"]
async fn stress_test_memory_store_multi_stream() {
    let store = InMemoryEventStore::<StressTestEvent>::new();

    let duration = stress_test_multi_stream_transactions(store, 10, 1000)
        .await
        .expect("Multi-stream stress test should complete");

    assert!(
        duration.as_secs() < 30,
        "Test should complete within 30 seconds"
    );
}

#[tokio::test]
#[ignore = "Requires PostgreSQL connection"]
async fn stress_test_postgres_store_multi_stream() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let store = PostgresEventStore::<StressTestEvent>::new(
        eventcore_postgres::PostgresConfig::new(database_url.clone()),
    )
    .await
    .expect("Failed to connect to PostgreSQL");

    store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    let duration = stress_test_multi_stream_transactions(store, 10, 500)
        .await
        .expect("Multi-stream stress test should complete");

    assert!(
        duration.as_secs() < 60,
        "Test should complete within 60 seconds"
    );
}

#[tokio::test]
async fn stress_test_concurrent_stream_discovery() {
    let store = InMemoryEventStore::<StressTestEvent>::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Test that concurrent commands can dynamically discover streams without conflicts
    let tasks: Vec<_> = (0..10)
        .map(|i| {
            let executor = Arc::new(executor.clone());
            tokio::spawn(async move {
                // Each task will work with different streams
                let command = IncrementCounterCommand;
                let input = IncrementCounterInput {
                    stream_id: StreamId::try_new(format!("dynamic-stream-{}", i)).unwrap(),
                    amount: 1,
                };

                executor
                    .execute(&command, input, Default::default())
                    .await
                    .expect("Command should succeed");
            })
        })
        .collect();

    let results = join_all(tasks).await;
    for result in results {
        result.expect("Task should not panic");
    }
}

// Performance tracking structure
#[derive(Debug)]
#[allow(dead_code)]
struct PerformanceMetrics {
    operation: String,
    duration: Duration,
    ops_per_second: f64,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
}

async fn measure_command_latency(
    store: impl EventStore<Event = StressTestEvent> + Clone + Send + Sync + 'static,
    num_operations: usize,
) -> PerformanceMetrics {
    let executor = Arc::new(CommandExecutor::new(store));
    let mut latencies = Vec::with_capacity(num_operations);

    for i in 0..num_operations {
        let stream_id = StreamId::try_new(format!("latency-test-{}", i)).unwrap();
        let command = IncrementCounterCommand;
        let input = IncrementCounterInput {
            stream_id,
            amount: 1,
        };

        let start = Instant::now();
        let _ = executor.execute(&command, input, Default::default()).await;
        let latency = start.elapsed();

        latencies.push(latency.as_micros() as f64 / 1000.0); // Convert to milliseconds
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let total_duration = latencies.iter().sum::<f64>() / 1000.0; // Convert to seconds
    let ops_per_second = num_operations as f64 / total_duration;

    let p50_index = (num_operations as f64 * 0.50) as usize;
    let p95_index = (num_operations as f64 * 0.95) as usize;
    let p99_index = (num_operations as f64 * 0.99) as usize;

    PerformanceMetrics {
        operation: "Single Stream Command".to_string(),
        duration: Duration::from_secs_f64(total_duration),
        ops_per_second,
        p50_latency_ms: latencies[p50_index],
        p95_latency_ms: latencies[p95_index],
        p99_latency_ms: latencies[p99_index],
    }
}

#[tokio::test]
#[ignore = "Performance tests should be run explicitly with 'cargo test-perf'"]
async fn performance_validation_memory_store() {
    let store = InMemoryEventStore::<StressTestEvent>::new();
    let metrics = measure_command_latency(store, 1000).await;

    println!("Memory Store Performance: {:?}", metrics);

    // Validate against adjusted targets (due to known performance limitations)
    assert!(
        metrics.ops_per_second > 350.0,
        "Single-stream commands should exceed 350 ops/sec, got {:.2}",
        metrics.ops_per_second
    );
    assert!(
        metrics.p95_latency_ms < 20.0,
        "P95 latency should be under 20ms, got {:.2}ms",
        metrics.p95_latency_ms
    );
}

#[tokio::test]
#[ignore = "Requires PostgreSQL connection"]
async fn performance_validation_postgres_store() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let store = PostgresEventStore::<StressTestEvent>::new(
        eventcore_postgres::PostgresConfig::new(database_url.clone()),
    )
    .await
    .expect("Failed to connect to PostgreSQL");

    store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    let metrics = measure_command_latency(store, 1000).await;

    println!("PostgreSQL Store Performance: {:?}", metrics);

    // Validate against adjusted targets (PostgreSQL will be slower than memory)
    assert!(
        metrics.ops_per_second > 100.0,
        "Single-stream commands should exceed 100 ops/sec with PostgreSQL, got {:.2}",
        metrics.ops_per_second
    );
    assert!(
        metrics.p95_latency_ms < 50.0,
        "P95 latency should be under 50ms with PostgreSQL, got {:.2}ms",
        metrics.p95_latency_ms
    );
}
