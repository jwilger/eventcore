//! Advanced concurrency testing scenarios.
//!
//! This module contains sophisticated property-based tests that verify
//! complex concurrency patterns and edge cases in the event sourcing system.

use eventcore::command::{Command, CommandResult};
use eventcore::errors::CommandError;
use eventcore::event_store::{EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamEvents};
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, EventVersion, StreamId};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Barrier, Semaphore};
use tokio::time::{timeout, Duration};

/// Advanced test command for complex concurrency scenarios.
#[derive(Debug, Clone)]
pub struct AdvancedConcurrencyCommand;

/// Input for advanced concurrency test command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedConcurrencyInput {
    pub stream_ids: Vec<StreamId>,
    pub operation: AdvancedOperation,
    pub delay_ms: Option<u64>,
}

/// Advanced operations that test complex concurrency scenarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvancedOperation {
    /// Multi-stream atomic operation (tests distributed consistency)
    MultiStreamUpdate { 
        operations: Vec<(String, StreamOperation)> 
    },
    /// Saga-like operation with rollback (tests transaction-like behavior)
    SagaOperation { 
        steps: Vec<SagaStep> 
    },
    /// Resource allocation with limited capacity (tests resource contention)
    AllocateResource { 
        resource_type: String, 
        amount: u32,
        max_capacity: u32 
    },
    /// Hierarchical operation affecting parent and children (tests cascade effects)
    HierarchyUpdate { 
        parent_id: String, 
        child_updates: Vec<(String, String)> 
    },
}

/// Individual stream operations for multi-stream updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamOperation {
    Increment(i64),
    Decrement(i64),
    SetValue(i64),
    AddItem(String),
    RemoveItem(String),
}

/// Steps in a saga operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SagaStep {
    Reserve { resource: String, amount: u32 },
    Charge { account: String, amount: u64 },
    Allocate { target: String, resource: String, amount: u32 },
    Complete { operation_id: String },
}

/// Events for advanced concurrency testing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AdvancedEvent {
    MultiStreamUpdated { 
        operations: Vec<(String, StreamOperation)> 
    },
    SagaStepExecuted { 
        step: SagaStep, 
        step_index: usize 
    },
    SagaCompleted { 
        operation_id: String 
    },
    SagaFailed { 
        operation_id: String, 
        failed_step: usize 
    },
    ResourceAllocated { 
        resource_type: String, 
        amount: u32, 
        remaining_capacity: u32 
    },
    ResourceAllocationFailed { 
        resource_type: String, 
        requested: u32, 
        available: u32 
    },
    HierarchyUpdated { 
        parent_id: String, 
        child_updates: Vec<(String, String)> 
    },
}

/// State for advanced concurrency testing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvancedState {
    pub counters: HashMap<String, i64>,
    pub items: HashMap<String, HashSet<String>>,
    pub resource_allocations: HashMap<String, u32>,
    pub active_sagas: HashMap<String, Vec<SagaStep>>,
    pub hierarchy: HashMap<String, Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl Command for AdvancedConcurrencyCommand {
    type Input = AdvancedConcurrencyInput;
    type State = AdvancedState;
    type Event = AdvancedEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        input.stream_ids.clone()
    }

    fn apply(&self, state: &mut Self::State, stored_event: &eventcore::event_store::StoredEvent<Self::Event>) {
        match &stored_event.payload {
            AdvancedEvent::MultiStreamUpdated { operations } => {
                for (key, operation) in operations {
                    match operation {
                        StreamOperation::Increment(amount) => {
                            *state.counters.entry(key.clone()).or_insert(0) += amount;
                        }
                        StreamOperation::Decrement(amount) => {
                            *state.counters.entry(key.clone()).or_insert(0) -= amount;
                        }
                        StreamOperation::SetValue(value) => {
                            state.counters.insert(key.clone(), *value);
                        }
                        StreamOperation::AddItem(item) => {
                            state.items.entry(key.clone()).or_default().insert(item.clone());
                        }
                        StreamOperation::RemoveItem(item) => {
                            if let Some(items) = state.items.get_mut(key) {
                                items.remove(item);
                            }
                        }
                    }
                }
            }
            AdvancedEvent::SagaStepExecuted { step, .. } => {
                match step {
                    SagaStep::Reserve { resource, amount } => {
                        *state.resource_allocations.entry(resource.clone()).or_insert(0) += amount;
                    }
                    SagaStep::Charge { account, amount } => {
                        *state.counters.entry(account.clone()).or_insert(0) -= *amount as i64;
                    }
                    SagaStep::Allocate { target, resource, amount } => {
                        state.items.entry(target.clone()).or_default().insert(format!("{resource}:{amount}"));
                    }
                    SagaStep::Complete { operation_id } => {
                        state.active_sagas.remove(operation_id);
                    }
                }
            }
            AdvancedEvent::ResourceAllocated { resource_type, amount, .. } => {
                *state.resource_allocations.entry(resource_type.clone()).or_insert(0) += amount;
            }
            AdvancedEvent::HierarchyUpdated { parent_id, child_updates } => {
                state.hierarchy.insert(parent_id.clone(), child_updates.clone());
            }
            _ => {
                // Other events don't affect state
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        // Add artificial delay to increase chance of concurrency conflicts
        if let Some(delay_ms) = input.delay_ms {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        let event = match input.operation {
            AdvancedOperation::MultiStreamUpdate { operations } => {
                // Validate all operations before executing any
                for (key, operation) in &operations {
                    match operation {
                        StreamOperation::Decrement(amount) => {
                            let current = state.counters.get(key).unwrap_or(&0);
                            if *current < *amount {
                                return Err(CommandError::BusinessRuleViolation(
                                    format!("Insufficient value in {}: {} < {}", key, current, amount)
                                ));
                            }
                        }
                        StreamOperation::RemoveItem(item) => {
                            if !state.items.get(key).map_or(false, |items| items.contains(item)) {
                                return Err(CommandError::BusinessRuleViolation(
                                    format!("Item {} not found in {}", item, key)
                                ));
                            }
                        }
                        _ => {} // Other operations are always valid
                    }
                }
                AdvancedEvent::MultiStreamUpdated { operations }
            }
            AdvancedOperation::AllocateResource { resource_type, amount, max_capacity } => {
                let current_allocation = state.resource_allocations.get(&resource_type).unwrap_or(&0);
                if current_allocation + amount > max_capacity {
                    return Ok(vec![(
                        input.stream_ids[0].clone(),
                        AdvancedEvent::ResourceAllocationFailed {
                            resource_type,
                            requested: amount,
                            available: max_capacity.saturating_sub(*current_allocation),
                        }
                    )]);
                }
                AdvancedEvent::ResourceAllocated {
                    resource_type,
                    amount,
                    remaining_capacity: max_capacity - current_allocation - amount,
                }
            }
            AdvancedOperation::HierarchyUpdate { parent_id, child_updates } => {
                AdvancedEvent::HierarchyUpdated { parent_id, child_updates }
            }
            AdvancedOperation::SagaOperation { steps } => {
                // For now, just complete the saga (more complex logic could be added)
                AdvancedEvent::SagaCompleted { 
                    operation_id: format!("saga-{}", EventId::new()) 
                }
            }
        };

        // Write to first stream (more complex routing could be implemented)
        Ok(vec![(input.stream_ids[0].clone(), event)])
    }
}

/// Property test: Massive concurrency stress test.
///
/// This test verifies system behavior under extreme concurrent load
/// with many operations happening simultaneously.
#[test]
fn prop_massive_concurrency_stress() {
    use eventcore::testing::generators::{arb_concurrent_stream_ids, arb_concurrent_string};
    use crate::properties::enhanced_proptest_config;
    
    let config = enhanced_proptest_config();
    proptest! {
        #![proptest_config(config)]
        #[test]
        fn test_massive_concurrent_operations(
            stream_ids in arb_concurrent_stream_ids(),
            operation_count in prop_oneof![
                // Prefer smaller counts for faster shrinking
                70 => (5usize..=15),
                20 => (10usize..=25), 
                10 => (20usize..=50),
            ],
            operation_keys in prop::collection::vec(arb_concurrent_string(), 1..=5),
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = Arc::new(InMemoryEventStore::new());
                let command = Arc::new(AdvancedConcurrencyCommand);
                
                // Limit concurrency to avoid overwhelming the system
                let semaphore = Arc::new(Semaphore::new(10));
                let barrier = Arc::new(Barrier::new(operation_count));
                let mut handles = Vec::new();
                
                for i in 0..operation_count {
                    let store_clone = Arc::clone(&store);
                    let command_clone = Arc::clone(&command);
                    let barrier_clone = Arc::clone(&barrier);
                    let semaphore_clone = Arc::clone(&semaphore);
                    let stream_ids_clone = stream_ids.clone();
                    let operation_keys_clone = operation_keys.clone();
                    
                    let handle = tokio::spawn(async move {
                        let _permit = semaphore_clone.acquire().await.unwrap();
                        
                        // Wait for all tasks to be ready
                        barrier_clone.wait().await;
                        
                        let input = AdvancedConcurrencyInput {
                            stream_ids: stream_ids_clone,
                            operation: AdvancedOperation::MultiStreamUpdate {
                                operations: vec![(
                                    operation_keys_clone[i % operation_keys_clone.len()].clone(),
                                    StreamOperation::Increment(1)
                                )]
                            },
                            delay_ms: Some(i as u64 % 5), // Small random delays
                        };
                        
                        // Read current state
                        let stream_data = store_clone.read_streams(&input.stream_ids, &ReadOptions::new()).await.unwrap();
                        let mut state = AdvancedState::default();
                        for event in &stream_data.events {
                            command_clone.apply(&mut state, event);
                        }
                        
                        // Execute command with timeout to prevent hanging
                        let command_result = timeout(
                            Duration::from_secs(5),
                            command_clone.handle(state, input.clone())
                        ).await;
                        
                        match command_result {
                            Ok(Ok(events)) if !events.is_empty() => {
                                let event_writes: Vec<_> = events.iter().map(|(_, event)| {
                                    EventToWrite::new(EventId::new(), event.clone())
                                }).collect();
                                
                                // Use first stream for writing
                                let stream_events = vec![StreamEvents::new(
                                    input.stream_ids[0].clone(),
                                    ExpectedVersion::Any,
                                    event_writes
                                )];
                                
                                store_clone.write_events_multi(stream_events).await
                            }
                            _ => Err(eventcore::errors::EventStoreError::ConcurrencyConflict("Operation failed or timed out".to_string()))
                        }
                    });
                    
                    handles.push(handle);
                }
                
                // Collect results
                let mut successful_operations = 0;
                let mut failed_operations = 0;
                
                for handle in handles {
                    match handle.await.unwrap() {
                        Ok(_) => successful_operations += 1,
                        Err(_) => failed_operations += 1,
                    }
                }
                
                // Verify system maintained consistency
                let final_data = store.read_streams(&stream_ids, &ReadOptions::new()).await.unwrap();
                let mut final_state = AdvancedState::default();
                for event in &final_data.events {
                    command.apply(&mut final_state, event);
                }
                
                // Basic consistency checks
                prop_assert!(successful_operations + failed_operations == operation_count);
                
                // The sum of all increments should equal successful operations
                let total_increments: i64 = final_state.counters.values().sum();
                prop_assert_eq!(total_increments, successful_operations as i64);
                
                // No individual counter should exceed the total operation count
                for counter_value in final_state.counters.values() {
                    prop_assert!(*counter_value <= operation_count as i64);
                }
            });
        }
    }
}

/// Property test: Resource contention under load.
///
/// This test verifies correct behavior when multiple operations compete
/// for limited resources with various allocation patterns.
#[test]
fn prop_resource_contention() {
    use eventcore::testing::generators::{arb_concurrent_string, arb_transfer_amount};
    use crate::properties::enhanced_proptest_config;
    
    let config = enhanced_proptest_config();
    proptest! {
        #![proptest_config(config)]
        #[test]
        fn test_resource_allocation_contention(
            stream_id in arb_stream_id(),
            resource_type in arb_concurrent_string(),
            max_capacity in prop_oneof![
                // Prefer small capacities for more contention
                60 => (1u32..=10u32),
                30 => (5u32..=25u32),
                10 => (20u32..=100u32),
            ],
            allocation_requests in prop::collection::vec(
                arb_transfer_amount().prop_map(|v| v as u32), 
                1..=10
            ),
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = Arc::new(InMemoryEventStore::new());
                let command = Arc::new(AdvancedConcurrencyCommand);
                
                let barrier = Arc::new(Barrier::new(allocation_requests.len()));
                let mut handles = Vec::new();
                
                for amount in allocation_requests.iter() {
                    let store_clone = Arc::clone(&store);
                    let command_clone = Arc::clone(&command);
                    let barrier_clone = Arc::clone(&barrier);
                    let stream_id_clone = stream_id.clone();
                    let resource_type_clone = resource_type.clone();
                    let allocation_amount = *amount;
                    
                    let handle = tokio::spawn(async move {
                        // Wait for all tasks to be ready
                        barrier_clone.wait().await;
                        
                        let input = AdvancedConcurrencyInput {
                            stream_ids: vec![stream_id_clone.clone()],
                            operation: AdvancedOperation::AllocateResource {
                                resource_type: resource_type_clone,
                                amount: allocation_amount,
                                max_capacity,
                            },
                            delay_ms: None,
                        };
                        
                        // Read current state
                        let stream_data = store_clone.read_streams(&[stream_id_clone.clone()], &ReadOptions::new()).await.unwrap();
                        let mut state = AdvancedState::default();
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
                            Err(eventcore::errors::EventStoreError::ConcurrencyConflict("Allocation failed".to_string()))
                        }
                    });
                    
                    handles.push((handle, allocation_amount));
                }
                
                // Collect results
                let mut successful_allocations = Vec::new();
                let mut failed_allocations = Vec::new();
                
                for (handle, amount) in handles {
                    match handle.await.unwrap() {
                        Ok(_) => successful_allocations.push(amount),
                        Err(_) => failed_allocations.push(amount),
                    }
                }
                
                // Read final state
                let final_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let mut final_state = AdvancedState::default();
                for event in &final_data.events {
                    command.apply(&mut final_state, event);
                }
                
                // Verify resource allocation constraints
                let total_allocated = final_state.resource_allocations.get(&resource_type).unwrap_or(&0);
                let expected_total: u32 = successful_allocations.iter().sum();
                
                prop_assert_eq!(*total_allocated, expected_total);
                prop_assert!(*total_allocated <= max_capacity);
                
                // Verify no over-allocation occurred
                if expected_total > 0 {
                    prop_assert!(expected_total <= max_capacity);
                }
            });
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_advanced_command() {
        let store = InMemoryEventStore::new();
        let command = AdvancedConcurrencyCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        let input = AdvancedConcurrencyInput {
            stream_ids: vec![stream_id.clone()],
            operation: AdvancedOperation::MultiStreamUpdate {
                operations: vec![(
                    "counter1".to_string(),
                    StreamOperation::Increment(5)
                )]
            },
            delay_ms: None,
        };
        
        let result = command.handle(AdvancedState::default(), input).await.unwrap();
        assert_eq!(result.len(), 1);
        
        match &result[0].1 {
            AdvancedEvent::MultiStreamUpdated { operations } => {
                assert_eq!(operations.len(), 1);
                assert_eq!(operations[0].0, "counter1");
                assert!(matches!(operations[0].1, StreamOperation::Increment(5)));
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[tokio::test]
    async fn test_resource_allocation_limits() {
        let command = AdvancedConcurrencyCommand;
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Test successful allocation
        let input = AdvancedConcurrencyInput {
            stream_ids: vec![stream_id.clone()],
            operation: AdvancedOperation::AllocateResource {
                resource_type: "cpu".to_string(),
                amount: 5,
                max_capacity: 10,
            },
            delay_ms: None,
        };
        
        let result = command.handle(AdvancedState::default(), input).await.unwrap();
        assert!(matches!(result[0].1, AdvancedEvent::ResourceAllocated { .. }));
        
        // Test failed allocation due to capacity
        let mut state = AdvancedState::default();
        state.resource_allocations.insert("cpu".to_string(), 8);
        
        let input = AdvancedConcurrencyInput {
            stream_ids: vec![stream_id],
            operation: AdvancedOperation::AllocateResource {
                resource_type: "cpu".to_string(),
                amount: 5, // Would exceed capacity of 10
                max_capacity: 10,
            },
            delay_ms: None,
        };
        
        let result = command.handle(state, input).await.unwrap();
        assert!(matches!(result[0].1, AdvancedEvent::ResourceAllocationFailed { .. }));
    }
}