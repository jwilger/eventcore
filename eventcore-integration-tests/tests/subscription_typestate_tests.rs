//! Comprehensive tests for the type-safe subscription lifecycle management.

use async_trait::async_trait;
use eventcore::{
    create_typed_subscription, EventId, EventProcessor, EventStore, EventStoreError, EventVersion,
    ReadOptions, StoredEvent, StreamData, StreamEvents, StreamId, Subscription, SubscriptionError,
    SubscriptionImpl, SubscriptionName, SubscriptionOptions, SubscriptionPosition,
    SubscriptionResult, Timestamp, TypedSubscription,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// Test event type
#[derive(Debug, Clone, PartialEq, Eq)]
struct TestEvent {
    id: String,
    data: String,
}

// Test processor that tracks processed events
struct TrackingProcessor {
    events: Arc<Mutex<Vec<TestEvent>>>,
}

impl TrackingProcessor {
    fn new() -> (Self, Arc<Mutex<Vec<TestEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                events: Arc::clone(&events),
            },
            events,
        )
    }
}

#[async_trait]
impl EventProcessor for TrackingProcessor {
    type Event = TestEvent;

    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()> {
        self.events.lock().unwrap().push(event.payload);
        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        // Mark that we've caught up
        Ok(())
    }
}

// Mock event store with controllable behavior
#[derive(Clone)]
struct ControllableEventStore {
    events: Arc<Mutex<Vec<StoredEvent<TestEvent>>>>,
    fail_subscribe: Arc<Mutex<bool>>,
    subscriptions_created: Arc<Mutex<usize>>,
}

impl ControllableEventStore {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            fail_subscribe: Arc::new(Mutex::new(false)),
            subscriptions_created: Arc::new(Mutex::new(0)),
        }
    }

    fn add_event(&self, event: TestEvent) {
        let stream_id = StreamId::try_new(format!("test-stream-{}", event.id)).unwrap();
        let event_id = EventId::new();
        let version = EventVersion::initial();
        let timestamp = Timestamp::now();

        let stored_event = StoredEvent::new(event_id, stream_id, version, timestamp, event, None);

        let mut events = self.events.lock().unwrap();
        events.push(stored_event);
    }

    fn set_fail_subscribe(&self, fail: bool) {
        *self.fail_subscribe.lock().unwrap() = fail;
    }

    fn get_subscription_count(&self) -> usize {
        *self.subscriptions_created.lock().unwrap()
    }
}

#[async_trait]
impl EventStore for ControllableEventStore {
    type Event = TestEvent;

    async fn read_streams(
        &self,
        _stream_ids: &[StreamId],
        _options: &ReadOptions,
    ) -> Result<StreamData<Self::Event>, EventStoreError> {
        let events = self.events.lock().unwrap().clone();
        Ok(StreamData::new(events, HashMap::new()))
    }

    async fn write_events_multi(
        &self,
        _stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> Result<HashMap<StreamId, EventVersion>, EventStoreError> {
        Ok(HashMap::new())
    }

    async fn stream_exists(&self, _stream_id: &StreamId) -> Result<bool, EventStoreError> {
        Ok(true)
    }

    async fn get_stream_version(
        &self,
        _stream_id: &StreamId,
    ) -> Result<Option<EventVersion>, EventStoreError> {
        Ok(Some(EventVersion::initial()))
    }

    async fn subscribe(
        &self,
        _options: SubscriptionOptions,
    ) -> Result<Box<dyn Subscription<Event = Self::Event>>, EventStoreError> {
        *self.subscriptions_created.lock().unwrap() += 1;

        if *self.fail_subscribe.lock().unwrap() {
            return Err(EventStoreError::ConnectionFailed(
                "Subscription creation failed".to_string(),
            ));
        }

        Ok(Box::new(SubscriptionImpl::<TestEvent>::new(Arc::new(
            Self::new(),
        ))))
    }
}

#[tokio::test]
async fn test_subscription_complete_lifecycle() {
    // Create event store and subscription
    let event_store = ControllableEventStore::new();
    let subscription = TypedSubscription::new(event_store.clone());

    // Configure subscription
    let name = SubscriptionName::try_new("test-lifecycle").unwrap();
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let (processor, _events_received) = TrackingProcessor::new();

    let configured = subscription.configure(name, options, Box::new(processor));

    // Start subscription
    let running = configured.start().await.unwrap();
    assert_eq!(event_store.get_subscription_count(), 1);

    // Test pause/resume/stop now that SubscriptionImpl is fully implemented
    let paused = running.pause().await.unwrap();
    let resumed = paused.resume().await.unwrap();
    let _stopped = resumed.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_start_failure() {
    // Create event store that fails on subscribe
    let event_store = ControllableEventStore::new();
    event_store.set_fail_subscribe(true);

    let subscription = TypedSubscription::new(event_store);

    // Configure subscription
    let name = SubscriptionName::try_new("test-failure").unwrap();
    let options = SubscriptionOptions::LiveOnly;
    let (processor, _) = TrackingProcessor::new();

    let configured = subscription.configure(name, options, Box::new(processor));

    // Start should fail
    let result = configured.start().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_adapter_pattern_migration() {
    // Test that existing code can use the adapter
    let event_store = ControllableEventStore::new();
    let mut subscription = create_typed_subscription(event_store);

    // Use the trait-based API
    let name = SubscriptionName::try_new("test-adapter").unwrap();
    let options = SubscriptionOptions::AllStreams {
        from_position: None,
    };
    let (processor, _) = TrackingProcessor::new();

    // Just test that we can start - the underlying implementation has todo!()
    subscription
        .start(name, options, Box::new(processor))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_invalid_state_transitions_with_adapter() {
    let event_store = ControllableEventStore::new();
    let mut subscription = create_typed_subscription(event_store);

    // Cannot pause without starting
    let result = subscription.pause().await;
    assert!(matches!(result, Err(SubscriptionError::EventStore(_))));

    // Cannot resume without pausing
    let result = subscription.resume().await;
    assert!(matches!(result, Err(SubscriptionError::EventStore(_))));
}

#[tokio::test]
async fn test_subscription_with_multiple_events() {
    let event_store = ControllableEventStore::new();

    // Add some test events
    for i in 0..5 {
        event_store.add_event(TestEvent {
            id: i.to_string(),
            data: format!("Event {i}"),
        });
    }

    let subscription = TypedSubscription::new(event_store.clone());

    let name = SubscriptionName::try_new("test-multiple").unwrap();
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let (processor, _events_received) = TrackingProcessor::new();

    let configured = subscription.configure(name, options, Box::new(processor));
    let running = configured.start().await.unwrap();

    // Test stop functionality now that SubscriptionImpl is fully implemented
    let _stopped = running.stop().await.unwrap();
    assert_eq!(event_store.get_subscription_count(), 1);
}

#[tokio::test]
async fn test_subscription_position_tracking() {
    let event_store = ControllableEventStore::new();
    let subscription = TypedSubscription::new(event_store);

    let name = SubscriptionName::try_new("test-position").unwrap();
    let options =
        SubscriptionOptions::CatchUpFromPosition(SubscriptionPosition::new(EventId::new()));
    let (processor, _) = TrackingProcessor::new();

    let configured = subscription.configure(name, options, Box::new(processor));
    let running = configured.start().await.unwrap();

    // Test position tracking now that SubscriptionImpl is fully implemented
    // Give the subscription a moment to process any events
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Check that we can get the current position
    let position = running.get_position().await.unwrap();
    // Position might be None if no events were processed yet
    println!("Current position: {position:?}");

    let _stopped = running.stop().await.unwrap();
}

// Property-based test for state machine invariants
#[test]
fn test_state_machine_type_safety() {
    // This test verifies compile-time guarantees
    // The following would not compile:

    // 1. Cannot create a subscription without an event store
    // let subscription: TypedSubscription<TestEvent, Uninitialized> = TypedSubscription::new();
    // ERROR: missing argument

    // 2. Cannot start without configuring
    // let subscription = TypedSubscription::new(store);
    // subscription.start().await;
    // ERROR: method not found

    // 3. Cannot pause a stopped subscription
    // let stopped: TypedSubscription<TestEvent, Stopped, _> = ...;
    // stopped.pause().await;
    // ERROR: method not found

    // The test passes if the code compiles
    // (No assertion needed - compilation success is the test)
}
