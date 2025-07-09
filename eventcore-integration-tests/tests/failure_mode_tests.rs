//! Failure mode tests with controlled chaos injection.
//!
//! This module tests `EventCore`'s resilience under various failure conditions
//! using the chaos testing framework to inject controlled failures.

#![cfg(feature = "testing")]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::use_self)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::needless_collect)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::unnested_or_patterns)]

use async_trait::async_trait;
use eventcore::testing::chaos::{
    ChaosEventStore, ChaosScenarioBuilder, FailurePolicy, FailureType, TargetOperations,
};
use eventcore::{
    CommandError, CommandExecutor, CommandLogic, CommandStreams, EventStore, ExecutionOptions,
    ReadOptions, ReadStreams, RetryConfig, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Barrier;
use tracing::info;

/// Test events for failure mode testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum FailureTestEvent {
    Initialized {
        value: u64,
    },
    Updated {
        new_value: u64,
    },
    Transferred {
        from: String,
        to: String,
        amount: u64,
    },
}

impl TryFrom<&FailureTestEvent> for FailureTestEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &FailureTestEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// Simple state for testing.
#[derive(Debug, Default, Clone)]
struct TestState {
    value: u64,
}

/// Test command for failure scenarios.
#[derive(Debug, Clone)]
struct TestCommand {
    stream_id: StreamId,
    new_value: u64,
}

impl CommandStreams for TestCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.stream_id.clone()]
    }
}

#[async_trait]
impl CommandLogic for TestCommand {
    type State = TestState;
    type Event = FailureTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            FailureTestEvent::Initialized { value } => state.value = *value,
            FailureTestEvent::Updated { new_value } => state.value = *new_value,
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        Ok(vec![StreamWrite::new(
            &read_streams,
            self.stream_id.clone(),
            FailureTestEvent::Updated {
                new_value: self.new_value,
            },
        )?])
    }
}

/// Multi-stream transfer command for testing atomicity under failures.
#[derive(Debug, Clone)]
struct TransferCommand {
    from_stream: StreamId,
    to_stream: StreamId,
    amount: u64,
}

impl CommandStreams for TransferCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.from_stream.clone(), self.to_stream.clone()]
    }
}

#[async_trait]
impl CommandLogic for TransferCommand {
    type State = HashMap<StreamId, TestState>;
    type Event = FailureTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        let stream_state = state.entry(event.stream_id.clone()).or_default();
        match &event.payload {
            FailureTestEvent::Initialized { value } => stream_state.value = *value,
            FailureTestEvent::Updated { new_value } => stream_state.value = *new_value,
            FailureTestEvent::Transferred { .. } => {
                // This is simplified - in real scenarios we'd track debits/credits
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Check source has sufficient balance
        let from_balance = state.get(&self.from_stream).map(|s| s.value).unwrap_or(0);
        if from_balance < self.amount {
            return Err(CommandError::BusinessRuleViolation(
                "Insufficient balance".to_string(),
            ));
        }

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                self.from_stream.clone(),
                FailureTestEvent::Transferred {
                    from: self.from_stream.as_ref().to_string(),
                    to: self.to_stream.as_ref().to_string(),
                    amount: self.amount,
                },
            )?,
            StreamWrite::new(
                &read_streams,
                self.to_stream.clone(),
                FailureTestEvent::Transferred {
                    from: self.from_stream.as_ref().to_string(),
                    to: self.to_stream.as_ref().to_string(),
                    amount: self.amount,
                },
            )?,
        ])
    }
}

/// Test database connection failures and recovery.
#[tokio::test]
async fn test_database_connection_failures() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create chaos store with connection failure injection
    let chaos_store = ChaosScenarioBuilder::new(base_store, "Connection Failure Test")
        .with_connection_failures(0.3) // 30% failure rate
        .build();

    let executor = CommandExecutor::new(chaos_store.clone());

    // Track success and failure rates
    let success_count = Arc::new(AtomicU64::new(0));
    let failure_count = Arc::new(AtomicU64::new(0));
    let recovered_count = Arc::new(AtomicU64::new(0));

    // Run multiple operations to observe failure behavior
    let num_operations = 100;
    let mut handles = vec![];

    for i in 0..num_operations {
        let executor = executor.clone();
        let success = success_count.clone();
        let failure = failure_count.clone();
        let recovered = recovered_count.clone();

        handles.push(tokio::spawn(async move {
            let command = TestCommand {
                stream_id: StreamId::try_new(format!("test-stream-{}", i % 10)).unwrap(),
                new_value: i as u64,
            };

            // Try with retry to test recovery
            let retry_config = RetryConfig {
                max_attempts: 3,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                backoff_multiplier: 2.0,
            };

            let result = executor
                .execute(
                    command.clone(),
                    ExecutionOptions::default().with_retry_config(retry_config),
                )
                .await;

            match result {
                Ok(_) => {
                    success.fetch_add(1, Ordering::Relaxed);
                }
                Err(CommandError::EventStore(_)) => {
                    // Try once more to simulate recovery
                    let retry_result = executor.execute(command, ExecutionOptions::default()).await;

                    if retry_result.is_ok() {
                        recovered.fetch_add(1, Ordering::Relaxed);
                    } else {
                        failure.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    failure.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    // Wait for all operations to complete
    for handle in handles {
        handle.await.unwrap();
    }

    let successes = success_count.load(Ordering::Relaxed);
    let failures = failure_count.load(Ordering::Relaxed);
    let recoveries = recovered_count.load(Ordering::Relaxed);

    info!(
        "Connection failure test results: {} successes, {} failures, {} recoveries",
        successes, failures, recoveries
    );

    // Verify statistics
    let stats = chaos_store.stats();
    assert!(
        stats.total_operations >= num_operations,
        "Should have at least the base operations"
    );
    assert!(stats.failed_operations > 0, "Should have some failures");
    assert!(
        stats.failed_operations < stats.total_operations,
        "Not all operations should fail"
    );

    // With retries and recovery, we should have a reasonable success rate
    // With 30% failure rate and retries, we expect high success, but allow for variance
    assert!(
        successes + recoveries >= num_operations * 4 / 10,  // At least 40% success
        "Should recover from many failures with retry. Got {} successes + {} recoveries = {} total ({}%)",
        successes, recoveries, successes + recoveries,
        (successes + recoveries) * 100 / num_operations
    );
}

/// Test concurrent command execution under failure conditions.
#[tokio::test]
async fn test_concurrent_execution_with_failures() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create chaos store with mixed failure types
    let chaos_store = ChaosEventStore::new(base_store)
        .with_policy(FailurePolicy::random_errors(0.1, FailureType::Timeout))
        .with_policy(FailurePolicy::random_errors(
            0.05,
            FailureType::VersionConflict,
        ))
        .with_policy(FailurePolicy::latency_injection(
            Duration::from_millis(20),
            Some(Duration::from_millis(30)),
        ));

    let executor = Arc::new(CommandExecutor::new(chaos_store.clone()));

    // Initialize some streams (disable chaos for setup)
    chaos_store.set_enabled(false);
    let streams: Vec<StreamId> = (0..5)
        .map(|i| StreamId::try_new(format!("concurrent-stream-{}", i)).unwrap())
        .collect();

    for stream in &streams {
        executor
            .execute(
                TestCommand {
                    stream_id: stream.clone(),
                    new_value: 100,
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();
    }
    chaos_store.set_enabled(true);

    // Run concurrent operations
    let barrier = Arc::new(Barrier::new(20));
    let mut handles = vec![];

    for i in 0..20 {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let stream = streams[i % streams.len()].clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            let command = TestCommand {
                stream_id: stream,
                new_value: 100 + i as u64,
            };

            executor.execute(command, ExecutionOptions::default()).await
        }));
    }

    // Collect results
    let mut success_count = 0;
    let mut timeout_count = 0;
    let mut conflict_count = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => success_count += 1,
            Err(CommandError::EventStore(eventcore::EventStoreError::Timeout(_))) => {
                timeout_count += 1
            }
            Err(CommandError::ConcurrencyConflict { .. }) => conflict_count += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    info!(
        "Concurrent execution results: {} successes, {} timeouts, {} conflicts",
        success_count, timeout_count, conflict_count
    );

    // Verify some operations succeeded despite failures
    assert!(success_count > 0, "Some operations should succeed");
    assert_eq!(
        success_count + timeout_count + conflict_count,
        20,
        "All operations should be accounted for"
    );

    // Check chaos statistics
    let stats = chaos_store.stats();
    assert!(stats.delayed_operations > 0, "Should have injected latency");
    assert!(
        stats.average_latency_ms >= 20.0,
        "Average latency should reflect injected delays"
    );
}

/// Test event store timeout scenarios.
#[tokio::test]
async fn test_timeout_scenarios() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create store with aggressive timeout injection
    let chaos_store = ChaosEventStore::new(base_store)
        .with_policy(FailurePolicy::targeted(
            "Read timeouts",
            FailureType::Timeout,
            0.5, // 50% timeout rate
            TargetOperations::Reads,
        ))
        .with_policy(FailurePolicy::latency_injection(
            Duration::from_millis(500), // High latency to trigger real timeouts
            None,
        ));

    let executor = CommandExecutor::new(chaos_store.clone());

    // Test with timeout configuration
    let timeout_options =
        ExecutionOptions::default().with_command_timeout(Some(Duration::from_millis(100))); // Short timeout

    let stream_id = StreamId::try_new("timeout-test").unwrap();

    // First, write some data (disable chaos for setup)
    chaos_store.set_enabled(false);
    executor
        .execute(
            TestCommand {
                stream_id: stream_id.clone(),
                new_value: 42,
            },
            ExecutionOptions::default(),
        )
        .await
        .unwrap();
    chaos_store.set_enabled(true);

    // Now test reads with timeouts
    let mut timeout_count = 0;
    let mut success_count = 0;

    for i in 0..10 {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: 100 + i,
                },
                timeout_options.clone(),
            )
            .await;

        match result {
            Ok(_) => success_count += 1,
            Err(
                CommandError::Timeout(_)
                | CommandError::EventStore(eventcore::EventStoreError::Timeout(_)),
            ) => timeout_count += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    info!(
        "Timeout test results: {} successes, {} timeouts",
        success_count, timeout_count
    );

    assert!(timeout_count > 0, "Should have some timeouts");
    assert_eq!(
        success_count + timeout_count,
        10,
        "All operations accounted for"
    );
}

/// Test network partition scenarios.
#[tokio::test]
async fn test_network_partition_scenarios() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Set up partitioned streams
    let partition_a: Vec<StreamId> = (0..3)
        .map(|i| StreamId::try_new(format!("partition-a-{}", i)).unwrap())
        .collect();
    let partition_b: Vec<StreamId> = (0..3)
        .map(|i| StreamId::try_new(format!("partition-b-{}", i)).unwrap())
        .collect();

    // Create chaos store simulating network partition
    let chaos_store = ChaosScenarioBuilder::new(base_store, "Network Partition")
        .with_partition(partition_a.clone())
        .build();

    let executor = CommandExecutor::new(chaos_store.clone());

    // Initialize all streams
    for stream in partition_a.iter().chain(partition_b.iter()) {
        let _ = executor
            .execute(
                TestCommand {
                    stream_id: stream.clone(),
                    new_value: 1000,
                },
                ExecutionOptions::default(),
            )
            .await;
    }

    // Test operations on both partitions
    let mut partition_a_failures = 0;
    let mut partition_b_successes = 0;

    // Operations on partition A (should fail)
    for stream in &partition_a {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream.clone(),
                    new_value: 2000,
                },
                ExecutionOptions::default(),
            )
            .await;

        if result.is_err() {
            partition_a_failures += 1;
        }
    }

    // Operations on partition B (should succeed)
    for stream in &partition_b {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream.clone(),
                    new_value: 2000,
                },
                ExecutionOptions::default(),
            )
            .await;

        if result.is_ok() {
            partition_b_successes += 1;
        }
    }

    info!(
        "Partition test: {} failures in partition A, {} successes in partition B",
        partition_a_failures, partition_b_successes
    );

    assert_eq!(
        partition_a_failures, 3,
        "All operations in partitioned streams should fail"
    );
    assert_eq!(
        partition_b_successes, 3,
        "All operations in non-partitioned streams should succeed"
    );
}

/// Test multi-stream atomicity under failure conditions.
#[tokio::test]
async fn test_multi_stream_atomicity_with_failures() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create chaos store with intermittent failures
    let chaos_store = ChaosEventStore::new(base_store).with_policy(FailurePolicy::random_errors(
        0.2,
        FailureType::TransactionRollback,
    ));

    let executor = CommandExecutor::new(chaos_store.clone());

    // Initialize accounts (disable chaos for setup)
    chaos_store.set_enabled(false);
    let account_a = StreamId::try_new("account-a").unwrap();
    let account_b = StreamId::try_new("account-b").unwrap();

    for account in &[&account_a, &account_b] {
        executor
            .execute(
                TestCommand {
                    stream_id: (*account).clone(),
                    new_value: 1000,
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();
    }
    chaos_store.set_enabled(true);

    // Perform multiple transfers
    let mut successful_transfers = 0;
    let mut failed_transfers = 0;

    for _ in 0..20 {
        let result = executor
            .execute(
                TransferCommand {
                    from_stream: account_a.clone(),
                    to_stream: account_b.clone(),
                    amount: 10,
                },
                ExecutionOptions::default(),
            )
            .await;

        match result {
            Ok(_) => successful_transfers += 1,
            Err(_) => failed_transfers += 1,
        }
    }

    info!(
        "Transfer atomicity test: {} successful, {} failed",
        successful_transfers, failed_transfers
    );

    // Verify atomicity - both streams should have consistent state
    // Disable chaos for verification
    chaos_store.set_enabled(false);
    let read_options = ReadOptions::default();
    let streams_data = executor
        .event_store()
        .read_streams(&[account_a, account_b], &read_options)
        .await
        .unwrap();

    // Count transfer events - they should appear in pairs or not at all
    let transfer_events: Vec<_> = streams_data
        .events()
        .filter(|e| matches!(e.payload, FailureTestEvent::Transferred { .. }))
        .collect();

    // Each successful transfer creates 2 events (one per stream)
    assert_eq!(
        transfer_events.len(),
        successful_transfers * 2,
        "Transfer events should be atomic"
    );
}

/// Test cascading failures and circuit breaker behavior.
#[tokio::test]
async fn test_cascading_failure_prevention() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create chaos store that simulates degrading service
    let chaos_store = ChaosEventStore::new(base_store);

    // We'll manually control failure injection
    chaos_store.set_enabled(false);

    let executor = CommandExecutor::new(chaos_store.clone());
    let stream_id = StreamId::try_new("cascade-test").unwrap();

    // Phase 1: Healthy operations
    for i in 0..10 {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: i,
                },
                ExecutionOptions::default(),
            )
            .await;
        assert!(result.is_ok(), "Healthy phase should succeed");
    }

    // Phase 2: Enable failures to simulate degradation
    chaos_store.set_enabled(true);
    let chaos_store_clone = chaos_store.clone();
    let chaos_store_injected =
        chaos_store_clone.with_policy(FailurePolicy::random_errors(0.8, FailureType::Unavailable));

    let executor_with_failures = CommandExecutor::new(chaos_store_injected);

    let mut phase2_failures = 0;
    for i in 10..20 {
        let result = executor_with_failures
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: i,
                },
                ExecutionOptions::default(),
            )
            .await;
        if result.is_err() {
            phase2_failures += 1;
        }
    }

    info!("Phase 2 (degraded): {} failures out of 10", phase2_failures);
    assert!(
        phase2_failures > 5,
        "Should have significant failures in degraded phase"
    );

    // Phase 3: Disable chaos to simulate recovery
    chaos_store.set_enabled(false);
    chaos_store.reset_stats();

    for i in 20..30 {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: i,
                },
                ExecutionOptions::default(),
            )
            .await;
        assert!(result.is_ok(), "Recovery phase should succeed");
    }

    // Verify final stats
    let final_stats = chaos_store.stats();
    assert_eq!(
        final_stats.failed_operations, 0,
        "No failures after recovery"
    );
}

/// Test behavior under sustained high-latency conditions.
#[tokio::test]
async fn test_high_latency_degradation() {
    let base_store = InMemoryEventStore::<FailureTestEvent>::new();

    // Create store with progressive latency injection
    let chaos_store =
        ChaosEventStore::new(base_store).with_policy(FailurePolicy::latency_injection(
            Duration::from_millis(50),
            Some(Duration::from_millis(100)), // High jitter
        ));

    let executor = CommandExecutor::new(chaos_store.clone());

    // Measure operation latencies
    let mut latencies = vec![];
    let stream_id = StreamId::try_new("latency-test").unwrap();

    for i in 0..10 {
        let start = std::time::Instant::now();
        let _ = executor
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: i,
                },
                ExecutionOptions::default(),
            )
            .await;
        let duration = start.elapsed();
        latencies.push(duration);
    }

    // Verify latency injection
    let avg_latency =
        latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    info!("Average operation latency: {}ms", avg_latency);

    assert!(
        avg_latency >= 50,
        "Average latency should reflect injection"
    );

    // Check chaos stats
    let stats = chaos_store.stats();
    assert!(
        stats.delayed_operations >= 10,
        "At least all our operations should be delayed"
    );
    assert!(
        stats.average_latency_ms >= 50.0,
        "Stats should reflect latency injection"
    );
}

/// Integration test with PostgreSQL if available.
#[tokio::test]
#[ignore = "Requires PostgreSQL"]
async fn test_postgres_failure_scenarios() {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string()
    });

    let postgres_store =
        PostgresEventStore::<FailureTestEvent>::new(PostgresConfig::new(database_url))
            .await
            .expect("Failed to connect to PostgreSQL");

    postgres_store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    // Wrap with chaos injection
    let chaos_store = ChaosScenarioBuilder::new(postgres_store, "PostgreSQL Chaos Test")
        .with_connection_failures(0.1)
        .with_timeouts(0.1)
        .with_latency(Duration::from_millis(20), Some(Duration::from_millis(10)))
        .build();

    let executor = CommandExecutor::new(chaos_store);

    // Run test scenario
    let stream_id = StreamId::try_new("postgres-chaos-test").unwrap();
    let mut successes = 0;
    let mut failures = 0;

    for i in 0..50 {
        let result = executor
            .execute(
                TestCommand {
                    stream_id: stream_id.clone(),
                    new_value: i,
                },
                ExecutionOptions::default().with_retry_config(RetryConfig {
                    max_attempts: 3,
                    base_delay: Duration::from_millis(50),
                    max_delay: Duration::from_millis(500),
                    backoff_multiplier: 2.0,
                }),
            )
            .await;

        match result {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    info!(
        "PostgreSQL chaos test: {} successes, {} failures ({}% success rate)",
        successes,
        failures,
        (successes * 100) / (successes + failures)
    );

    // With retries, we should maintain a high success rate
    assert!(
        successes > 40,
        "Should maintain >80% success rate with retries"
    );
}
