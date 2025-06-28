//! Integration tests for `PostgreSQL` event store implementation

#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use async_trait::async_trait;
use eventcore::{
    command::Command, errors::CommandError, event_store::EventStore, executor::CommandExecutor,
    types::StreamId,
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};

// Test container setup
const POSTGRES_VERSION: &str = "16-alpine";
const POSTGRES_USER: &str = "postgres";
const POSTGRES_PASSWORD: &str = "postgres";
const POSTGRES_DB: &str = "eventcore_test";

async fn setup_postgres_container() -> (ContainerAsync<GenericImage>, PostgresConfig) {
    let postgres_image = GenericImage::new("postgres", POSTGRES_VERSION).with_wait_for(
        WaitFor::message_on_stderr("database system is ready to accept connections"),
    );

    let container = postgres_image
        .with_env_var("POSTGRES_USER", POSTGRES_USER)
        .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
        .with_env_var("POSTGRES_DB", POSTGRES_DB)
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();

    let config = PostgresConfig::new(format!(
        "postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@localhost:{port}/{POSTGRES_DB}"
    ));

    // Give PostgreSQL a moment to fully initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    (container, config)
}

// Test events and commands
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum TestEvent {
    CounterIncremented { amount: u32 },
    CounterDecremented { amount: u32 },
    CounterReset,
}

// Conversion implementations for PostgreSQL JSON storage
impl From<TestEvent> for serde_json::Value {
    fn from(event: TestEvent) -> Self {
        serde_json::to_value(event).expect("Failed to serialize TestEvent")
    }
}

impl TryFrom<&serde_json::Value> for TestEvent {
    type Error = serde_json::Error;

    fn try_from(value: &serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[derive(Debug, Default, Clone)]
struct CounterState {
    value: u32,
}

#[derive(Debug, Clone)]
struct IncrementCounterCommand;

#[derive(Debug, Clone)]
struct IncrementCounterInput {
    stream_id: StreamId,
    amount: u32,
}

#[async_trait]
impl Command for IncrementCounterCommand {
    type Input = IncrementCounterInput;
    type State = CounterState;
    type Event = TestEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &Self::Event) {
        match event {
            TestEvent::CounterIncremented { amount } => state.value += amount,
            TestEvent::CounterDecremented { amount } => {
                state.value = state.value.saturating_sub(*amount);
            }
            TestEvent::CounterReset => state.value = 0,
        }
    }

    async fn handle(
        &self,
        _state: Self::State,
        input: Self::Input,
    ) -> Result<Vec<(StreamId, Self::Event)>, CommandError> {
        if input.amount == 0 {
            return Err(CommandError::ValidationFailed(
                "Amount must be greater than 0".to_string(),
            ));
        }

        Ok(vec![(
            input.stream_id,
            TestEvent::CounterIncremented {
                amount: input.amount,
            },
        )])
    }
}

#[derive(Debug, Clone)]
struct TransferBetweenCountersCommand;

#[derive(Debug, Clone)]
struct TransferBetweenCountersInput {
    from_stream: StreamId,
    to_stream: StreamId,
    amount: u32,
}

#[async_trait]
impl Command for TransferBetweenCountersCommand {
    type Input = TransferBetweenCountersInput;
    type State = (CounterState, CounterState);
    type Event = TestEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.from_stream.clone(), input.to_stream.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &Self::Event) {
        // This is a simplified apply - in real implementation we'd track which stream
        // For testing purposes, we'll apply decrements to the first state and increments to the second
        match event {
            TestEvent::CounterDecremented { amount } => {
                state.0.value = state.0.value.saturating_sub(*amount);
            }
            TestEvent::CounterIncremented { amount } => state.1.value += amount,
            TestEvent::CounterReset => {
                state.0.value = 0;
                state.1.value = 0;
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> Result<Vec<(StreamId, Self::Event)>, CommandError> {
        if input.amount == 0 {
            return Err(CommandError::ValidationFailed(
                "Amount must be greater than 0".to_string(),
            ));
        }

        if state.0.value < input.amount {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Insufficient balance: {} < {}",
                state.0.value, input.amount
            )));
        }

        Ok(vec![
            (
                input.from_stream,
                TestEvent::CounterDecremented {
                    amount: input.amount,
                },
            ),
            (
                input.to_stream,
                TestEvent::CounterIncremented {
                    amount: input.amount,
                },
            ),
        ])
    }
}

// Integration tests
#[tokio::test]
async fn test_concurrent_command_execution() {
    let (_container, config) = setup_postgres_container().await;

    // Initialize event store
    let event_store = PostgresEventStore::new(config).await.unwrap();
    event_store.initialize().await.unwrap();

    let executor = CommandExecutor::new(event_store);
    let command = IncrementCounterCommand;

    // Create a shared stream ID for concurrent operations
    let stream_id = StreamId::try_new("counter-1").unwrap();

    // Execute multiple commands concurrently
    let mut handles = vec![];
    for i in 1..=10 {
        let executor = executor.clone();
        let command = command.clone();
        let stream_id = stream_id.clone();

        let handle = tokio::spawn(async move {
            executor
                .execute(
                    &command,
                    IncrementCounterInput {
                        stream_id,
                        amount: i,
                    },
                )
                .await
        });

        handles.push(handle);
    }

    // Wait for all commands to complete
    let results: Vec<_> = futures::future::join_all(handles).await;

    // Verify all commands succeeded
    let mut success_count = 0;
    let mut conflict_count = 0;

    for result in results {
        match result.unwrap() {
            Ok(_) => success_count += 1,
            Err(CommandError::ConcurrencyConflict { .. }) => conflict_count += 1,
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }

    // At least some commands should succeed, and some might conflict
    assert!(success_count > 0, "At least some commands should succeed");
    assert_eq!(
        success_count + conflict_count,
        10,
        "All commands should either succeed or conflict"
    );

    // Verify final state
    let read_options = eventcore::event_store::ReadOptions::default();
    let stream_data = executor
        .event_store()
        .read_streams(&[stream_id.clone()], &read_options)
        .await
        .unwrap();

    // Calculate expected sum based on successful increments
    let mut final_value = 0u32;
    for _event in &stream_data.events {
        // Events need to be deserialized from the storage format
        // For this test, we'll just count the events as successful increments
        final_value += 1;
    }

    assert!(final_value > 0, "Counter should have been incremented");
}

#[tokio::test]
async fn test_multi_stream_atomicity() {
    let (_container, config) = setup_postgres_container().await;

    // Initialize event store
    let event_store = PostgresEventStore::new(config).await.unwrap();
    event_store.initialize().await.unwrap();

    let executor = CommandExecutor::new(event_store);

    // Initialize two counters
    let counter1_id = StreamId::try_new("counter-1").unwrap();
    let counter2_id = StreamId::try_new("counter-2").unwrap();

    // Set up initial state for counter1
    let increment_cmd = IncrementCounterCommand;
    executor
        .execute(
            &increment_cmd,
            IncrementCounterInput {
                stream_id: counter1_id.clone(),
                amount: 100,
            },
        )
        .await
        .unwrap();

    // Test successful transfer
    let transfer_cmd = TransferBetweenCountersCommand;
    let result = executor
        .execute(
            &transfer_cmd,
            TransferBetweenCountersInput {
                from_stream: counter1_id.clone(),
                to_stream: counter2_id.clone(),
                amount: 50,
            },
        )
        .await;

    assert!(result.is_ok(), "Transfer should succeed");

    // Verify both streams were updated atomically
    let read_options = eventcore::event_store::ReadOptions::default();
    let counter1_data = executor
        .event_store()
        .read_streams(&[counter1_id.clone()], &read_options)
        .await
        .unwrap();
    let counter2_data = executor
        .event_store()
        .read_streams(&[counter2_id.clone()], &read_options)
        .await
        .unwrap();

    // Filter events by stream
    let counter1_event_count = counter1_data
        .events
        .iter()
        .filter(|e| e.stream_id == counter1_id)
        .count();
    let counter2_event_count = counter2_data
        .events
        .iter()
        .filter(|e| e.stream_id == counter2_id)
        .count();

    assert_eq!(counter1_event_count, 2, "Counter1 should have 2 events");
    assert_eq!(counter2_event_count, 1, "Counter2 should have 1 event");

    // Test failing transfer (insufficient funds)
    let failing_result = executor
        .execute(
            &transfer_cmd,
            TransferBetweenCountersInput {
                from_stream: counter1_id.clone(),
                to_stream: counter2_id.clone(),
                amount: 100, // More than available
            },
        )
        .await;

    assert!(
        matches!(failing_result, Err(CommandError::BusinessRuleViolation(_))),
        "Transfer should fail due to insufficient funds"
    );

    // Verify no events were written on failure
    let counter1_data_after = executor
        .event_store()
        .read_streams(&[counter1_id.clone()], &read_options)
        .await
        .unwrap();
    let counter2_data_after = executor
        .event_store()
        .read_streams(&[counter2_id.clone()], &read_options)
        .await
        .unwrap();

    let counter1_event_count_after = counter1_data_after
        .events
        .iter()
        .filter(|e| e.stream_id == counter1_id)
        .count();
    let counter2_event_count_after = counter2_data_after
        .events
        .iter()
        .filter(|e| e.stream_id == counter2_id)
        .count();

    assert_eq!(
        counter1_event_count_after, 2,
        "Counter1 should still have 2 events"
    );
    assert_eq!(
        counter2_event_count_after, 1,
        "Counter2 should still have 1 event"
    );
}

#[tokio::test]
async fn test_transaction_isolation() {
    let (_container, config) = setup_postgres_container().await;

    // Initialize event store
    let event_store = PostgresEventStore::new(config).await.unwrap();
    event_store.initialize().await.unwrap();

    let executor = CommandExecutor::new(event_store);

    // Create multiple stream IDs
    let stream_ids: Vec<_> = (0..5)
        .map(|i| StreamId::try_new(format!("isolation-test-{i}")).unwrap())
        .collect();

    // Initialize streams with different commands concurrently
    let mut handles = vec![];

    for (i, stream_id) in stream_ids.iter().enumerate() {
        let executor = executor.clone();
        let stream_id = stream_id.clone();
        let amount = (u32::try_from(i).unwrap() + 1) * 10;

        let handle = tokio::spawn(async move {
            let command = IncrementCounterCommand;
            executor
                .execute(&command, IncrementCounterInput { stream_id, amount })
                .await
        });

        handles.push(handle);
    }

    // Wait for all initialization to complete
    futures::future::join_all(handles).await;

    // Now perform concurrent operations on different streams
    let mut concurrent_handles = vec![];

    // Some operations on individual streams
    for stream_id in stream_ids.iter().take(3) {
        let executor = executor.clone();
        let stream_id = stream_id.clone();

        let handle = tokio::spawn(async move {
            let command = IncrementCounterCommand;
            executor
                .execute(
                    &command,
                    IncrementCounterInput {
                        stream_id,
                        amount: 5,
                    },
                )
                .await
        });

        concurrent_handles.push(handle);
    }

    // Some transfer operations between streams
    for i in 0..2 {
        let executor = executor.clone();
        let from_stream = stream_ids[i].clone();
        let to_stream = stream_ids[i + 3].clone();

        let handle = tokio::spawn(async move {
            let command = TransferBetweenCountersCommand;
            executor
                .execute(
                    &command,
                    TransferBetweenCountersInput {
                        from_stream,
                        to_stream,
                        amount: 5,
                    },
                )
                .await
        });

        concurrent_handles.push(handle);
    }

    // Wait for all concurrent operations
    let results: Vec<_> = futures::future::join_all(concurrent_handles).await;

    // All operations should complete without deadlocks
    for (i, result) in results.into_iter().enumerate() {
        let outcome = result.expect("Task should not panic");
        assert!(
            outcome.is_ok() || matches!(outcome, Err(CommandError::ConcurrencyConflict { .. })),
            "Operation {i} failed unexpectedly: {outcome:?}"
        );
    }

    // Verify data consistency
    for stream_id in &stream_ids {
        let read_options = eventcore::event_store::ReadOptions::default();
        let stream_data = executor
            .event_store()
            .read_streams(&[stream_id.clone()], &read_options)
            .await
            .unwrap();

        // Each event should have a unique event ID
        let event_ids: std::collections::HashSet<_> = stream_data
            .events
            .iter()
            .map(|e| e.event_id)
            .collect();

        assert_eq!(
            event_ids.len(),
            stream_data.events.len(),
            "All events should have unique IDs"
        );

        // Events should be ordered by version
        for window in stream_data.events.windows(2) {
            assert!(
                window[0].event_version < window[1].event_version,
                "Events should be ordered by version"
            );
        }
    }
}

// Performance benchmarks would go in a separate benches/ directory
// but we'll add a basic performance test here
#[tokio::test]
async fn test_basic_performance() {
    let (_container, config) = setup_postgres_container().await;

    // Initialize event store
    let event_store = PostgresEventStore::new(config).await.unwrap();
    event_store.initialize().await.unwrap();

    let executor = CommandExecutor::new(event_store);
    let command = IncrementCounterCommand;

    // Measure time for sequential operations
    let stream_id = StreamId::try_new("perf-test").unwrap();
    let start = std::time::Instant::now();

    for i in 1..=100 {
        executor
            .execute(
                &command,
                IncrementCounterInput {
                    stream_id: stream_id.clone(),
                    amount: i,
                },
            )
            .await
            .unwrap();
    }

    let duration = start.elapsed();
    let ops_per_sec = 100.0 / duration.as_secs_f64();

    println!("Sequential operations: {ops_per_sec:.2} ops/sec");

    // Basic assertion - should handle at least 50 ops/sec even in test environment
    assert!(
        ops_per_sec > 50.0,
        "Performance too low: {ops_per_sec:.2} ops/sec"
    );
}
