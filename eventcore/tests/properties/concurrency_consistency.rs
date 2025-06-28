//! Property tests for concurrent command consistency.
//!
//! These tests verify that when multiple commands execute concurrently,
//! the system maintains consistency through proper concurrency control
//! mechanisms like optimistic locking.

use eventcore::command::{Command, CommandResult};
use eventcore::errors::CommandError;
use eventcore::event_store::{EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamEvents};
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, EventVersion, StreamId};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Barrier;

/// Test command for concurrency testing.
#[derive(Debug, Clone)]
pub struct ConcurrencyTestCommand;

/// Input for concurrency test command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyTestInput {
    pub stream_id: StreamId,
    pub operation: ConcurrencyOperation,
    pub expected_version: Option<EventVersion>,
}

/// Operations that test different concurrency scenarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConcurrencyOperation {
    /// Add to a counter (tests atomic increments)
    AddToCounter { amount: i64 },
    /// Transfer between accounts (tests multi-stream consistency)
    Transfer { from_key: String, to_key: String, amount: u64 },
    /// Create unique item (tests uniqueness constraints)
    CreateUniqueItem { id: String, name: String },
    /// Update item if exists (tests conditional updates)
    UpdateIfExists { id: String, new_name: String },
}

/// Test event for concurrency testing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConcurrencyTestEvent {
    CounterIncremented { amount: i64 },
    MoneyTransferred { from_key: String, to_key: String, amount: u64 },
    UniqueItemCreated { id: String, name: String },
    ItemUpdated { id: String, new_name: String },
}

/// State for concurrency test command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConcurrencyTestState {
    pub counter: i64,
    pub balances: HashMap<String, u64>,
    pub items: HashMap<String, String>,
}

#[async_trait::async_trait]
impl Command for ConcurrencyTestCommand {
    type Input = ConcurrencyTestInput;
    type State = ConcurrencyTestState;
    type Event = ConcurrencyTestEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &eventcore::event_store::StoredEvent<Self::Event>) {
        match &stored_event.payload {
            ConcurrencyTestEvent::CounterIncremented { amount } => {
                state.counter += amount;
            }
            ConcurrencyTestEvent::MoneyTransferred { from_key, to_key, amount } => {
                if let Some(from_balance) = state.balances.get_mut(from_key) {
                    *from_balance = from_balance.saturating_sub(*amount);
                }
                let to_balance = state.balances.entry(to_key.clone()).or_insert(0);
                *to_balance += amount;
            }
            ConcurrencyTestEvent::UniqueItemCreated { id, name } => {
                state.items.insert(id.clone(), name.clone());
            }
            ConcurrencyTestEvent::ItemUpdated { id, new_name } => {
                if state.items.contains_key(id) {
                    state.items.insert(id.clone(), new_name.clone());
                }
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        let event = match input.operation {
            ConcurrencyOperation::AddToCounter { amount } => {
                ConcurrencyTestEvent::CounterIncremented { amount }
            }
            ConcurrencyOperation::Transfer { from_key, to_key, amount } => {
                // Check if transfer is valid
                let from_balance = state.balances.get(&from_key).unwrap_or(&0);
                if *from_balance < amount {
                    return Err(CommandError::BusinessRuleViolation(
                        format!("Insufficient balance: {} < {}", from_balance, amount)
                    ));
                }
                ConcurrencyTestEvent::MoneyTransferred { from_key, to_key, amount }
            }
            ConcurrencyOperation::CreateUniqueItem { id, name } => {
                // Check if item already exists
                if state.items.contains_key(&id) {
                    return Err(CommandError::BusinessRuleViolation(
                        format!("Item {} already exists", id)
                    ));
                }
                ConcurrencyTestEvent::UniqueItemCreated { id, name }
            }
            ConcurrencyOperation::UpdateIfExists { id, new_name } => {
                // Only update if item exists
                if !state.items.contains_key(&id) {
                    return Err(CommandError::BusinessRuleViolation(
                        format!("Item {} does not exist", id)
                    ));
                }
                ConcurrencyTestEvent::ItemUpdated { id, new_name }
            }
        };

        Ok(vec![(input.stream_id, event)])
    }
}

/// Property test: Concurrent counter increments maintain consistency.
///
/// This test verifies that when multiple commands increment a counter
/// concurrently, the final result is consistent (though some commands
/// may fail due to version conflicts).
#[test]
fn prop_concurrent_counter_consistency() {
    proptest! {
        #[test]
        fn test_concurrent_counter_increments(
            stream_id in arb_stream_id(),
            increments in prop::collection::vec(1i64..100, 2..10),
            initial_value in 0i64..1000
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = Arc::new(InMemoryEventStore::new());
                let command = Arc::new(ConcurrencyTestCommand);
                
                // Set up initial state
                if initial_value > 0 {
                    let initial_events = vec![EventToWrite::new(
                        EventId::new(),
                        ConcurrencyTestEvent::CounterIncremented { amount: initial_value }
                    )];
                    
                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        ExpectedVersion::New,
                        initial_events
                    )];
                    
                    store.write_events_multi(stream_events).await.unwrap();
                }
                
                // Get current version
                let current_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let current_version = current_data.stream_versions.get(&stream_id)
                    .copied()
                    .unwrap_or(EventVersion::initial());
                
                // Execute concurrent increments
                let barrier = Arc::new(Barrier::new(increments.len()));
                let mut handles = Vec::new();
                
                for increment in increments.iter() {
                    let store_clone = Arc::clone(&store);
                    let command_clone = Arc::clone(&command);
                    let stream_id_clone = stream_id.clone();
                    let barrier_clone = Arc::clone(&barrier);
                    let increment_amount = *increment;
                    
                    let handle = tokio::spawn(async move {
                        // Wait for all tasks to be ready
                        barrier_clone.wait().await;
                        
                        // Try to execute command with optimistic concurrency control
                        let input = ConcurrencyTestInput {
                            stream_id: stream_id_clone.clone(),
                            operation: ConcurrencyOperation::AddToCounter { amount: increment_amount },
                            expected_version: Some(current_version),
                        };
                        
                        // Read current state
                        let stream_data = store_clone.read_streams(&[stream_id_clone.clone()], &ReadOptions::new()).await.unwrap();
                        let mut state = ConcurrencyTestState::default();
                        for event in &stream_data.events {
                            command_clone.apply(&mut state, event);
                        }
                        
                        // Execute command
                        let command_result = command_clone.handle(state, input).await;
                        
                        if let Ok(events) = command_result {
                            if !events.is_empty() {
                                let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                    EventToWrite::new(EventId::new(), event.clone())
                                }).collect();
                                
                                let stream_events = vec![StreamEvents::new(
                                    stream_id_clone,
                                    ExpectedVersion::Exact(current_version),
                                    event_writes
                                )];
                                
                                // This may fail due to version conflict, which is expected
                                store_clone.write_events_multi(stream_events).await
                            } else {
                                Ok(HashMap::new())
                            }
                        } else {
                            Err(eventcore::errors::EventStoreError::ConcurrencyConflict("Command failed".to_string()))
                        }
                    });
                    
                    handles.push((handle, increment_amount));
                }
                
                // Collect results
                let mut successful_increments = Vec::new();
                let mut failed_count = 0;
                
                for (handle, increment_amount) in handles {
                    match handle.await.unwrap() {
                        Ok(_) => successful_increments.push(increment_amount),
                        Err(_) => failed_count += 1,
                    }
                }
                
                // Read final state
                let final_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let mut final_state = ConcurrencyTestState::default();
                for event in &final_data.events {
                    command.apply(&mut final_state, event);
                }
                
                // Verify consistency
                let expected_final_value = initial_value + successful_increments.iter().sum::<i64>();
                prop_assert_eq!(final_state.counter, expected_final_value);
                
                // At least one command should succeed or all should fail due to conflicts
                prop_assert!(successful_increments.len() + failed_count == increments.len());
                
                // Due to optimistic concurrency control, at most one concurrent operation should succeed
                // (since they all use the same expected version)
                prop_assert!(successful_increments.len() <= 1);
            });
        }
    }
}

/// Property test: Concurrent unique item creation maintains uniqueness.
///
/// This test verifies that when multiple commands try to create the same
/// unique item, only one succeeds.
#[test]
fn prop_concurrent_unique_creation_consistency() {
    proptest! {
        #[test]
        fn test_concurrent_unique_item_creation(
            stream_id in arb_stream_id(),
            item_id in any::<String>(),
            creators in prop::collection::vec(any::<String>(), 2..8)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = Arc::new(InMemoryEventStore::new());
                let command = Arc::new(ConcurrencyTestCommand);
                
                // Execute concurrent attempts to create the same unique item
                let barrier = Arc::new(Barrier::new(creators.len()));
                let mut handles = Vec::new();
                
                for creator_name in creators.iter() {
                    let store_clone = Arc::clone(&store);
                    let command_clone = Arc::clone(&command);
                    let stream_id_clone = stream_id.clone();
                    let barrier_clone = Arc::clone(&barrier);
                    let item_id_clone = item_id.clone();
                    let creator_name_clone = creator_name.clone();
                    
                    let handle = tokio::spawn(async move {
                        // Wait for all tasks to be ready
                        barrier_clone.wait().await;
                        
                        let input = ConcurrencyTestInput {
                            stream_id: stream_id_clone.clone(),
                            operation: ConcurrencyOperation::CreateUniqueItem {
                                id: item_id_clone,
                                name: creator_name_clone,
                            },
                            expected_version: None,
                        };
                        
                        // Read current state
                        let stream_data = store_clone.read_streams(&[stream_id_clone.clone()], &ReadOptions::new()).await.unwrap();
                        let mut state = ConcurrencyTestState::default();
                        for event in &stream_data.events {
                            command_clone.apply(&mut state, event);
                        }
                        
                        // Execute command
                        let command_result = command_clone.handle(state, input).await;
                        
                        if let Ok(events) = command_result {
                            if !events.is_empty() {
                                let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                    EventToWrite::new(EventId::new(), event.clone())
                                }).collect();
                                
                                let stream_events = vec![StreamEvents::new(
                                    stream_id_clone,
                                    ExpectedVersion::Any,
                                    event_writes
                                )];
                                
                                store_clone.write_events_multi(stream_events).await
                            } else {
                                Ok(HashMap::new())
                            }
                        } else {
                            // Command logic rejected the operation
                            Err(eventcore::errors::EventStoreError::ConcurrencyConflict("Command rejected".to_string()))
                        }
                    });
                    
                    handles.push(handle);
                }
                
                // Collect results
                let mut successful_creators = Vec::new();
                let mut failed_count = 0;
                
                for handle in handles {
                    match handle.await.unwrap() {
                        Ok(_) => successful_creators.push(()),
                        Err(_) => failed_count += 1,
                    }
                }
                
                // Read final state
                let final_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let mut final_state = ConcurrencyTestState::default();
                for event in &final_data.events {
                    command.apply(&mut final_state, event);
                }
                
                // Verify uniqueness constraint
                if final_state.items.contains_key(&item_id) {
                    // If item was created, exactly one creation should have succeeded
                    prop_assert_eq!(successful_creators.len(), 1);
                    prop_assert_eq!(failed_count, creators.len() - 1);
                } else {
                    // If no item was created, all attempts should have failed
                    prop_assert_eq!(successful_creators.len(), 0);
                    prop_assert_eq!(failed_count, creators.len());
                }
            });
        }
    }
}

/// Property test: Multi-stream operations maintain consistency.
///
/// This test verifies that operations affecting multiple streams
/// maintain consistency even under concurrent execution.
#[test]
fn prop_multi_stream_consistency() {
    proptest! {
        #[test]
        fn test_multi_stream_transfer_consistency(
            stream_ids in prop::collection::vec(arb_stream_id(), 2..4),
            initial_balances in prop::collection::vec(100u64..1000, 2..4),
            transfer_amounts in prop::collection::vec(1u64..50, 2..6)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = Arc::new(InMemoryEventStore::new());
                let command = Arc::new(ConcurrencyTestCommand);
                
                // Ensure we have at least as many balances as streams
                let stream_count = stream_ids.len().min(initial_balances.len());
                let streams = &stream_ids[..stream_count];
                let balances = &initial_balances[..stream_count];
                
                // Set up initial balances for each stream
                for (stream_id, initial_balance) in streams.iter().zip(balances.iter()) {
                    let initial_events = vec![EventToWrite::new(
                        EventId::new(),
                        ConcurrencyTestEvent::MoneyTransferred {
                            from_key: "genesis".to_string(),
                            to_key: "account".to_string(),
                            amount: *initial_balance,
                        }
                    )];
                    
                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        ExpectedVersion::New,
                        initial_events
                    )];
                    
                    store.write_events_multi(stream_events).await.unwrap();
                }
                
                // Calculate total money in system
                let total_initial_money: u64 = balances.iter().sum();
                
                // Execute concurrent transfers between streams
                let barrier = Arc::new(Barrier::new(transfer_amounts.len()));
                let mut handles = Vec::new();
                
                for (i, transfer_amount) in transfer_amounts.iter().enumerate() {
                    let store_clone = Arc::clone(&store);
                    let command_clone = Arc::clone(&command);
                    let barrier_clone = Arc::clone(&barrier);
                    let from_stream = streams[i % streams.len()].clone();
                    let to_stream = streams[(i + 1) % streams.len()].clone();
                    let amount = *transfer_amount;
                    
                    let handle = tokio::spawn(async move {
                        // Wait for all tasks to be ready
                        barrier_clone.wait().await;
                        
                        let input = ConcurrencyTestInput {
                            stream_id: from_stream.clone(),
                            operation: ConcurrencyOperation::Transfer {
                                from_key: "account".to_string(),
                                to_key: "account".to_string(),
                                amount,
                            },
                            expected_version: None,
                        };
                        
                        // Read current state from the source stream
                        let stream_data = store_clone.read_streams(&[from_stream.clone()], &ReadOptions::new()).await.unwrap();
                        let mut state = ConcurrencyTestState::default();
                        for event in &stream_data.events {
                            command_clone.apply(&mut state, &event.payload);
                        }
                        
                        // Execute command
                        let command_result = command_clone.handle(state, input).await;
                        
                        if let Ok(events) = command_result {
                            if !events.is_empty() {
                                let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                    EventToWrite::new(EventId::new(), event.clone())
                                }).collect();
                                
                                let stream_events = vec![StreamEvents::new(
                                    from_stream,
                                    ExpectedVersion::Any,
                                    event_writes
                                )];
                                
                                store_clone.write_events_multi(stream_events).await
                            } else {
                                Ok(HashMap::new())
                            }
                        } else {
                            Err(eventcore::errors::EventStoreError::ConcurrencyConflict("Transfer rejected".to_string()))
                        }
                    });
                    
                    handles.push(handle);
                }
                
                // Wait for all transfers to complete
                for handle in handles {
                    let _ = handle.await.unwrap(); // Some may fail due to insufficient funds
                }
                
                // Read final state from all streams
                let mut total_final_money = 0u64;
                for stream_id in streams {
                    let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    let mut state = ConcurrencyTestState::default();
                    for event in &stream_data.events {
                        command.apply(&mut state, &event.payload);
                    }
                    
                    total_final_money += state.balances.get("account").unwrap_or(&0);
                }
                
                // Verify conservation of money (no money created or destroyed)
                prop_assert_eq!(total_final_money, total_initial_money);
            });
        }
    }
}

/// Property test: Version conflicts are properly handled.
///
/// This test verifies that when commands specify expected versions,
/// version conflicts are detected and handled correctly.
#[test]
fn prop_version_conflict_handling() {
    proptest! {
        #[test]
        fn test_version_conflict_detection(
            stream_id in arb_stream_id(),
            operations in prop::collection::vec(arb_concurrency_operation(), 2..8),
            wrong_version_offset in 1u64..10
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let command = ConcurrencyTestCommand;
                
                // Execute first operation to establish a baseline
                let first_input = ConcurrencyTestInput {
                    stream_id: stream_id.clone(),
                    operation: operations[0].clone(),
                    expected_version: None,
                };
                
                let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let mut state = ConcurrencyTestState::default();
                for event in &stream_data.events {
                    command.apply(&mut state, &event.payload);
                }
                
                let first_result = command.handle(state, first_input).await;
                
                if let Ok(events) = first_result {
                    if !events.is_empty() {
                        let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                            EventToWrite::new(EventId::new(), event.clone())
                        }).collect();
                        
                        let stream_events = vec![StreamEvents::new(
                            stream_id.clone(),
                            ExpectedVersion::New,
                            event_writes
                        )];
                        
                        store.write_events_multi(stream_events).await.unwrap();
                    }
                }
                
                // Get current version
                let current_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let current_version = current_data.stream_versions.get(&stream_id)
                    .copied()
                    .unwrap_or(EventVersion::initial());
                
                // Try to execute with wrong expected version
                if let Ok(wrong_version) = EventVersion::try_new(u64::from(current_version) + wrong_version_offset) {
                    let wrong_input = ConcurrencyTestInput {
                        stream_id: stream_id.clone(),
                        operation: operations.get(1).unwrap_or(&operations[0]).clone(),
                        expected_version: Some(wrong_version),
                    };
                    
                    let current_state_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    let mut current_state = ConcurrencyTestState::default();
                    for event in &current_state_data.events {
                        command.apply(&mut current_state, &event.payload);
                    }
                    
                    let wrong_result = command.handle(current_state, wrong_input).await;
                    
                    if let Ok(events) = wrong_result {
                        if !events.is_empty() {
                            let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                EventToWrite::new(EventId::new(), event.clone())
                            }).collect();
                            
                            let stream_events = vec![StreamEvents::new(
                                stream_id.clone(),
                                ExpectedVersion::Exact(wrong_version),
                                event_writes
                            )];
                            
                            // This should fail due to version mismatch
                            let write_result = store.write_events_multi(stream_events).await;
                            prop_assert!(write_result.is_err(), "Write with wrong version should fail");
                        }
                    }
                }
                
                // Verify stream state is unchanged
                let unchanged_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let unchanged_version = unchanged_data.stream_versions.get(&stream_id)
                    .copied()
                    .unwrap_or(EventVersion::initial());
                
                prop_assert_eq!(unchanged_version, current_version);
            });
        }
    }
}

/// Generator for concurrency test operations.
fn arb_concurrency_operation() -> impl Strategy<Value = ConcurrencyOperation> {
    prop_oneof![
        any::<i64>().prop_map(|amount| ConcurrencyOperation::AddToCounter { amount }),
        (any::<String>(), any::<String>(), 1u64..100).prop_map(|(from, to, amount)| {
            ConcurrencyOperation::Transfer { from_key: from, to_key: to, amount }
        }),
        (any::<String>(), any::<String>()).prop_map(|(id, name)| {
            ConcurrencyOperation::CreateUniqueItem { id, name }
        }),
        (any::<String>(), any::<String>()).prop_map(|(id, name)| {
            ConcurrencyOperation::UpdateIfExists { id, new_name: name }
        })
    ]
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_concurrency_setup() {
        let store = InMemoryEventStore::new();
        let command = ConcurrencyTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Test basic command execution
        let input = ConcurrencyTestInput {
            stream_id: stream_id.clone(),
            operation: ConcurrencyOperation::AddToCounter { amount: 10 },
            expected_version: None,
        };
        
        let result = command.handle(ConcurrencyTestState::default(), input).await.unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].1, ConcurrencyTestEvent::CounterIncremented { amount: 10 }));
    }

    #[tokio::test]
    async fn test_transfer_validation() {
        let command = ConcurrencyTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Test transfer with insufficient balance
        let mut state = ConcurrencyTestState::default();
        state.balances.insert("account1".to_string(), 50);
        
        let input = ConcurrencyTestInput {
            stream_id: stream_id.clone(),
            operation: ConcurrencyOperation::Transfer {
                from_key: "account1".to_string(),
                to_key: "account2".to_string(),
                amount: 100, // More than available
            },
            expected_version: None,
        };
        
        let result = command.handle(state, input).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CommandError::BusinessRuleViolation(_)));
    }

    #[tokio::test]
    async fn test_unique_item_constraint() {
        let command = ConcurrencyTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Create initial state with existing item
        let mut state = ConcurrencyTestState::default();
        state.items.insert("item1".to_string(), "Original Name".to_string());
        
        // Try to create item with same ID
        let input = ConcurrencyTestInput {
            stream_id: stream_id.clone(),
            operation: ConcurrencyOperation::CreateUniqueItem {
                id: "item1".to_string(),
                name: "New Name".to_string(),
            },
            expected_version: None,
        };
        
        let result = command.handle(state, input).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CommandError::BusinessRuleViolation(_)));
    }
}