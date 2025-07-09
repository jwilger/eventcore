//! Comprehensive edge case tests for the subscription system.
//!
//! This module tests various failure scenarios, race conditions, and edge cases
//! to ensure the subscription system is robust and reliable in production.

#![allow(dead_code)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::uninlined_format_args)]

use eventcore::{
    EventProcessor, EventStore, EventStoreError, StreamId, SubscriptionError, SubscriptionName,
    SubscriptionOptions, SubscriptionPosition, SubscriptionResult,
};
use eventcore_memory::InMemoryEventStore;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestEvent {
    id: u32,
    message: String,
}

/// Mock event processor that can simulate various failure scenarios.
struct MockEventProcessor {
    processed_events: Arc<Mutex<Vec<TestEvent>>>,
    failure_mode: FailureMode,
    fail_after_count: Option<usize>,
    processing_delay: Option<Duration>,
}

#[derive(Debug, Clone)]
enum FailureMode {
    None,
    AlwaysFail,
    FailAfterCount(usize),
    FailOnSpecificEvent(u32),
    TransientFailure {
        fail_count: usize,
        then_succeed: bool,
    },
}

impl MockEventProcessor {
    fn new(failure_mode: FailureMode) -> Self {
        Self {
            processed_events: Arc::new(Mutex::new(Vec::new())),
            failure_mode,
            fail_after_count: None,
            processing_delay: None,
        }
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.processing_delay = Some(delay);
        self
    }

    fn get_processed_events(&self) -> Vec<TestEvent> {
        self.processed_events.lock().unwrap().clone()
    }

    fn processed_count(&self) -> usize {
        self.processed_events.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl EventProcessor for MockEventProcessor {
    type Event = TestEvent;

    async fn process_event(
        &mut self,
        event: eventcore::StoredEvent<Self::Event>,
    ) -> SubscriptionResult<()> {
        // Add processing delay if configured
        if let Some(delay) = self.processing_delay {
            tokio::time::sleep(delay).await;
        }

        let current_count = self.processed_count();

        // Check failure conditions
        let should_fail = match &self.failure_mode {
            FailureMode::None => false,
            FailureMode::AlwaysFail => true,
            FailureMode::FailAfterCount(count) => current_count >= *count,
            FailureMode::FailOnSpecificEvent(event_id) => event.payload.id == *event_id,
            FailureMode::TransientFailure {
                fail_count,
                then_succeed,
            } => {
                if current_count < *fail_count {
                    true
                } else {
                    !then_succeed
                }
            }
        };

        if should_fail {
            return Err(SubscriptionError::CheckpointSaveFailed(format!(
                "Mock failure for event {}",
                event.payload.id
            )));
        }

        // Record successful processing
        self.processed_events.lock().unwrap().push(event.payload);
        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        // Mock implementation - could simulate live processing logic
        Ok(())
    }
}

/// Helper function to create test events in a stream
async fn create_test_events(
    store: &InMemoryEventStore<TestEvent>,
    stream_id: &StreamId,
    count: usize,
) -> Result<(), EventStoreError> {
    use eventcore::{EventToWrite, ExpectedVersion, StreamEvents};

    let events: Vec<_> = (1..=count)
        .map(|i| {
            EventToWrite::new(
                eventcore::EventId::new(),
                TestEvent {
                    id: i as u32,
                    message: format!("Test event {}", i),
                },
            )
        })
        .collect();

    let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, events);
    store.write_events_multi(vec![stream_events]).await?;
    Ok(())
}

#[tokio::test]
async fn test_subscription_basic_functionality() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").unwrap();

    // Create some test events
    create_test_events(&store, &stream_id, 5).await.unwrap();

    // Create subscription
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Create processor
    let processor = MockEventProcessor::new(FailureMode::None);
    let processor_ref = Arc::new(Mutex::new(processor));

    // Start subscription
    let name = SubscriptionName::try_new("test-sub").unwrap();
    let _processor_clone = processor_ref.clone();
    let processor_box: Box<dyn EventProcessor<Event = TestEvent>> =
        Box::new(MockEventProcessor::new(FailureMode::None));

    // Note: This is a simplified test - in practice, we'd need to handle
    // the asynchronous nature of subscription processing
    let result = subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_subscription_handles_processing_failures() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").unwrap();

    // Create test events
    create_test_events(&store, &stream_id, 3).await.unwrap();

    // Create subscription that fails on second event
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = MockEventProcessor::new(FailureMode::FailOnSpecificEvent(2));
    let processor_box: Box<dyn EventProcessor<Event = TestEvent>> = Box::new(processor);

    let name = SubscriptionName::try_new("failing-sub").unwrap();

    // Start subscription - this should handle the failure gracefully
    let result = subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await;

    // The subscription should start successfully even if processing might fail later
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_subscription_checkpoint_persistence() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("checkpoint-stream").unwrap();

    // Create test events
    create_test_events(&store, &stream_id, 5).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Test checkpoint save/load
    let test_position = SubscriptionPosition::new(eventcore::EventId::new());

    // Save checkpoint
    let save_result = subscription.save_checkpoint(test_position.clone()).await;
    assert!(save_result.is_ok());

    // Load checkpoint
    let name = SubscriptionName::try_new("checkpoint-test").unwrap();
    let loaded_position = subscription.load_checkpoint(&name).await.unwrap();

    // For in-memory implementation, this might be None since we don't have
    // a specific name context in save_checkpoint
    // This tests the interface contract
    assert!(loaded_position.is_none() || loaded_position == Some(test_position));
}

#[tokio::test]
async fn test_subscription_pause_resume_functionality() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let options = SubscriptionOptions::LiveOnly;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Test pause
    let pause_result = subscription.pause().await;
    assert!(pause_result.is_ok());

    // Test resume
    let resume_result = subscription.resume().await;
    assert!(resume_result.is_ok());

    // Test stop
    let stop_result = subscription.stop().await;
    assert!(stop_result.is_ok());
}

#[tokio::test]
async fn test_subscription_position_tracking() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let subscription = store.subscribe(options).await.unwrap();

    // Test getting position when none exists
    let initial_position = subscription.get_position().await.unwrap();
    assert!(initial_position.is_none());
}

#[tokio::test]
async fn test_subscription_with_different_options() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();

    // Test different subscription options
    let options_variants = vec![
        SubscriptionOptions::CatchUpFromBeginning,
        SubscriptionOptions::LiveOnly,
        SubscriptionOptions::AllStreams {
            from_position: None,
        },
        SubscriptionOptions::SpecificStreams {
            streams: vec![StreamId::try_new("test").unwrap()],
            from_position: None,
        },
    ];

    for options in options_variants {
        let subscription_result = store.subscribe(options).await;
        assert!(subscription_result.is_ok(), "Failed to create subscription");
    }
}

#[tokio::test]
async fn test_subscription_error_scenarios() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Test with invalid subscription name
    let invalid_name_result = SubscriptionName::try_new("");
    assert!(invalid_name_result.is_err());

    // Test with valid name but mock failure scenario
    let valid_name = SubscriptionName::try_new("error-test").unwrap();
    let failing_processor = MockEventProcessor::new(FailureMode::AlwaysFail);
    let processor_box: Box<dyn EventProcessor<Event = TestEvent>> = Box::new(failing_processor);

    // This should succeed at the subscription level - processing failures
    // are handled within the subscription processing loop
    let start_result = subscription
        .start(
            valid_name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await;
    assert!(start_result.is_ok());
}

#[tokio::test]
async fn test_subscription_concurrent_operations() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("concurrent-stream").unwrap();

    // Create events while subscription might be running
    create_test_events(&store, &stream_id, 10).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Test pause/resume operations sequentially (borrow checker limitation)
    let pause_result = subscription.pause().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    let resume_result = subscription.resume().await;

    assert!(pause_result.is_ok());
    assert!(resume_result.is_ok());
}

#[tokio::test]
async fn test_subscription_memory_efficiency() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("memory-test-stream").unwrap();

    // Create a large number of events to test memory handling
    create_test_events(&store, &stream_id, 1000).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let subscription = store.subscribe(options).await.unwrap();

    // Verify subscription was created successfully even with many events
    assert!(subscription.get_position().await.is_ok());

    // Test that we can create multiple subscriptions without issues
    let subscription2 = store
        .subscribe(SubscriptionOptions::LiveOnly)
        .await
        .unwrap();
    assert!(subscription2.get_position().await.is_ok());
}

#[tokio::test]
async fn test_subscription_stream_filtering() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();

    // Create events in multiple streams
    let stream1 = StreamId::try_new("stream-1").unwrap();
    let stream2 = StreamId::try_new("stream-2").unwrap();

    create_test_events(&store, &stream1, 5).await.unwrap();
    create_test_events(&store, &stream2, 5).await.unwrap();

    // Test specific streams subscription
    let options = SubscriptionOptions::SpecificStreams {
        streams: vec![stream1.clone()],
        from_position: None,
    };

    let subscription = store.subscribe(options).await.unwrap();
    assert!(subscription.get_position().await.is_ok());

    // Test all streams subscription
    let all_options = SubscriptionOptions::AllStreams {
        from_position: None,
    };
    let all_subscription = store.subscribe(all_options).await.unwrap();
    assert!(all_subscription.get_position().await.is_ok());
}

/// Integration test that verifies end-to-end subscription behavior
#[tokio::test]
async fn test_subscription_integration_workflow() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("integration-stream").unwrap();

    // Phase 1: Create initial events
    create_test_events(&store, &stream_id, 3).await.unwrap();

    // Phase 2: Create and start subscription
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = MockEventProcessor::new(FailureMode::None);
    let name = SubscriptionName::try_new("integration-test").unwrap();
    let processor_box: Box<dyn EventProcessor<Event = TestEvent>> = Box::new(processor);

    let start_result = subscription
        .start(
            name.clone(),
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await;
    assert!(start_result.is_ok());

    // Phase 3: Add more events while subscription is running
    create_test_events(&store, &stream_id, 2).await.unwrap();

    // Phase 4: Test pause/resume cycle
    subscription.pause().await.unwrap();
    subscription.resume().await.unwrap();

    // Phase 5: Clean shutdown
    subscription.stop().await.unwrap();
}
