//! Property tests for command idempotency.
//!
//! These tests verify that commands produce the same result when executed
//! multiple times with the same input and state, which is essential for
//! reliable event sourcing systems.

use eventcore::command::{Command, CommandResult};
use eventcore::errors::CommandError;
use eventcore::event_store::{EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamEvents};
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, StreamId};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use std::collections::HashMap;

/// Test command that implements idempotent behavior.
#[derive(Debug, Clone)]
pub struct IdempotentTestCommand;

/// Input for the idempotent test command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotentCommandInput {
    pub stream_id: StreamId,
    pub operation: IdempotentOperation,
}

/// Operations that should be idempotent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotentOperation {
    /// Create an item with a specific ID (idempotent if item already exists)
    CreateItem { id: String, name: String },
    /// Set a value (idempotent - always results in the same final state)
    SetValue { key: String, value: i64 },
    /// Increment by amount (NOT idempotent - demonstrates non-idempotent behavior)
    IncrementValue { key: String, amount: i64 },
    /// Delete an item (idempotent if item doesn't exist)
    DeleteItem { id: String },
}

/// Test event for idempotency testing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IdempotentTestEvent {
    ItemCreated { id: String, name: String },
    ValueSet { key: String, value: i64 },
    ValueIncremented { key: String, amount: i64 },
    ItemDeleted { id: String },
}

/// State for the idempotent test command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdempotentTestState {
    pub items: HashMap<String, String>,
    pub values: HashMap<String, i64>,
}

#[async_trait::async_trait]
impl Command for IdempotentTestCommand {
    type Input = IdempotentCommandInput;
    type State = IdempotentTestState;
    type Event = IdempotentTestEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &eventcore::event_store::StoredEvent<Self::Event>) {
        match &stored_event.payload {
            IdempotentTestEvent::ItemCreated { id, name } => {
                state.items.insert(id.clone(), name.clone());
            }
            IdempotentTestEvent::ValueSet { key, value } => {
                state.values.insert(key.clone(), *value);
            }
            IdempotentTestEvent::ValueIncremented { key, amount } => {
                let current = state.values.get(key).unwrap_or(&0);
                state.values.insert(key.clone(), current + amount);
            }
            IdempotentTestEvent::ItemDeleted { id } => {
                state.items.remove(id);
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        let event = match input.operation {
            IdempotentOperation::CreateItem { id, name } => {
                // Idempotent: only create if doesn't exist
                if state.items.contains_key(&id) {
                    // Item already exists, no event needed (idempotent)
                    return Ok(vec![]);
                }
                IdempotentTestEvent::ItemCreated { id, name }
            }
            IdempotentOperation::SetValue { key, value } => {
                // Idempotent: only emit event if value changes
                if state.values.get(&key) == Some(&value) {
                    // Value already set to this value, no event needed (idempotent)
                    return Ok(vec![]);
                }
                IdempotentTestEvent::ValueSet { key, value }
            }
            IdempotentOperation::IncrementValue { key, amount } => {
                // NOT idempotent: always increments regardless of current state
                IdempotentTestEvent::ValueIncremented { key, amount }
            }
            IdempotentOperation::DeleteItem { id } => {
                // Idempotent: only delete if exists
                if !state.items.contains_key(&id) {
                    // Item doesn't exist, no event needed (idempotent)
                    return Ok(vec![]);
                }
                IdempotentTestEvent::ItemDeleted { id }
            }
        };

        Ok(vec![(input.stream_id, event)])
    }
}

/// Property test: Idempotent commands produce same result when repeated.
///
/// This test verifies that truly idempotent operations (create, set, delete)
/// produce the same result when executed multiple times.
#[test]
fn prop_idempotent_commands_same_result() {
    proptest! {
        #[test]
        fn test_idempotent_operations(
            stream_id in arb_stream_id(),
            operation in arb_idempotent_operation(),
            repetitions in 1usize..5
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let command = IdempotentTestCommand;
                let input = IdempotentCommandInput {
                    stream_id: stream_id.clone(),
                    operation: operation.clone(),
                };
                
                let mut previous_result = None;
                let mut previous_state = None;
                
                // Execute the command multiple times
                for iteration in 0..repetitions {
                    // Read current state
                    let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    let mut state = IdempotentTestState::default();
                    for event in &stream_data.events {
                        command.apply(&mut state, event);
                    }
                    
                    // Execute command
                    let result = command.handle(state.clone(), input.clone()).await;
                    prop_assert!(result.is_ok(), "Command should not fail on iteration {}: {:?}", iteration, result);
                    
                    let events = result.unwrap();
                    
                    if iteration == 0 {
                        // First execution - store result
                        previous_result = Some(events.clone());
                        previous_state = Some(state.clone());
                        
                        // Write events to store if any were produced
                        if !events.is_empty() {
                            let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                EventToWrite::new(EventId::new(), event.clone())
                            }).collect();
                            
                            let stream_events = vec![StreamEvents::new(
                                stream_id.clone(),
                                ExpectedVersion::Any,
                                event_writes
                            )];
                            
                            store.write_events_multi(stream_events).await.unwrap();
                        }
                    } else {
                        // Subsequent executions - should be idempotent
                        match &operation {
                            IdempotentOperation::IncrementValue { .. } => {
                                // Increment is NOT idempotent, so we expect different results
                                // (This tests our test framework can detect non-idempotent operations)
                                prop_assert!(
                                    events != previous_result.as_ref().unwrap() || 
                                    (events.is_empty() && previous_result.as_ref().unwrap().is_empty()),
                                    "Increment operation should not be idempotent"
                                );
                            }
                            _ => {
                                // These operations should be idempotent
                                prop_assert_eq!(
                                    events, 
                                    previous_result.as_ref().unwrap().clone(),
                                    "Idempotent operation {:?} produced different result on iteration {}", 
                                    operation, iteration
                                );
                            }
                        }
                    }
                }
            });
        }
    }
}

/// Property test: Command execution with same input and state is deterministic.
///
/// This test verifies that commands always produce the same result when
/// given identical input and state.
#[test]
fn prop_command_execution_deterministic() {
    proptest! {
        #[test]
        fn test_deterministic_execution(
            stream_id in arb_stream_id(),
            operation in arb_any_operation(),
            initial_events in prop::collection::vec(arb_idempotent_event(), 0..10)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let command = IdempotentTestCommand;
                let input = IdempotentCommandInput {
                    stream_id: stream_id.clone(),
                    operation,
                };
                
                // Build initial state from events
                let mut initial_state = IdempotentTestState::default();
                for event in &initial_events {
                    command.apply(&mut initial_state, event);
                }
                
                // Execute command multiple times with same state and input
                let mut results = Vec::new();
                for _ in 0..3 {
                    let state_copy = initial_state.clone();
                    let input_copy = input.clone();
                    let result = command.handle(state_copy, input_copy).await;
                    results.push(result);
                }
                
                // All results should be identical
                let first_result = &results[0];
                for (i, result) in results.iter().enumerate().skip(1) {
                    prop_assert_eq!(
                        result, first_result,
                        "Command execution was not deterministic on attempt {}", i + 1
                    );
                }
            });
        }
    }
}

/// Property test: State reconstruction is idempotent.
///
/// This test verifies that applying the same sequence of events
/// multiple times produces the same final state.
#[test]
fn prop_state_reconstruction_idempotent() {
    proptest! {
        #[test]
        fn test_state_reconstruction_idempotency(
            events in prop::collection::vec(arb_idempotent_event(), 0..20)
        ) {
            let command = IdempotentTestCommand;
            
            // Apply events to build state
            let mut state1 = IdempotentTestState::default();
            for event in &events {
                command.apply(&mut state1, event);
            }
            
            // Apply same events again to a fresh state
            let mut state2 = IdempotentTestState::default();
            for event in &events {
                command.apply(&mut state2, event);
            }
            
            // States should be identical
            prop_assert_eq!(state1, state2);
            
            // Apply events a third time to existing state (should not change it)
            let state_before_reapply = state1.clone();
            for event in &events {
                command.apply(&mut state1, event);
            }
            
            // State should not change when events are reapplied
            // (This verifies that our apply logic is truly idempotent for state building)
            prop_assert_eq!(state1.items, state_before_reapply.items);
            
            // Note: Values might change if we have increment events, which is expected
            // but items (create/delete) should be idempotent
        }
    }
}

/// Property test: Command behavior is consistent across different execution contexts.
///
/// This test verifies that commands behave the same way regardless of
/// how they are executed (direct vs. through event store).
#[test]
fn prop_command_context_independence() {
    proptest! {
        #[test]
        fn test_context_independent_execution(
            stream_id in arb_stream_id(),
            operation in arb_idempotent_operation(), // Only test truly idempotent operations
            setup_events in prop::collection::vec(arb_idempotent_event(), 0..5)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let command = IdempotentTestCommand;
                let input = IdempotentCommandInput {
                    stream_id: stream_id.clone(),
                    operation,
                };
                
                // Scenario 1: Direct execution with in-memory state
                let mut direct_state = IdempotentTestState::default();
                for event in &setup_events {
                    command.apply(&mut direct_state, event);
                }
                let direct_result = command.handle(direct_state.clone(), input.clone()).await.unwrap();
                
                // Scenario 2: Execution through event store
                let store = InMemoryEventStore::new();
                
                // Write setup events to store
                if !setup_events.is_empty() {
                    let event_writes: Vec<_> = setup_events.iter().map(|event| {
                        EventToWrite::new(EventId::new(), event.clone())
                    }).collect();
                    
                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        ExpectedVersion::New,
                        event_writes
                    )];
                    
                    store.write_events_multi(stream_events).await.unwrap();
                }
                
                // Read state from store and execute command
                let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let mut store_state = IdempotentTestState::default();
                for event in &stream_data.events {
                    command.apply(&mut store_state, &event.payload);
                }
                let store_result = command.handle(store_state, input).await.unwrap();
                
                // Results should be identical regardless of execution context
                prop_assert_eq!(direct_result, store_result);
            });
        }
    }
}

/// Generator for idempotent operations (excludes increment which is not idempotent).
fn arb_idempotent_operation() -> impl Strategy<Value = IdempotentOperation> {
    prop_oneof![
        (any::<String>(), any::<String>()).prop_map(|(id, name)| {
            IdempotentOperation::CreateItem { id, name }
        }),
        (any::<String>(), any::<i64>()).prop_map(|(key, value)| {
            IdempotentOperation::SetValue { key, value }
        }),
        any::<String>().prop_map(|id| IdempotentOperation::DeleteItem { id })
    ]
}

/// Generator for any operation (including non-idempotent ones).
fn arb_any_operation() -> impl Strategy<Value = IdempotentOperation> {
    prop_oneof![
        (any::<String>(), any::<String>()).prop_map(|(id, name)| {
            IdempotentOperation::CreateItem { id, name }
        }),
        (any::<String>(), any::<i64>()).prop_map(|(key, value)| {
            IdempotentOperation::SetValue { key, value }
        }),
        (any::<String>(), any::<i64>()).prop_map(|(key, amount)| {
            IdempotentOperation::IncrementValue { key, amount }
        }),
        any::<String>().prop_map(|id| IdempotentOperation::DeleteItem { id })
    ]
}

/// Generator for idempotent test events.
fn arb_idempotent_event() -> impl Strategy<Value = IdempotentTestEvent> {
    prop_oneof![
        (any::<String>(), any::<String>()).prop_map(|(id, name)| {
            IdempotentTestEvent::ItemCreated { id, name }
        }),
        (any::<String>(), any::<i64>()).prop_map(|(key, value)| {
            IdempotentTestEvent::ValueSet { key, value }
        }),
        (any::<String>(), any::<i64>()).prop_map(|(key, amount)| {
            IdempotentTestEvent::ValueIncremented { key, amount }
        }),
        any::<String>().prop_map(|id| IdempotentTestEvent::ItemDeleted { id })
    ]
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_item_idempotency() {
        let command = IdempotentTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let input = IdempotentCommandInput {
            stream_id: stream_id.clone(),
            operation: IdempotentOperation::CreateItem {
                id: "item-1".to_string(),
                name: "Test Item".to_string(),
            },
        };
        
        // First execution - should create event
        let result1 = command.handle(IdempotentTestState::default(), input.clone()).await.unwrap();
        assert_eq!(result1.len(), 1);
        assert!(matches!(result1[0].1, IdempotentTestEvent::ItemCreated { .. }));
        
        // Build state from first result
        let mut state = IdempotentTestState::default();
        command.apply(&mut state, &result1[0].1);
        
        // Second execution - should be idempotent (no events)
        let result2 = command.handle(state, input).await.unwrap();
        assert_eq!(result2.len(), 0); // No events because item already exists
    }

    #[tokio::test]
    async fn test_set_value_idempotency() {
        let command = IdempotentTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let input = IdempotentCommandInput {
            stream_id: stream_id.clone(),
            operation: IdempotentOperation::SetValue {
                key: "counter".to_string(),
                value: 42,
            },
        };
        
        // First execution
        let result1 = command.handle(IdempotentTestState::default(), input.clone()).await.unwrap();
        assert_eq!(result1.len(), 1);
        
        // Build state
        let mut state = IdempotentTestState::default();
        command.apply(&mut state, &result1[0].1);
        
        // Second execution with same value - should be idempotent
        let result2 = command.handle(state, input).await.unwrap();
        assert_eq!(result2.len(), 0); // No events because value already set
    }

    #[tokio::test]
    async fn test_increment_not_idempotent() {
        let command = IdempotentTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let input = IdempotentCommandInput {
            stream_id: stream_id.clone(),
            operation: IdempotentOperation::IncrementValue {
                key: "counter".to_string(),
                amount: 10,
            },
        };
        
        // First execution
        let result1 = command.handle(IdempotentTestState::default(), input.clone()).await.unwrap();
        assert_eq!(result1.len(), 1);
        
        // Build state
        let mut state = IdempotentTestState::default();
        command.apply(&mut state, &result1[0].1);
        
        // Second execution - should NOT be idempotent (creates another event)
        let result2 = command.handle(state, input).await.unwrap();
        assert_eq!(result2.len(), 1); // Another event because increment is not idempotent
    }

    #[tokio::test]
    async fn test_delete_item_idempotency() {
        let command = IdempotentTestCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let input = IdempotentCommandInput {
            stream_id: stream_id.clone(),
            operation: IdempotentOperation::DeleteItem {
                id: "item-1".to_string(),
            },
        };
        
        // First execution on empty state - should be idempotent (no delete needed)
        let result1 = command.handle(IdempotentTestState::default(), input.clone()).await.unwrap();
        assert_eq!(result1.len(), 0); // No events because item doesn't exist
        
        // Create an item first
        let mut state = IdempotentTestState::default();
        state.items.insert("item-1".to_string(), "Test Item".to_string());
        
        // Delete the item
        let result2 = command.handle(state.clone(), input.clone()).await.unwrap();
        assert_eq!(result2.len(), 1);
        assert!(matches!(result2[0].1, IdempotentTestEvent::ItemDeleted { .. }));
        
        // Apply delete event
        command.apply(&mut state, &result2[0].1);
        
        // Try to delete again - should be idempotent
        let result3 = command.handle(state, input).await.unwrap();
        assert_eq!(result3.len(), 0); // No events because item already deleted
    }
}