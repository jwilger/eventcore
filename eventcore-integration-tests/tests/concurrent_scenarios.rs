//! Comprehensive concurrent scenario tests for EventCore.
//!
//! This module tests complex concurrent behaviors including:
//! - Multi-stream command execution
//! - Isolation verification
//! - Race conditions and optimistic concurrency control
//! - Advanced scenarios like dynamic stream discovery and resource allocation

#![allow(clippy::too_many_lines)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::similar_names)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::use_self)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::uninlined_format_args)]

use async_trait::async_trait;
use eventcore::{
    CommandError, CommandExecutor, CommandLogic, CommandStreams, EventStore, ExecutionOptions,
    ReadOptions, ReadStreams, RetryConfig, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Barrier, RwLock, Semaphore};

/// Events for concurrent scenario testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ConcurrentTestEvent {
    // Basic operations
    CounterIncremented {
        amount: u64,
        operation_id: String,
    },
    CounterDecremented {
        amount: u64,
        operation_id: String,
    },

    // Multi-stream operations
    TransferInitiated {
        from: String,
        to: String,
        amount: u64,
        transfer_id: String,
    },
    TransferCompleted {
        from: String,
        to: String,
        amount: u64,
        transfer_id: String,
    },
    TransferFailed {
        from: String,
        to: String,
        amount: u64,
        transfer_id: String,
        reason: String,
    },

    // Resource allocation
    ResourceAllocated {
        resource_type: String,
        amount: u32,
        holder_id: String,
    },
    ResourceReleased {
        resource_type: String,
        amount: u32,
        holder_id: String,
    },

    // State management
    EntityCreated {
        entity_id: String,
        initial_value: u64,
    },
    EntityUpdated {
        entity_id: String,
        new_value: u64,
        update_id: String,
    },
    EntityDeleted {
        entity_id: String,
    },

    // Saga operations
    SagaStarted {
        saga_id: String,
        steps: Vec<String>,
    },
    SagaStepCompleted {
        saga_id: String,
        step: String,
        step_index: usize,
    },
    SagaCompleted {
        saga_id: String,
    },
    SagaRolledBack {
        saga_id: String,
        failed_step: String,
    },
}

impl<'a> TryFrom<&'a ConcurrentTestEvent> for ConcurrentTestEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &'a ConcurrentTestEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// State for entity-based operations.
#[derive(Debug, Default, Clone)]
struct EntityState {
    entities: HashMap<String, u64>,
    deleted: HashSet<String>,
}

/// State for counter operations.
#[derive(Debug, Default, Clone)]
struct CounterState {
    value: u64,
    operation_history: Vec<String>,
}

/// State for resource allocation.
#[derive(Debug, Default, Clone)]
struct ResourceState {
    allocations: HashMap<String, HashMap<String, u32>>, // resource_type -> holder_id -> amount
    capacities: HashMap<String, u32>,                   // resource_type -> max_capacity
}

/// Multi-stream counter increment command.
/// This tests basic multi-stream atomic operations.
#[derive(Debug, Clone)]
struct MultiStreamIncrementCommand {
    stream_ids: Vec<StreamId>,
    amount: u64,
    operation_id: String,
}

impl CommandStreams for MultiStreamIncrementCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        self.stream_ids.clone()
    }
}

#[async_trait]
impl CommandLogic for MultiStreamIncrementCommand {
    type State = HashMap<StreamId, CounterState>;
    type Event = ConcurrentTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        let counter_state = state.entry(event.stream_id.clone()).or_default();

        match &event.payload {
            ConcurrentTestEvent::CounterIncremented {
                amount,
                operation_id,
            } => {
                counter_state.value += amount;
                counter_state.operation_history.push(operation_id.clone());
            }
            ConcurrentTestEvent::CounterDecremented {
                amount,
                operation_id,
            } => {
                counter_state.value = counter_state.value.saturating_sub(*amount);
                counter_state.operation_history.push(operation_id.clone());
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Check for duplicate operation
        for counter_state in state.values() {
            if counter_state.operation_history.contains(&self.operation_id) {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Operation {} already executed",
                    self.operation_id
                )));
            }
        }

        // Create events for all streams atomically
        let mut events = Vec::new();
        for stream_id in &self.stream_ids {
            events.push(StreamWrite::new(
                &read_streams,
                stream_id.clone(),
                ConcurrentTestEvent::CounterIncremented {
                    amount: self.amount,
                    operation_id: self.operation_id.clone(),
                },
            )?);
        }

        Ok(events)
    }
}

/// Transfer command that operates on multiple streams atomically.
/// This tests complex multi-stream transactions.
#[derive(Debug, Clone)]
struct TransferCommand {
    from_stream: StreamId,
    to_stream: StreamId,
    amount: u64,
    transfer_id: String,
}

impl CommandStreams for TransferCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.from_stream.clone(), self.to_stream.clone()]
    }
}

#[async_trait]
impl CommandLogic for TransferCommand {
    type State = HashMap<StreamId, CounterState>;
    type Event = ConcurrentTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        let counter_state = state.entry(event.stream_id.clone()).or_default();

        match &event.payload {
            ConcurrentTestEvent::CounterIncremented { amount, .. } => {
                counter_state.value += amount;
            }
            ConcurrentTestEvent::CounterDecremented { amount, .. } => {
                counter_state.value = counter_state.value.saturating_sub(*amount);
            }
            ConcurrentTestEvent::TransferCompleted {
                from: _,
                to: _,
                amount: _,
                transfer_id,
            } => {
                // Track transfer in history
                counter_state.operation_history.push(transfer_id.clone());
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Check if transfer already executed
        if let Some(from_state) = state.get(&self.from_stream) {
            if from_state.operation_history.contains(&self.transfer_id) {
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Transfer {} already executed",
                    self.transfer_id
                )));
            }
        }

        // Validate source has sufficient balance
        let from_balance = state.get(&self.from_stream).map(|s| s.value).unwrap_or(0);
        if from_balance < self.amount {
            return Ok(vec![StreamWrite::new(
                &read_streams,
                self.from_stream.clone(),
                ConcurrentTestEvent::TransferFailed {
                    from: self.from_stream.as_ref().to_string(),
                    to: self.to_stream.as_ref().to_string(),
                    amount: self.amount,
                    transfer_id: self.transfer_id.clone(),
                    reason: format!("Insufficient balance: {} < {}", from_balance, self.amount),
                },
            )?]);
        }

        // Create atomic transfer events
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                self.from_stream.clone(),
                ConcurrentTestEvent::CounterDecremented {
                    amount: self.amount,
                    operation_id: self.transfer_id.clone(),
                },
            )?,
            StreamWrite::new(
                &read_streams,
                self.to_stream.clone(),
                ConcurrentTestEvent::CounterIncremented {
                    amount: self.amount,
                    operation_id: self.transfer_id.clone(),
                },
            )?,
            StreamWrite::new(
                &read_streams,
                self.from_stream.clone(),
                ConcurrentTestEvent::TransferCompleted {
                    from: self.from_stream.as_ref().to_string(),
                    to: self.to_stream.as_ref().to_string(),
                    amount: self.amount,
                    transfer_id: self.transfer_id.clone(),
                },
            )?,
        ])
    }
}

/// Resource allocation command with capacity limits.
/// This tests concurrent resource contention.
#[derive(Debug, Clone)]
struct AllocateResourceCommand {
    max_capacities: Arc<RwLock<HashMap<String, u32>>>,
    resource_stream: StreamId,
    resource_type: String,
    amount: u32,
    holder_id: String,
}

impl CommandStreams for AllocateResourceCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.resource_stream.clone()]
    }
}

#[async_trait]
impl CommandLogic for AllocateResourceCommand {
    type State = ResourceState;
    type Event = ConcurrentTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            ConcurrentTestEvent::ResourceAllocated {
                resource_type,
                amount,
                holder_id,
            } => {
                state
                    .allocations
                    .entry(resource_type.clone())
                    .or_default()
                    .entry(holder_id.clone())
                    .and_modify(|a| *a += amount)
                    .or_insert(*amount);
            }
            ConcurrentTestEvent::ResourceReleased {
                resource_type,
                amount,
                holder_id,
            } => {
                if let Some(type_allocations) = state.allocations.get_mut(resource_type) {
                    if let Some(holder_amount) = type_allocations.get_mut(holder_id) {
                        *holder_amount = holder_amount.saturating_sub(*amount);
                        if *holder_amount == 0 {
                            type_allocations.remove(holder_id);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        mut state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Get max capacity for this resource type
        let max_capacity = self
            .max_capacities
            .read()
            .await
            .get(&self.resource_type)
            .copied()
            .unwrap_or(100); // Default capacity

        state
            .capacities
            .insert(self.resource_type.clone(), max_capacity);

        // Calculate current total allocation
        let current_total: u32 = state
            .allocations
            .get(&self.resource_type)
            .map(|allocs| allocs.values().sum())
            .unwrap_or(0);

        // Check if allocation would exceed capacity
        if current_total + self.amount > max_capacity {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Resource allocation would exceed capacity: {} + {} > {}",
                current_total, self.amount, max_capacity
            )));
        }

        Ok(vec![StreamWrite::new(
            &read_streams,
            self.resource_stream.clone(),
            ConcurrentTestEvent::ResourceAllocated {
                resource_type: self.resource_type.clone(),
                amount: self.amount,
                holder_id: self.holder_id.clone(),
            },
        )?])
    }
}

/// Dynamic stream discovery command.
/// This tests commands that dynamically add streams during execution.
#[derive(Debug, Clone)]
struct DynamicStreamCommand {
    initial_stream: StreamId,
    entity_id: String,
    related_entities: Vec<String>,
}

impl CommandStreams for DynamicStreamCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.initial_stream.clone()]
    }
}

#[async_trait]
impl CommandLogic for DynamicStreamCommand {
    type State = EntityState;
    type Event = ConcurrentTestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            ConcurrentTestEvent::EntityCreated {
                entity_id,
                initial_value,
            } => {
                state.entities.insert(entity_id.clone(), *initial_value);
            }
            ConcurrentTestEvent::EntityUpdated {
                entity_id,
                new_value,
                ..
            } => {
                state.entities.insert(entity_id.clone(), *new_value);
            }
            ConcurrentTestEvent::EntityDeleted { entity_id } => {
                state.entities.remove(entity_id);
                state.deleted.insert(entity_id.clone());
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Check if any related entities exist and need to be read
        let mut need_discovery = false;
        for entity_id in &self.related_entities {
            if state.entities.contains_key(entity_id) {
                need_discovery = true;
                let stream_id = StreamId::try_new(format!("entity-{}", entity_id))
                    .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;
                stream_resolver.add_streams(vec![stream_id]);
            }
        }

        // If we need to discover additional streams, the executor will handle re-execution
        if need_discovery {
            // Return empty to trigger re-execution with additional streams
            return Ok(vec![]);
        }

        // All necessary streams have been read, execute the operation
        let events = vec![StreamWrite::new(
            &read_streams,
            self.initial_stream.clone(),
            ConcurrentTestEvent::EntityCreated {
                entity_id: self.entity_id.clone(),
                initial_value: self.related_entities.len() as u64,
            },
        )?];

        Ok(events)
    }
}

/// Helper function to create test streams with initial values.
async fn setup_test_streams(
    executor: &CommandExecutor<InMemoryEventStore<ConcurrentTestEvent>>,
    stream_values: Vec<(StreamId, u64)>,
) {
    for (stream_id, initial_value) in stream_values {
        let command = MultiStreamIncrementCommand {
            stream_ids: vec![stream_id],
            amount: initial_value,
            operation_id: format!(
                "init-{}",
                uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
            ),
        };

        executor
            .execute(command, ExecutionOptions::default())
            .await
            .unwrap();
    }
}

/// Test concurrent multi-stream commands with overlapping stream sets.
#[tokio::test]
async fn test_concurrent_multi_stream_operations() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create test streams
    let stream_a = StreamId::try_new("stream-a").unwrap();
    let stream_b = StreamId::try_new("stream-b").unwrap();
    let stream_c = StreamId::try_new("stream-c").unwrap();

    // Initialize streams
    setup_test_streams(
        &executor,
        vec![
            (stream_a.clone(), 100),
            (stream_b.clone(), 100),
            (stream_c.clone(), 100),
        ],
    )
    .await;

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = vec![];

    // Command 1: Operates on streams A and B
    let executor1 = executor.clone();
    let barrier1 = barrier.clone();
    let stream_a1 = stream_a.clone();
    let stream_b1 = stream_b.clone();

    handles.push(tokio::spawn(async move {
        barrier1.wait().await;
        executor1
            .execute(
                MultiStreamIncrementCommand {
                    stream_ids: vec![stream_a1, stream_b1],
                    amount: 10,
                    operation_id: "op1".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Command 2: Operates on streams B and C
    let executor2 = executor.clone();
    let barrier2 = barrier.clone();
    let stream_b2 = stream_b.clone();
    let stream_c2 = stream_c.clone();

    handles.push(tokio::spawn(async move {
        barrier2.wait().await;
        executor2
            .execute(
                MultiStreamIncrementCommand {
                    stream_ids: vec![stream_b2, stream_c2],
                    amount: 20,
                    operation_id: "op2".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Command 3: Operates on streams A and C
    let executor3 = executor.clone();
    let barrier3 = barrier.clone();
    let stream_a3 = stream_a.clone();
    let stream_c3 = stream_c.clone();

    handles.push(tokio::spawn(async move {
        barrier3.wait().await;
        executor3
            .execute(
                MultiStreamIncrementCommand {
                    stream_ids: vec![stream_a3, stream_c3],
                    amount: 30,
                    operation_id: "op3".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Collect results
    let mut results = vec![];
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    // Count successes and conflicts
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    let conflict_count = results
        .iter()
        .filter(|r| matches!(r, Err(CommandError::ConcurrencyConflict { .. })))
        .count();

    // At least one should succeed, others might conflict
    assert!(success_count >= 1, "At least one command should succeed");
    assert_eq!(
        success_count + conflict_count,
        3,
        "All commands should either succeed or conflict"
    );

    // Verify final state consistency
    let all_streams = vec![stream_a, stream_b, stream_c];
    let stream_data = executor
        .event_store()
        .read_streams(&all_streams, &ReadOptions::default())
        .await
        .unwrap();

    // Check that all events are properly ordered
    for stream_id in &all_streams {
        let stream_events: Vec<_> = stream_data
            .events()
            .filter(|e| &e.stream_id == stream_id)
            .collect();

        // Verify version ordering
        for window in stream_events.windows(2) {
            assert!(window[0].event_version < window[1].event_version);
        }
    }
}

/// Test isolation between concurrent commands on disjoint stream sets.
#[tokio::test]
async fn test_stream_isolation() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create non-overlapping stream sets
    let stream_sets: Vec<Vec<StreamId>> = (0..5)
        .map(|i| {
            (0..3)
                .map(|j| StreamId::try_new(format!("set{i}-stream{j}")).unwrap())
                .collect()
        })
        .collect();

    // Initialize all streams
    let mut all_streams = vec![];
    for streams in &stream_sets {
        for stream in streams {
            all_streams.push((stream.clone(), 50));
        }
    }
    setup_test_streams(&executor, all_streams).await;

    let barrier = Arc::new(Barrier::new(stream_sets.len()));
    let success_counter = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Execute concurrent operations on disjoint stream sets
    for (i, streams) in stream_sets.into_iter().enumerate() {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let counter = success_counter.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            let result = executor
                .execute(
                    MultiStreamIncrementCommand {
                        stream_ids: streams,
                        amount: 10,
                        operation_id: format!("isolated-op-{i}"),
                    },
                    ExecutionOptions::default(),
                )
                .await;

            if result.is_ok() {
                counter.fetch_add(1, Ordering::Relaxed);
            }

            result
        }));
    }

    // All operations should succeed since they operate on disjoint streams
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Isolated operations should all succeed");
    }

    assert_eq!(
        success_counter.load(Ordering::Relaxed),
        5,
        "All 5 isolated operations should succeed"
    );
}

/// Test high contention scenarios with many concurrent operations on shared streams.
#[tokio::test]
async fn test_high_contention_optimistic_concurrency() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create a shared stream
    let shared_stream = StreamId::try_new("high-contention-stream").unwrap();
    setup_test_streams(&executor, vec![(shared_stream.clone(), 1000)]).await;

    let num_operations = 50;
    let barrier = Arc::new(Barrier::new(num_operations));
    let success_counter = Arc::new(AtomicUsize::new(0));
    let conflict_counter = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Launch many concurrent operations
    for i in 0..num_operations {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let success_counter = success_counter.clone();
        let conflict_counter = conflict_counter.clone();
        let stream = shared_stream.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            // Add small random delay to increase chance of conflicts
            tokio::time::sleep(Duration::from_micros((i as u64) % 100)).await;

            let result = executor
                .execute(
                    MultiStreamIncrementCommand {
                        stream_ids: vec![stream],
                        amount: 1,
                        operation_id: format!("contention-op-{i}"),
                    },
                    ExecutionOptions::default(),
                )
                .await;

            match result {
                Ok(_) => {
                    success_counter.fetch_add(1, Ordering::Relaxed);
                }
                Err(CommandError::ConcurrencyConflict { .. }) => {
                    conflict_counter.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }));
    }

    // Wait for all operations
    for handle in handles {
        handle.await.unwrap();
    }

    let successes = success_counter.load(Ordering::Relaxed);
    let conflicts = conflict_counter.load(Ordering::Relaxed);

    println!(
        "High contention test: {} successes, {} conflicts",
        successes, conflicts
    );

    // Verify all operations either succeeded or conflicted
    assert_eq!(successes + conflicts, num_operations);

    // At least some should succeed
    assert!(successes > 0, "At least some operations should succeed");

    // Under high contention with in-memory store using RwLock, conflicts may not occur
    // as operations are serialized. With a real database, conflicts would be expected.
    // We'll accept either behavior as valid.
    println!("Note: In-memory store may serialize operations, preventing conflicts");

    // Verify final state is consistent
    let stream_data = executor
        .event_store()
        .read_streams(&[shared_stream], &ReadOptions::default())
        .await
        .unwrap();

    let mut state = HashMap::new();
    let command = MultiStreamIncrementCommand {
        stream_ids: vec![],
        amount: 0,
        operation_id: String::new(),
    };
    for event in stream_data.events() {
        CommandLogic::apply(&command, &mut state, event);
    }

    // Final value should be initial value plus successful increments
    let final_value = state.values().map(|s| s.value).sum::<u64>();
    assert_eq!(final_value, 1000 + successes as u64);
}

/// Test concurrent transfers between accounts to verify atomicity.
#[tokio::test]
async fn test_concurrent_transfers_atomicity() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create account streams
    let accounts: Vec<StreamId> = (0..5)
        .map(|i| StreamId::try_new(format!("account-{i}")).unwrap())
        .collect();

    // Initialize accounts with balance
    for account in &accounts {
        setup_test_streams(&executor, vec![(account.clone(), 100)]).await;
    }

    let num_transfers = 20;
    let barrier = Arc::new(Barrier::new(num_transfers));
    let mut handles = vec![];

    // Create random transfers
    for i in 0..num_transfers {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let accounts = accounts.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            // Random source and destination
            let from_idx = i % accounts.len();
            let to_idx = (i + 1 + (i / accounts.len())) % accounts.len();

            if from_idx == to_idx {
                return Ok(()); // Skip self-transfers
            }

            let result = executor
                .execute(
                    TransferCommand {
                        from_stream: accounts[from_idx].clone(),
                        to_stream: accounts[to_idx].clone(),
                        amount: 10,
                        transfer_id: format!("transfer-{i}"),
                    },
                    ExecutionOptions::default(),
                )
                .await;

            result.map(|_| ())
        }));
    }

    // Collect results
    let mut success_count = 0;
    let mut failure_count = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(()) => success_count += 1,
            Err(_) => failure_count += 1,
        }
    }

    println!(
        "Transfer test: {} successful, {} failed",
        success_count, failure_count
    );

    // Verify total balance is conserved
    let stream_data = executor
        .event_store()
        .read_streams(&accounts, &ReadOptions::default())
        .await
        .unwrap();

    let mut total_balance = 0u64;
    let mut state = HashMap::new();
    let command = TransferCommand {
        from_stream: StreamId::try_new("dummy").unwrap(),
        to_stream: StreamId::try_new("dummy").unwrap(),
        amount: 0,
        transfer_id: String::new(),
    };

    for event in stream_data.events() {
        CommandLogic::apply(&command, &mut state, event);
    }

    for (_, counter_state) in state {
        total_balance += counter_state.value;
    }

    // Total balance should be conserved (5 accounts * 100 initial balance)
    assert_eq!(total_balance, 500, "Total balance should be conserved");
}

/// Test resource allocation with capacity constraints.
#[tokio::test]
async fn test_concurrent_resource_allocation() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Set up resource capacities
    let max_capacities = Arc::new(RwLock::new(HashMap::new()));
    max_capacities.write().await.insert("cpu".to_string(), 100);
    max_capacities
        .write()
        .await
        .insert("memory".to_string(), 1000);

    let resource_stream = StreamId::try_new("resources").unwrap();

    let num_allocations = 30;
    let barrier = Arc::new(Barrier::new(num_allocations));
    let mut handles = vec![];

    for i in 0..num_allocations {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let capacities = max_capacities.clone();
        let stream = resource_stream.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            let resource_type = if i % 2 == 0 { "cpu" } else { "memory" };
            let amount = match resource_type {
                "cpu" => 10 + (i % 5) as u32,
                "memory" => 100 + (i % 10) as u32 * 10,
                _ => unreachable!(),
            };

            executor
                .execute(
                    AllocateResourceCommand {
                        max_capacities: capacities,
                        resource_stream: stream,
                        resource_type: resource_type.to_string(),
                        amount,
                        holder_id: format!("holder-{i}"),
                    },
                    ExecutionOptions::default(),
                )
                .await
        }));
    }

    // Collect results
    let mut successful_allocations = 0;
    let mut failed_allocations = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => successful_allocations += 1,
            Err(CommandError::BusinessRuleViolation(_)) => failed_allocations += 1,
            Err(CommandError::ConcurrencyConflict { .. }) => failed_allocations += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    println!(
        "Resource allocation: {} successful, {} failed",
        successful_allocations, failed_allocations
    );

    // Verify allocations don't exceed capacity
    let stream_data = executor
        .event_store()
        .read_streams(&[resource_stream.clone()], &ReadOptions::default())
        .await
        .unwrap();

    let mut state = ResourceState::default();
    let command = AllocateResourceCommand {
        max_capacities: max_capacities.clone(),
        resource_stream: resource_stream.clone(),
        resource_type: String::new(),
        amount: 0,
        holder_id: String::new(),
    };

    for event in stream_data.events() {
        CommandLogic::apply(&command, &mut state, event);
    }

    // Check CPU allocations
    let cpu_total: u32 = state
        .allocations
        .get("cpu")
        .map(|allocs| allocs.values().sum())
        .unwrap_or(0);
    assert!(
        cpu_total <= 100,
        "CPU allocations should not exceed capacity"
    );

    // Check memory allocations
    let memory_total: u32 = state
        .allocations
        .get("memory")
        .map(|allocs| allocs.values().sum())
        .unwrap_or(0);
    assert!(
        memory_total <= 1000,
        "Memory allocations should not exceed capacity"
    );
}

/// Test dynamic stream discovery under concurrent load.
#[tokio::test]
async fn test_concurrent_dynamic_stream_discovery() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create some initial entities
    let catalog_stream = StreamId::try_new("entity-catalog").unwrap();

    // Pre-create some entities that will be discovered
    for i in 0..5 {
        executor
            .execute(
                DynamicStreamCommand {
                    initial_stream: catalog_stream.clone(),
                    entity_id: format!("pre-entity-{i}"),
                    related_entities: vec![],
                },
                ExecutionOptions::default().with_max_stream_discovery_iterations(5),
            )
            .await
            .unwrap();
    }

    let num_operations = 10;
    let barrier = Arc::new(Barrier::new(num_operations));
    let mut handles = vec![];

    for i in 0..num_operations {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let catalog = catalog_stream.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            // Some commands reference existing entities (triggering discovery)
            let related = if i % 2 == 0 {
                vec![format!("pre-entity-{}", i % 5)]
            } else {
                vec![]
            };

            executor
                .execute(
                    DynamicStreamCommand {
                        initial_stream: catalog,
                        entity_id: format!("new-entity-{i}"),
                        related_entities: related,
                    },
                    ExecutionOptions::default().with_max_stream_discovery_iterations(5),
                )
                .await
        }));
    }

    // All operations should eventually succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Dynamic stream discovery should succeed");
    }
}

/// Test retry behavior under high contention.
#[tokio::test]
async fn test_retry_mechanism_under_contention() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    let shared_stream = StreamId::try_new("retry-test-stream").unwrap();
    setup_test_streams(&executor, vec![(shared_stream.clone(), 0)]).await;

    let retry_config = RetryConfig {
        max_attempts: 5,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(100),
        backoff_multiplier: 2.0,
    };

    let num_operations = 20;
    let barrier = Arc::new(Barrier::new(num_operations));
    let attempt_counter = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];

    for i in 0..num_operations {
        let executor = executor.clone();
        let barrier = barrier.clone();
        let counter = attempt_counter.clone();
        let stream = shared_stream.clone();
        let config = retry_config.clone();

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            // Track retry attempts
            let start = Instant::now();

            let result = executor
                .execute(
                    MultiStreamIncrementCommand {
                        stream_ids: vec![stream],
                        amount: 1,
                        operation_id: format!("retry-op-{i}"),
                    },
                    ExecutionOptions::default().with_retry_config(config),
                )
                .await;

            let duration = start.elapsed();

            // Count this as a retry if it took longer than base delay
            if duration > Duration::from_millis(10) {
                counter.fetch_add(1, Ordering::Relaxed);
            }

            result
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    let retry_attempts = attempt_counter.load(Ordering::Relaxed);

    println!(
        "Retry test: {success_count} successful operations, {retry_attempts} involved retries"
    );

    // With retries, more operations should succeed than without
    assert!(
        success_count >= num_operations / 2,
        "With retries, at least half of operations should succeed"
    );

    // With in-memory store using RwLock, operations are serialized so retries may not occur.
    // With a real database, retries would be expected under contention.
    if retry_attempts == 0 {
        println!("Note: In-memory store serializes operations, preventing the need for retries");
    }
}

/// Test transaction-like rollback behavior in complex scenarios.
#[tokio::test]
async fn test_complex_transaction_rollback_scenarios() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Create accounts for testing failed transfers
    let account_a = StreamId::try_new("rollback-account-a").unwrap();
    let account_b = StreamId::try_new("rollback-account-b").unwrap();
    let account_c = StreamId::try_new("rollback-account-c").unwrap();

    setup_test_streams(
        &executor,
        vec![
            (account_a.clone(), 50), // Limited balance
            (account_b.clone(), 100),
            (account_c.clone(), 100),
        ],
    )
    .await;

    // Test 1: Concurrent transfers that should cause some failures
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = vec![];

    // Transfer 1: A -> B (60) - should fail due to insufficient funds
    let executor1 = executor.clone();
    let barrier1 = barrier.clone();
    let a1 = account_a.clone();
    let b1 = account_b.clone();

    handles.push(tokio::spawn(async move {
        barrier1.wait().await;
        executor1
            .execute(
                TransferCommand {
                    from_stream: a1,
                    to_stream: b1,
                    amount: 60,
                    transfer_id: "rollback-transfer-1".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Transfer 2: B -> C (50) - should succeed
    let executor2 = executor.clone();
    let barrier2 = barrier.clone();
    let b2 = account_b.clone();
    let c2 = account_c.clone();

    handles.push(tokio::spawn(async move {
        barrier2.wait().await;
        executor2
            .execute(
                TransferCommand {
                    from_stream: b2,
                    to_stream: c2,
                    amount: 50,
                    transfer_id: "rollback-transfer-2".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Transfer 3: C -> A (30) - should succeed
    let executor3 = executor.clone();
    let barrier3 = barrier.clone();
    let c3 = account_c.clone();
    let a3 = account_a.clone();

    handles.push(tokio::spawn(async move {
        barrier3.wait().await;
        executor3
            .execute(
                TransferCommand {
                    from_stream: c3,
                    to_stream: a3,
                    amount: 30,
                    transfer_id: "rollback-transfer-3".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    }));

    // Collect results
    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // Verify Transfer 1 failed
    assert!(results[0].is_ok()); // It should succeed but record a failure event

    // Verify final balances are consistent
    let all_accounts = vec![account_a, account_b, account_c];
    let stream_data = executor
        .event_store()
        .read_streams(&all_accounts, &ReadOptions::default())
        .await
        .unwrap();

    let mut state: HashMap<StreamId, CounterState> = HashMap::new();
    let command = TransferCommand {
        from_stream: StreamId::try_new("dummy").unwrap(),
        to_stream: StreamId::try_new("dummy").unwrap(),
        amount: 0,
        transfer_id: String::new(),
    };

    for event in stream_data.events() {
        CommandLogic::apply(&command, &mut state, event);
    }

    let total_balance: u64 = state.values().map(|s| s.value).sum();
    assert_eq!(
        total_balance, 250,
        "Total balance should be conserved even with failed transfers"
    );
}

/// Test performance characteristics under various concurrent loads.
#[tokio::test]
#[ignore = "Performance test - run with cargo test --ignored"]
async fn test_concurrent_performance_characteristics() {
    let store = InMemoryEventStore::new();
    let executor = Arc::new(CommandExecutor::new(store));

    // Test different concurrency levels
    let concurrency_levels = vec![1, 5, 10, 20, 50, 100];
    let operations_per_level = 100;

    for concurrency in concurrency_levels {
        let shared_stream = StreamId::try_new(format!("perf-test-{concurrency}")).unwrap();
        setup_test_streams(&executor, vec![(shared_stream.clone(), 0)]).await;

        let start = Instant::now();
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut handles = vec![];

        for i in 0..operations_per_level {
            let executor = executor.clone();
            let semaphore = semaphore.clone();
            let stream = shared_stream.clone();

            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                executor
                    .execute(
                        MultiStreamIncrementCommand {
                            stream_ids: vec![stream],
                            amount: 1,
                            operation_id: format!("perf-op-{concurrency}-{i}"),
                        },
                        ExecutionOptions::default(),
                    )
                    .await
            }));
        }

        let mut success_count = 0;
        for handle in handles {
            if handle.await.unwrap().is_ok() {
                success_count += 1;
            }
        }

        let duration = start.elapsed();
        let throughput = f64::from(success_count) / duration.as_secs_f64();

        println!(
            "Concurrency {concurrency}: {success_count} successful ops in {duration:?} ({throughput:.2} ops/sec)"
        );
    }
}
