//! Rollback behavior verification tests.
//!
//! This module tests `EventCore`'s transaction rollback behavior to ensure
//! data consistency and proper error handling when operations fail.

#![allow(clippy::similar_names)] // Test code often uses similar names for test data
#![allow(clippy::doc_markdown)] // Not necessary for test documentation
#![allow(clippy::uninlined_format_args)] // Format args are clearer inline for tests

use async_trait::async_trait;
#[cfg(feature = "testing")]
use eventcore::testing::chaos::{
    ChaosScenarioBuilder, FailurePolicy, FailureType, TargetOperations,
};
use eventcore::{
    CommandError, CommandExecutor, EventId, EventMetadata, EventStore, EventToWrite,
    ExecutionOptions, ExpectedVersion, ReadOptions, ReadStreams, RetryConfig, StreamEvents,
    StreamId, StreamResolver, StreamWrite,
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
use tracing::info;

/// Event type for rollback tests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum RollbackTestEvent {
    AccountCreated {
        balance: u64,
    },
    MoneyTransferred {
        amount: u64,
        from: String,
        to: String,
    },
    BalanceUpdated {
        new_balance: u64,
    },
}

impl TryFrom<&Self> for RollbackTestEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// State for tracking account balances.
#[derive(Debug, Default, Clone)]
struct AccountState {
    balances: HashMap<StreamId, u64>,
}

/// Multi-stream transfer command that should be atomic.
#[derive(Debug, Clone)]
struct AtomicTransferCommand {
    from_account: StreamId,
    to_account: StreamId,
    amount: u64,
}

impl eventcore::CommandStreams for AtomicTransferCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.from_account.clone(), self.to_account.clone()]
    }
}

#[async_trait]
impl eventcore::CommandLogic for AtomicTransferCommand {
    type State = AccountState;
    type Event = RollbackTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            RollbackTestEvent::AccountCreated { balance } => {
                state.balances.insert(event.stream_id.clone(), *balance);
            }
            RollbackTestEvent::BalanceUpdated { new_balance } => {
                state.balances.insert(event.stream_id.clone(), *new_balance);
            }
            RollbackTestEvent::MoneyTransferred { .. } => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Check if source account has sufficient balance
        let from_balance = state.balances.get(&self.from_account).copied().unwrap_or(0);
        if from_balance < self.amount {
            return Err(CommandError::ValidationFailed(
                "Insufficient balance".to_string(),
            ));
        }

        let to_balance = state.balances.get(&self.to_account).copied().unwrap_or(0);

        // Create events for both accounts
        let events = vec![
            StreamWrite::new(
                &read_streams,
                self.from_account.clone(),
                RollbackTestEvent::BalanceUpdated {
                    new_balance: from_balance - self.amount,
                },
            )?,
            StreamWrite::new(
                &read_streams,
                self.to_account.clone(),
                RollbackTestEvent::BalanceUpdated {
                    new_balance: to_balance + self.amount,
                },
            )?,
        ];

        Ok(events)
    }
}

/// Test that partial write failures result in complete rollback.
#[tokio::test]
#[cfg(feature = "testing")]
async fn test_partial_write_failure_rollback() {
    let base_store = InMemoryEventStore::<RollbackTestEvent>::new();

    // Set up initial accounts
    let account_a = StreamId::try_new("account-a").unwrap();
    let account_b = StreamId::try_new("account-b").unwrap();

    // Initialize accounts
    let metadata = EventMetadata::new();
    base_store
        .write_events_multi(vec![
            StreamEvents {
                stream_id: account_a.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 1000 },
                    metadata.clone(),
                )],
            },
            StreamEvents {
                stream_id: account_b.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 500 },
                    metadata.clone(),
                )],
            },
        ])
        .await
        .unwrap();

    // Create chaos store that fails on second stream write
    let failure_count = Arc::new(AtomicU64::new(0));
    let failure_count_clone = failure_count.clone();

    let chaos_store = ChaosScenarioBuilder::new(base_store.clone(), "Partial Write Failure")
        .with_policy(FailurePolicy::targeted(
            "Fail first multi-stream write",
            FailureType::TransactionRollback,
            1.0, // 100% probability
            TargetOperations::Custom {
                name: "Multi-stream write".to_string(),
                predicate: Arc::new(move |op| {
                    match op {
                        eventcore::testing::chaos::Operation::Write { stream_ids } => {
                            if stream_ids.len() > 1 {
                                let count = failure_count_clone.fetch_add(1, Ordering::SeqCst);
                                // Fail on the first multi-stream write attempt
                                count == 0
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                }),
            },
        ))
        .build();

    let executor = CommandExecutor::new(chaos_store);

    // Attempt transfer that should fail and rollback
    let command = AtomicTransferCommand {
        from_account: account_a.clone(),
        to_account: account_b.clone(),
        amount: 300,
    };
    let result = executor
        .execute(
            command,
            ExecutionOptions::default().with_retry_config(RetryConfig {
                max_attempts: 1, // No retry to test rollback
                ..Default::default()
            }),
        )
        .await;

    assert!(result.is_err(), "Transfer should fail");

    // Verify that balances remain unchanged (rollback occurred)
    let account_a_events = base_store
        .read_streams(&[account_a.clone()], &ReadOptions::default())
        .await
        .unwrap();

    let account_b_events = base_store
        .read_streams(&[account_b.clone()], &ReadOptions::default())
        .await
        .unwrap();

    // Should only have initial events, no balance updates
    assert_eq!(account_a_events.events_for_stream(&account_a).count(), 1);
    assert_eq!(account_b_events.events_for_stream(&account_b).count(), 1);
}

/// Test version conflict detection and rollback.
#[tokio::test]
async fn test_version_conflict_rollback() {
    // This test verifies that concurrent modifications are handled correctly.
    // Since the command reads all streams first, we need to test the scenario
    // where a concurrent write happens between read and write phases.

    // For now, we'll test that the system handles concurrent modifications
    // gracefully by re-reading and retrying when there's a conflict.
    let store = InMemoryEventStore::<RollbackTestEvent>::new();
    let executor = CommandExecutor::new(store.clone());

    // Set up initial accounts
    let account_a = StreamId::try_new("account-conflict-a").unwrap();
    let account_b = StreamId::try_new("account-conflict-b").unwrap();

    // Initialize accounts
    let metadata = EventMetadata::new();
    store
        .write_events_multi(vec![
            StreamEvents {
                stream_id: account_a.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 1000 },
                    metadata.clone(),
                )],
            },
            StreamEvents {
                stream_id: account_b.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 500 },
                    metadata.clone(),
                )],
            },
        ])
        .await
        .unwrap();

    // Simulate concurrent transfers happening in parallel
    let executor1 = executor.clone();
    let executor2 = executor.clone();
    let account_a1 = account_a.clone();
    let account_a2 = account_a.clone();
    let account_b1 = account_b.clone();
    let account_b2 = account_b.clone();

    // Launch two concurrent transfers
    let handle1 = tokio::spawn(async move {
        executor1
            .execute(
                AtomicTransferCommand {
                    from_account: account_a1,
                    to_account: account_b1,
                    amount: 300,
                },
                ExecutionOptions::default(),
            )
            .await
    });

    let handle2 = tokio::spawn(async move {
        executor2
            .execute(
                AtomicTransferCommand {
                    from_account: account_a2,
                    to_account: account_b2,
                    amount: 200,
                },
                ExecutionOptions::default(),
            )
            .await
    });

    // Wait for both operations
    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    // Both should eventually succeed (one might retry due to conflict)
    assert!(result1.is_ok(), "First transfer should succeed");
    assert!(result2.is_ok(), "Second transfer should succeed");

    // Verify final state is consistent
    let final_events = store
        .read_streams(
            &[account_a.clone(), account_b.clone()],
            &ReadOptions::default(),
        )
        .await
        .unwrap();

    // Calculate final balances
    let mut balance_a = 0u64;
    let mut balance_b = 0u64;

    for event in final_events.events_for_stream(&account_a) {
        match &event.payload {
            RollbackTestEvent::AccountCreated { balance } => balance_a = *balance,
            RollbackTestEvent::BalanceUpdated { new_balance } => balance_a = *new_balance,
            RollbackTestEvent::MoneyTransferred { .. } => {}
        }
    }

    for event in final_events.events_for_stream(&account_b) {
        match &event.payload {
            RollbackTestEvent::AccountCreated { balance } => balance_b = *balance,
            RollbackTestEvent::BalanceUpdated { new_balance } => balance_b = *new_balance,
            RollbackTestEvent::MoneyTransferred { .. } => {}
        }
    }

    // Total should be conserved (1000 + 500 = 1500)
    assert_eq!(
        balance_a + balance_b,
        1500,
        "Total balance should be conserved after concurrent transfers"
    );

    // Account A should have had 500 deducted (300 + 200)
    assert_eq!(balance_a, 500, "Account A should have 500 remaining");
    assert_eq!(balance_b, 1000, "Account B should have 1000");
}

/// Test state consistency after rollback.
#[tokio::test]
#[cfg(feature = "testing")]
async fn test_state_consistency_after_rollback() {
    let store = InMemoryEventStore::<RollbackTestEvent>::new();

    // Create chaos store with intermittent failures
    let chaos_store = ChaosScenarioBuilder::new(store.clone(), "State Consistency Test")
        .with_connection_failures(0.3) // 30% failure rate
        .build();

    let executor = CommandExecutor::new(chaos_store);

    // Set up multiple accounts
    let accounts: Vec<_> = (0..5)
        .map(|i| StreamId::try_new(format!("consistency-account-{}", i)).unwrap())
        .collect();

    // Initialize all accounts
    let metadata = EventMetadata::new();
    let init_events: Vec<_> = accounts
        .iter()
        .map(|account| StreamEvents {
            stream_id: account.clone(),
            expected_version: ExpectedVersion::New,
            events: vec![EventToWrite::with_metadata(
                EventId::new(),
                RollbackTestEvent::AccountCreated { balance: 1000 },
                metadata.clone(),
            )],
        })
        .collect();

    store.write_events_multi(init_events).await.unwrap();

    // Perform multiple concurrent transfers with potential failures
    let mut handles = vec![];
    let success_count = Arc::new(AtomicU64::new(0));
    let failure_count = Arc::new(AtomicU64::new(0));

    for i in 0..20 {
        let executor = executor.clone();
        let from = accounts[i % accounts.len()].clone();
        let to = accounts[(i + 1) % accounts.len()].clone();
        let success = success_count.clone();
        let failure = failure_count.clone();

        handles.push(tokio::spawn(async move {
            let result = executor
                .execute(
                    AtomicTransferCommand {
                        from_account: from,
                        to_account: to,
                        amount: 10,
                    },
                    ExecutionOptions::default(),
                )
                .await;

            match result {
                Ok(_) => {
                    success.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    failure.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    // Wait for all operations
    for handle in handles {
        handle.await.unwrap();
    }

    let successes = success_count.load(Ordering::Relaxed);
    let failures = failure_count.load(Ordering::Relaxed);

    info!(
        "State consistency test: {} successes, {} failures",
        successes, failures
    );

    // Verify total balance remains constant (conservation of money)
    let mut total_balance = 0u64;
    for account in &accounts {
        let events = store
            .read_streams(&[account.clone()], &ReadOptions::default())
            .await
            .unwrap();

        let mut balance = 0u64;
        for event in events.events_for_stream(account) {
            match &event.payload {
                RollbackTestEvent::AccountCreated { balance: b } => balance = *b,
                RollbackTestEvent::BalanceUpdated { new_balance } => balance = *new_balance,
                RollbackTestEvent::MoneyTransferred { .. } => {}
            }
        }
        total_balance += balance;
    }

    assert_eq!(
        total_balance,
        accounts.len() as u64 * 1000,
        "Total balance should be conserved despite failures and rollbacks"
    );
}

/// Test retry behavior after transaction rollback.
#[tokio::test]
#[allow(clippy::too_many_lines)]
#[cfg(feature = "testing")]
async fn test_retry_after_rollback() {
    let base_store = InMemoryEventStore::<RollbackTestEvent>::new();

    // Create chaos store that fails first two attempts with decreasing probability
    let attempt_count = Arc::new(AtomicU64::new(0));
    let attempt_count_clone = attempt_count.clone();

    let chaos_store = ChaosScenarioBuilder::new(base_store.clone(), "Retry After Rollback")
        .with_policy(FailurePolicy::targeted(
            "Fail with decreasing probability",
            FailureType::ConnectionFailure, // Use connection failure which should trigger retry
            0.8, // 80% probability (will decrease based on attempt count)
            TargetOperations::Custom {
                name: "Fail based on attempt count".to_string(),
                predicate: Arc::new(move |op| {
                    match op {
                        eventcore::testing::chaos::Operation::Write { stream_ids } => {
                            // Only affect multi-stream writes (transfers)
                            if stream_ids.len() > 1 {
                                let count = attempt_count_clone.load(Ordering::SeqCst);
                                if count < 2 {
                                    // Increment for next time
                                    attempt_count_clone.fetch_add(1, Ordering::SeqCst);
                                    true // Fail first two attempts
                                } else {
                                    false // Succeed on third attempt
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                }),
            },
        ))
        .build();

    let executor = CommandExecutor::new(chaos_store);

    // Set up accounts
    let account_a = StreamId::try_new("retry-account-a").unwrap();
    let account_b = StreamId::try_new("retry-account-b").unwrap();

    let metadata = EventMetadata::new();
    base_store
        .write_events_multi(vec![
            StreamEvents {
                stream_id: account_a.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 1000 },
                    metadata.clone(),
                )],
            },
            StreamEvents {
                stream_id: account_b.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 500 },
                    metadata.clone(),
                )],
            },
        ])
        .await
        .unwrap();

    // Manual retry loop to demonstrate rollback behavior
    let mut last_error = None;
    let mut manual_attempts = 0;

    for i in 0..3 {
        manual_attempts += 1;
        let result = executor
            .execute(
                AtomicTransferCommand {
                    from_account: account_a.clone(),
                    to_account: account_b.clone(),
                    amount: 300,
                },
                ExecutionOptions::default(),
            )
            .await;

        match result {
            Ok(_) => {
                eprintln!("Transfer succeeded on attempt {}", i + 1);
                break;
            }
            Err(e) => {
                eprintln!("Transfer failed on attempt {}: {:?}", i + 1, e);
                last_error = Some(e);
                if i < 2 {
                    // Add small delay before retry
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }

    eprintln!("Total manual attempts: {}", manual_attempts);
    eprintln!(
        "Chaos failure count: {}",
        attempt_count.load(Ordering::Relaxed)
    );

    // Verify the transfer eventually succeeded
    let final_events = base_store
        .read_streams(
            &[account_a.clone(), account_b.clone()],
            &ReadOptions::default(),
        )
        .await
        .unwrap();

    // Should have balance updates if transfer succeeded
    let account_a_count = final_events.events_for_stream(&account_a).count();
    let account_b_count = final_events.events_for_stream(&account_b).count();

    if account_a_count == 2 && account_b_count == 2 {
        // Transfer succeeded - verify balances
        let account_a_vec: Vec<_> = final_events.events_for_stream(&account_a).collect();
        if let RollbackTestEvent::BalanceUpdated { new_balance } = &account_a_vec[1].payload {
            assert_eq!(*new_balance, 700);
        }

        let account_b_vec: Vec<_> = final_events.events_for_stream(&account_b).collect();
        if let RollbackTestEvent::BalanceUpdated { new_balance } = &account_b_vec[1].payload {
            assert_eq!(*new_balance, 800);
        }
    } else {
        // All attempts failed
        panic!(
            "Transfer failed after {} attempts. Last error: {:?}",
            manual_attempts, last_error
        );
    }
}

/// Test PostgreSQL-specific rollback behavior.
#[tokio::test]
#[ignore = "Requires PostgreSQL"]
async fn test_postgres_transaction_rollback() {
    let config = std::env::var("DATABASE_URL").map_or_else(
        |_| PostgresConfig::new("postgres://postgres:postgres@localhost:5433/eventcore_test"),
        PostgresConfig::new,
    );

    let postgres_store = PostgresEventStore::<RollbackTestEvent>::new(config)
        .await
        .expect("Failed to create PostgreSQL store");

    // Clear any existing test data
    let test_accounts = vec![
        StreamId::try_new("pg-rollback-a").unwrap(),
        StreamId::try_new("pg-rollback-b").unwrap(),
    ];

    // Initialize accounts
    let metadata = EventMetadata::new();
    let _ = postgres_store
        .write_events_multi(vec![
            StreamEvents {
                stream_id: test_accounts[0].clone(),
                expected_version: ExpectedVersion::Any,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 2000 },
                    metadata.clone(),
                )],
            },
            StreamEvents {
                stream_id: test_accounts[1].clone(),
                expected_version: ExpectedVersion::Any,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 1000 },
                    metadata.clone(),
                )],
            },
        ])
        .await;

    // Create executor
    let executor = CommandExecutor::new(postgres_store.clone());

    // Simulate concurrent modification
    let handle = tokio::spawn({
        let store = postgres_store.clone();
        let account = test_accounts[1].clone();
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let metadata = EventMetadata::new();
            let _ = store
                .write_events_multi(vec![StreamEvents {
                    stream_id: account,
                    expected_version: ExpectedVersion::Any,
                    events: vec![EventToWrite::with_metadata(
                        EventId::new(),
                        RollbackTestEvent::BalanceUpdated { new_balance: 1100 },
                        metadata,
                    )],
                }])
                .await;
        }
    });

    // Attempt transfer that might conflict
    let result = executor
        .execute(
            AtomicTransferCommand {
                from_account: test_accounts[0].clone(),
                to_account: test_accounts[1].clone(),
                amount: 500,
            },
            ExecutionOptions::default(),
        )
        .await;

    handle.await.unwrap();

    // Check final state
    let final_events = postgres_store
        .read_streams(&test_accounts, &ReadOptions::default())
        .await
        .unwrap();

    // Verify that either the transfer succeeded or was properly rolled back
    let account_a_event_count = final_events.events_for_stream(&test_accounts[0]).count();
    let account_b_event_count = final_events.events_for_stream(&test_accounts[1]).count();

    if result.is_ok() {
        // Transfer succeeded - both accounts should have updates
        assert!(account_a_event_count >= 2);
        assert!(account_b_event_count >= 2);
    } else {
        // Transfer failed - verify proper rollback
        info!("Transfer failed as expected due to conflict: {:?}", result);

        // Calculate total balance to ensure consistency
        let mut total = 0u64;
        for stream_id in &test_accounts {
            let mut stream_balance = 0u64;
            for event in final_events.events_for_stream(stream_id) {
                match &event.payload {
                    RollbackTestEvent::AccountCreated { balance } => stream_balance = *balance,
                    RollbackTestEvent::BalanceUpdated { new_balance } => {
                        stream_balance = *new_balance;
                    }
                    RollbackTestEvent::MoneyTransferred { .. } => {}
                }
            }
            total += stream_balance;
        }

        // Total should still be 3000 regardless of outcome
        assert_eq!(total, 3000, "Total balance should be conserved");
    }
}

/// Test complex nested operation rollback.
#[tokio::test]
async fn test_nested_operation_rollback() {
    // This tests a scenario where multiple commands are executed in sequence
    // and a failure in a later command should not affect earlier successful commands
    let store = InMemoryEventStore::<RollbackTestEvent>::new();
    let executor = CommandExecutor::new(store.clone());

    // Create a chain of accounts
    let accounts: Vec<_> = (0..4)
        .map(|i| StreamId::try_new(format!("nested-account-{}", i)).unwrap())
        .collect();

    // Initialize all accounts
    let metadata = EventMetadata::new();
    for account in &accounts {
        store
            .write_events_multi(vec![StreamEvents {
                stream_id: account.clone(),
                expected_version: ExpectedVersion::New,
                events: vec![EventToWrite::with_metadata(
                    EventId::new(),
                    RollbackTestEvent::AccountCreated { balance: 1000 },
                    metadata.clone(),
                )],
            }])
            .await
            .unwrap();
    }

    // Execute a series of transfers
    let transfers = vec![
        (0, 1, 100),  // A -> B: 100
        (1, 2, 50),   // B -> C: 50
        (2, 3, 200),  // C -> D: 200 (should succeed)
        (3, 0, 2000), // D -> A: 2000 (should fail - insufficient funds)
    ];

    let mut successful_transfers = 0;

    for (from_idx, to_idx, amount) in transfers {
        let command = AtomicTransferCommand {
            from_account: accounts[from_idx].clone(),
            to_account: accounts[to_idx].clone(),
            amount,
        };
        let result = executor.execute(command, ExecutionOptions::default()).await;

        if result.is_ok() {
            successful_transfers += 1;
        } else {
            info!(
                "Transfer {} -> {} of {} failed as expected",
                from_idx, to_idx, amount
            );
            break; // Stop on first failure
        }
    }

    assert_eq!(
        successful_transfers, 3,
        "First three transfers should succeed"
    );

    // Verify final balances
    let expected_balances = [
        900,  // Account 0: 1000 - 100
        1050, // Account 1: 1000 - 100 + 50
        850,  // Account 2: 1000 + 50 - 200
        1200, // Account 3: 1000 + 200
    ];

    for (idx, expected) in expected_balances.iter().enumerate() {
        let events = store
            .read_streams(&[accounts[idx].clone()], &ReadOptions::default())
            .await
            .unwrap();

        let mut balance = 0u64;
        for event in events.events_for_stream(&accounts[idx]) {
            match &event.payload {
                RollbackTestEvent::AccountCreated { balance: b } => balance = *b,
                RollbackTestEvent::BalanceUpdated { new_balance } => balance = *new_balance,
                RollbackTestEvent::MoneyTransferred { .. } => {}
            }
        }

        assert_eq!(
            balance, *expected,
            "Account {} should have balance {}",
            idx, expected
        );
    }
}
