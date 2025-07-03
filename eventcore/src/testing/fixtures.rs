//! Common test fixtures and data factories.
//!
//! This module provides pre-configured test data and scenarios commonly needed
//! in event sourcing tests.

use crate::command::{CommandLogic, CommandResult, CommandStreams, ReadStreams, StreamWrite};
use crate::errors::CommandError;
use crate::event_store::{
    EventStore, EventToWrite, ReadOptions, StoredEvent as StoreStoredEvent, StreamData,
    StreamEvents,
};
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A simple test event type for use in examples and tests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TestEvent {
    /// Something was created
    Created {
        /// The ID of the created item
        id: String,
        /// The name of the created item
        name: String,
    },
    /// Something was updated
    Updated {
        /// The ID of the updated item
        id: String,
        /// The new name of the item
        name: String,
    },
    /// Something was deleted
    Deleted {
        /// The ID of the deleted item
        id: String,
    },
    /// A counter was incremented
    Incremented {
        /// The amount incremented
        amount: i32,
    },
    /// A counter was decremented
    Decremented {
        /// The amount decremented
        amount: i32,
    },
}

/// A simple test command for demonstrating the command pattern.
#[derive(Clone)]
pub struct TestCommand {
    /// The stream to operate on
    pub stream_id: StreamId,
    /// The action to perform
    pub action: TestAction,
}

/// Actions that the test command can perform.
#[derive(Debug, Clone)]
pub enum TestAction {
    /// Create something
    Create {
        /// The ID of the item to create
        id: String,
        /// The name of the item to create
        name: String,
    },
    /// Update something
    Update {
        /// The ID of the item to update
        id: String,
        /// The new name for the item
        name: String,
    },
    /// Delete something
    Delete {
        /// The ID of the item to delete
        id: String,
    },
    /// Increment a counter
    Increment {
        /// The amount to increment by
        amount: i32,
    },
    /// Decrement a counter (can fail if would go negative)
    Decrement {
        /// The amount to decrement by
        amount: i32,
    },
}

/// State for the test command.
#[derive(Debug, Clone, Default)]
pub struct TestState {
    /// Current counter value
    pub counter: i32,
    /// Map of created items
    pub items: HashMap<String, String>,
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
    type Event = TestEvent;

    fn apply(
        &self,
        state: &mut Self::State,
        stored_event: &crate::event_store::StoredEvent<Self::Event>,
    ) {
        match &stored_event.payload {
            TestEvent::Created { id, name } | TestEvent::Updated { id, name } => {
                state.items.insert(id.clone(), name.clone());
            }
            TestEvent::Deleted { id } => {
                state.items.remove(id);
            }
            TestEvent::Incremented { amount } => {
                state.counter += amount;
            }
            TestEvent::Decremented { amount } => {
                state.counter -= amount;
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut crate::command::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let event = match &self.action {
            TestAction::Create { id, name } => {
                if state.items.contains_key(id) {
                    return Err(CommandError::BusinessRuleViolation(format!(
                        "Item {id} already exists"
                    )));
                }
                TestEvent::Created {
                    id: id.clone(),
                    name: name.clone(),
                }
            }
            TestAction::Update { id, name } => {
                if !state.items.contains_key(id) {
                    return Err(CommandError::BusinessRuleViolation(format!(
                        "Item {id} does not exist"
                    )));
                }
                TestEvent::Updated {
                    id: id.clone(),
                    name: name.clone(),
                }
            }
            TestAction::Delete { id } => {
                if !state.items.contains_key(id) {
                    return Err(CommandError::BusinessRuleViolation(format!(
                        "Item {id} does not exist"
                    )));
                }
                TestEvent::Deleted { id: id.clone() }
            }
            TestAction::Increment { amount } => TestEvent::Incremented { amount: *amount },
            TestAction::Decrement { amount } => {
                if state.counter - amount < 0 {
                    return Err(CommandError::BusinessRuleViolation(
                        "Counter cannot go negative".to_string(),
                    ));
                }
                TestEvent::Decremented { amount: *amount }
            }
        };

        Ok(vec![StreamWrite::new(
            &read_streams,
            self.stream_id.clone(),
            event,
        )?])
    }
}

/// A test event store that always fails operations.
///
/// Useful for testing error handling.
#[derive(Clone)]
pub struct FailingEventStore<E> {
    _phantom: std::marker::PhantomData<E>,
}

impl<E> FailingEventStore<E> {
    /// Creates a new failing event store.
    pub const fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<E> Default for FailingEventStore<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<E> EventStore for FailingEventStore<E>
where
    E: Send + Sync + 'static,
{
    type Event = E;

    async fn read_streams(
        &self,
        _stream_ids: &[StreamId],
        _options: &ReadOptions,
    ) -> crate::errors::EventStoreResult<StreamData<Self::Event>> {
        Err(crate::errors::EventStoreError::Unavailable(
            "Event store is failing".to_string(),
        ))
    }

    async fn write_events_multi(
        &self,
        _stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> crate::errors::EventStoreResult<HashMap<StreamId, EventVersion>> {
        Err(crate::errors::EventStoreError::Unavailable(
            "Event store is failing".to_string(),
        ))
    }

    async fn stream_exists(&self, _stream_id: &StreamId) -> crate::errors::EventStoreResult<bool> {
        Err(crate::errors::EventStoreError::Unavailable(
            "Event store is failing".to_string(),
        ))
    }

    async fn get_stream_version(
        &self,
        _stream_id: &StreamId,
    ) -> crate::errors::EventStoreResult<Option<EventVersion>> {
        Err(crate::errors::EventStoreError::Unavailable(
            "Event store is failing".to_string(),
        ))
    }

    async fn subscribe(
        &self,
        _options: crate::subscription::SubscriptionOptions,
    ) -> crate::errors::EventStoreResult<
        Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
    > {
        Err(crate::errors::EventStoreError::Unavailable(
            "Event store is failing".to_string(),
        ))
    }
}

/// A counting event store that tracks operation calls.
///
/// Useful for verifying that operations are called the expected number of times.
#[derive(Clone)]
pub struct CountingEventStore<E> {
    read_count: Arc<AtomicUsize>,
    write_count: Arc<AtomicUsize>,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> CountingEventStore<E> {
    /// Creates a new counting event store.
    pub fn new() -> Self {
        Self {
            read_count: Arc::new(AtomicUsize::new(0)),
            write_count: Arc::new(AtomicUsize::new(0)),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the number of read operations.
    pub fn read_count(&self) -> usize {
        self.read_count.load(Ordering::SeqCst)
    }

    /// Returns the number of write operations.
    pub fn write_count(&self) -> usize {
        self.write_count.load(Ordering::SeqCst)
    }

    /// Resets all counters to zero.
    pub fn reset_counts(&self) {
        self.read_count.store(0, Ordering::SeqCst);
        self.write_count.store(0, Ordering::SeqCst);
    }
}

impl<E> Default for CountingEventStore<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<E> EventStore for CountingEventStore<E>
where
    E: Send + Sync + Clone + 'static,
{
    type Event = E;

    async fn read_streams(
        &self,
        _stream_ids: &[StreamId],
        _options: &ReadOptions,
    ) -> crate::errors::EventStoreResult<StreamData<Self::Event>> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        Ok(StreamData::new(vec![], HashMap::new()))
    }

    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> crate::errors::EventStoreResult<HashMap<StreamId, EventVersion>> {
        self.write_count.fetch_add(1, Ordering::SeqCst);

        let mut versions = HashMap::new();
        for stream in stream_events {
            let version = EventVersion::try_new(stream.events.len() as u64).unwrap();
            versions.insert(stream.stream_id, version);
        }

        Ok(versions)
    }

    async fn stream_exists(&self, _stream_id: &StreamId) -> crate::errors::EventStoreResult<bool> {
        Ok(false)
    }

    async fn get_stream_version(
        &self,
        _stream_id: &StreamId,
    ) -> crate::errors::EventStoreResult<Option<EventVersion>> {
        Ok(None)
    }

    async fn subscribe(
        &self,
        _options: crate::subscription::SubscriptionOptions,
    ) -> crate::errors::EventStoreResult<
        Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
    > {
        let subscription = crate::subscription::SubscriptionImpl::new();
        Ok(Box::new(subscription))
    }
}

/// Creates a standard test scenario with multiple streams and events.
///
/// Returns a map of stream IDs to their events.
pub fn create_test_scenario() -> HashMap<StreamId, Vec<StoreStoredEvent<TestEvent>>> {
    let mut scenario = HashMap::new();

    // Account stream with balance operations
    let account_stream = StreamId::try_new("account-123").unwrap();
    let account_events = vec![
        StoreStoredEvent::new(
            EventId::new(),
            account_stream.clone(),
            EventVersion::try_new(1).unwrap(),
            Timestamp::now(),
            TestEvent::Created {
                id: "account-123".to_string(),
                name: "Test Account".to_string(),
            },
            None,
        ),
        StoreStoredEvent::new(
            EventId::new(),
            account_stream.clone(),
            EventVersion::try_new(2).unwrap(),
            Timestamp::now(),
            TestEvent::Incremented { amount: 100 },
            None,
        ),
        StoreStoredEvent::new(
            EventId::new(),
            account_stream.clone(),
            EventVersion::try_new(3).unwrap(),
            Timestamp::now(),
            TestEvent::Decremented { amount: 30 },
            None,
        ),
    ];
    scenario.insert(account_stream, account_events);

    // Product stream with CRUD operations
    let product_stream = StreamId::try_new("product-456").unwrap();
    let product_events = vec![
        StoreStoredEvent::new(
            EventId::new(),
            product_stream.clone(),
            EventVersion::try_new(1).unwrap(),
            Timestamp::now(),
            TestEvent::Created {
                id: "product-456".to_string(),
                name: "Test Product".to_string(),
            },
            None,
        ),
        StoreStoredEvent::new(
            EventId::new(),
            product_stream.clone(),
            EventVersion::try_new(2).unwrap(),
            Timestamp::now(),
            TestEvent::Updated {
                id: "product-456".to_string(),
                name: "Updated Product".to_string(),
            },
            None,
        ),
    ];
    scenario.insert(product_stream, product_events);

    scenario
}

/// Creates a test command for common scenarios.
pub fn create_test_command_input(scenario: &str) -> TestCommand {
    match scenario {
        "create" => TestCommand {
            stream_id: StreamId::try_new("test-stream").unwrap(),
            action: TestAction::Create {
                id: "item-1".to_string(),
                name: "Test Item".to_string(),
            },
        },
        "update" => TestCommand {
            stream_id: StreamId::try_new("test-stream").unwrap(),
            action: TestAction::Update {
                id: "item-1".to_string(),
                name: "Updated Item".to_string(),
            },
        },
        "delete" => TestCommand {
            stream_id: StreamId::try_new("test-stream").unwrap(),
            action: TestAction::Delete {
                id: "item-1".to_string(),
            },
        },
        "increment" => TestCommand {
            stream_id: StreamId::try_new("counter-stream").unwrap(),
            action: TestAction::Increment { amount: 10 },
        },
        "decrement" => TestCommand {
            stream_id: StreamId::try_new("counter-stream").unwrap(),
            action: TestAction::Decrement { amount: 5 },
        },
        _ => panic!("Unknown scenario: {scenario}"),
    }
}

/// Creates a batch of events for performance testing.
pub fn create_large_event_batch(count: usize) -> Vec<EventToWrite<TestEvent>> {
    (0..count)
        .map(|i| {
            let event = if i % 3 == 0 {
                TestEvent::Created {
                    id: format!("item-{i}"),
                    name: format!("Item {i}"),
                }
            } else if i % 3 == 1 {
                TestEvent::Updated {
                    id: format!("item-{i}"),
                    name: format!("Updated Item {i}"),
                }
            } else {
                TestEvent::Incremented {
                    amount: i32::try_from(i).unwrap_or(i32::MAX),
                }
            };

            EventToWrite::new(EventId::new(), event)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_command_create_success() {
        let command = create_test_command_input("create");
        let state = TestState::default();

        let read_streams = ReadStreams::new(vec![command.stream_id.clone()]);
        let mut stream_resolver = crate::command::StreamResolver::new();
        let stream_writes = command
            .handle(read_streams, state, &mut stream_resolver)
            .await
            .unwrap();
        let result: Vec<(StreamId, TestEvent)> = stream_writes
            .into_iter()
            .map(crate::command::StreamWrite::into_parts)
            .collect();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, command.stream_id);
        matches!(result[0].1, TestEvent::Created { .. });
    }

    #[tokio::test]
    async fn test_command_create_duplicate_fails() {
        let input = create_test_command_input("create");
        let command = TestCommand {
            stream_id: input.stream_id.clone(),
            action: input.action,
        };
        let mut state = TestState::default();
        state
            .items
            .insert("item-1".to_string(), "Existing".to_string());

        let read_streams = ReadStreams::new(vec![command.stream_id.clone()]);
        let mut stream_resolver = crate::command::StreamResolver::new();
        let result = command
            .handle(read_streams, state, &mut stream_resolver)
            .await;

        assert!(matches!(
            result,
            Err(CommandError::BusinessRuleViolation(_))
        ));
    }

    #[tokio::test]
    async fn test_command_decrement_negative_fails() {
        let input = create_test_command_input("decrement");
        let command = TestCommand {
            stream_id: input.stream_id.clone(),
            action: input.action,
        };
        let state = TestState::default(); // Counter starts at 0

        let read_streams = ReadStreams::new(vec![command.stream_id.clone()]);
        let mut stream_resolver = crate::command::StreamResolver::new();
        let result = command
            .handle(read_streams, state, &mut stream_resolver)
            .await;

        assert!(matches!(
            result,
            Err(CommandError::BusinessRuleViolation(_))
        ));
    }

    #[tokio::test]
    async fn test_failing_event_store() {
        let store: FailingEventStore<TestEvent> = FailingEventStore::new();

        let result = store.read_streams(&[], &ReadOptions::new()).await;
        assert!(result.is_err());

        let result = store.write_events_multi(vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_counting_event_store() {
        let store: CountingEventStore<TestEvent> = CountingEventStore::new();

        assert_eq!(store.read_count(), 0);
        assert_eq!(store.write_count(), 0);

        let _ = store.read_streams(&[], &ReadOptions::new()).await;
        assert_eq!(store.read_count(), 1);

        let _ = store.write_events_multi(vec![]).await;
        assert_eq!(store.write_count(), 1);

        store.reset_counts();
        assert_eq!(store.read_count(), 0);
        assert_eq!(store.write_count(), 0);
    }

    #[test]
    fn test_create_test_scenario() {
        let scenario = create_test_scenario();

        assert_eq!(scenario.len(), 2);
        assert!(scenario.contains_key(&StreamId::try_new("account-123").unwrap()));
        assert!(scenario.contains_key(&StreamId::try_new("product-456").unwrap()));

        let account_events = &scenario[&StreamId::try_new("account-123").unwrap()];
        assert_eq!(account_events.len(), 3);
    }

    #[test]
    fn test_create_large_event_batch() {
        let batch = create_large_event_batch(100);

        assert_eq!(batch.len(), 100);

        // Verify distribution of event types
        let created_count = batch
            .iter()
            .filter(|e| matches!(e.payload, TestEvent::Created { .. }))
            .count();
        let updated_count = batch
            .iter()
            .filter(|e| matches!(e.payload, TestEvent::Updated { .. }))
            .count();
        let incremented_count = batch
            .iter()
            .filter(|e| matches!(e.payload, TestEvent::Incremented { .. }))
            .count();

        assert!(created_count > 30);
        assert!(updated_count > 30);
        assert!(incremented_count > 30);
    }
}
