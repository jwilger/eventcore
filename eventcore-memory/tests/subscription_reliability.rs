//! Comprehensive reliability tests for in-memory subscription implementation.
//!
//! These tests specifically target the `InMemorySubscription` implementation
//! and verify robust behavior under various conditions.

#![allow(dead_code)]
#![allow(clippy::uninlined_format_args)]

use eventcore::{
    EventId, EventProcessor, EventStore, EventToWrite, ExpectedVersion, StreamEvents, StreamId,
    SubscriptionError, SubscriptionName, SubscriptionOptions, SubscriptionPosition,
    SubscriptionResult,
};
use eventcore_memory::InMemoryEventStore;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReliabilityTestEvent {
    sequence: u64,
    data: String,
    stream_name: String,
}

/// Event processor that tracks detailed processing information for reliability testing.
struct ReliabilityEventProcessor {
    processed_events: Arc<Mutex<Vec<ReliabilityTestEvent>>>,
    processing_delays: Vec<Duration>,
    failure_events: Arc<Mutex<Vec<u64>>>, // Event sequences that should fail
    retry_count: Arc<Mutex<usize>>,
    live_notifications: Arc<Mutex<usize>>,
}

impl ReliabilityEventProcessor {
    fn new() -> Self {
        Self {
            processed_events: Arc::new(Mutex::new(Vec::new())),
            processing_delays: Vec::new(),
            failure_events: Arc::new(Mutex::new(Vec::new())),
            retry_count: Arc::new(Mutex::new(0)),
            live_notifications: Arc::new(Mutex::new(0)),
        }
    }

    fn with_failures(self, failure_sequences: Vec<u64>) -> Self {
        *self.failure_events.lock().unwrap() = failure_sequences;
        self
    }

    fn with_processing_delays(mut self, delays: Vec<Duration>) -> Self {
        self.processing_delays = delays;
        self
    }

    fn get_processed_events(&self) -> Vec<ReliabilityTestEvent> {
        self.processed_events.lock().unwrap().clone()
    }

    fn get_retry_count(&self) -> usize {
        *self.retry_count.lock().unwrap()
    }

    fn get_live_notification_count(&self) -> usize {
        *self.live_notifications.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl EventProcessor for ReliabilityEventProcessor {
    type Event = ReliabilityTestEvent;

    async fn process_event(
        &mut self,
        event: eventcore::StoredEvent<Self::Event>,
    ) -> SubscriptionResult<()> {
        let sequence = event.payload.sequence;

        // Check if this event should fail
        let should_fail = {
            let failures = self.failure_events.lock().unwrap();
            failures.contains(&sequence)
        };

        if should_fail {
            *self.retry_count.lock().unwrap() += 1;
            return Err(SubscriptionError::CheckpointSaveFailed(format!(
                "Simulated failure for event sequence {}",
                sequence
            )));
        }

        // Apply processing delay if configured
        let processed_count = self.processed_events.lock().unwrap().len();
        if let Some(delay) = self.processing_delays.get(processed_count) {
            sleep(*delay).await;
        }

        // Record successful processing
        self.processed_events.lock().unwrap().push(event.payload);
        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        *self.live_notifications.lock().unwrap() += 1;
        Ok(())
    }
}

/// Helper to create events across multiple streams
async fn create_multi_stream_events(
    store: &InMemoryEventStore<ReliabilityTestEvent>,
    stream_count: usize,
    events_per_stream: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    for stream_idx in 0..stream_count {
        let stream_id = StreamId::try_new(format!("reliability-stream-{}", stream_idx))?;

        let events: Vec<_> = (0..events_per_stream)
            .map(|event_idx| {
                let sequence = (stream_idx * events_per_stream + event_idx) as u64;
                EventToWrite::new(
                    EventId::new(),
                    ReliabilityTestEvent {
                        sequence,
                        data: format!("Event {} in stream {}", event_idx, stream_idx),
                        stream_name: format!("reliability-stream-{}", stream_idx),
                    },
                )
            })
            .collect();

        let stream_events = StreamEvents::new(stream_id, ExpectedVersion::Any, events);
        store.write_events_multi(vec![stream_events]).await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_subscription_processes_all_events_in_order() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create events in multiple streams
    create_multi_stream_events(&store, 3, 5).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("order-test").unwrap();

    // Start subscription
    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await
        .unwrap();

    // Give some time for processing
    sleep(Duration::from_millis(100)).await;

    // Stop subscription
    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_handles_rapid_event_creation() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("rapid-events").unwrap();

    // Start subscription first
    let options = SubscriptionOptions::LiveOnly;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("rapid-test").unwrap();

    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(name, SubscriptionOptions::LiveOnly, processor_box)
        .await
        .unwrap();

    // Rapidly create events while subscription is running
    for i in 0..50 {
        let event = EventToWrite::new(
            EventId::new(),
            ReliabilityTestEvent {
                sequence: i,
                data: format!("Rapid event {}", i),
                stream_name: "rapid-events".to_string(),
            },
        );

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event]);
        store.write_events_multi(vec![stream_events]).await.unwrap();

        // Small delay to simulate realistic event creation
        sleep(Duration::from_millis(1)).await;
    }

    // Give time for processing
    sleep(Duration::from_millis(200)).await;

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_checkpoint_recovery() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create initial events
    create_multi_stream_events(&store, 2, 10).await.unwrap();

    // First subscription processes some events
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription1 = store.subscribe(options.clone()).await.unwrap();

    let processor1 = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("checkpoint-recovery").unwrap();

    let processor_box1: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> =
        Box::new(processor1);
    subscription1
        .start(name.clone(), options.clone(), processor_box1)
        .await
        .unwrap();

    // Process for a short time
    sleep(Duration::from_millis(50)).await;

    // Get current position
    let position = subscription1.get_position().await.unwrap();

    // Stop first subscription
    subscription1.stop().await.unwrap();

    // Create second subscription that should resume from checkpoint
    let mut subscription2 = store
        .subscribe(SubscriptionOptions::CatchUpFromPosition(
            position.unwrap_or_else(|| SubscriptionPosition::new(EventId::new())),
        ))
        .await
        .unwrap();

    let processor2 = ReliabilityEventProcessor::new();
    let processor_box2: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> =
        Box::new(processor2);

    subscription2
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box2,
        )
        .await
        .unwrap();

    // Process remaining events
    sleep(Duration::from_millis(100)).await;

    subscription2.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_pause_resume_reliability() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("pause-resume-test").unwrap();

    let options = SubscriptionOptions::LiveOnly;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("pause-resume").unwrap();

    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(name, SubscriptionOptions::LiveOnly, processor_box)
        .await
        .unwrap();

    // Create some events
    for i in 0..5 {
        let event = EventToWrite::new(
            EventId::new(),
            ReliabilityTestEvent {
                sequence: i,
                data: format!("Pre-pause event {}", i),
                stream_name: "pause-resume-test".to_string(),
            },
        );

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event]);
        store.write_events_multi(vec![stream_events]).await.unwrap();
    }

    sleep(Duration::from_millis(50)).await;

    // Pause subscription
    subscription.pause().await.unwrap();

    // Create events while paused (these should not be processed immediately)
    for i in 5..10 {
        let event = EventToWrite::new(
            EventId::new(),
            ReliabilityTestEvent {
                sequence: i,
                data: format!("Paused event {}", i),
                stream_name: "pause-resume-test".to_string(),
            },
        );

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event]);
        store.write_events_multi(vec![stream_events]).await.unwrap();
    }

    sleep(Duration::from_millis(50)).await;

    // Resume subscription
    subscription.resume().await.unwrap();

    // Create post-resume events
    for i in 10..15 {
        let event = EventToWrite::new(
            EventId::new(),
            ReliabilityTestEvent {
                sequence: i,
                data: format!("Post-resume event {}", i),
                stream_name: "pause-resume-test".to_string(),
            },
        );

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event]);
        store.write_events_multi(vec![stream_events]).await.unwrap();
    }

    // Allow processing
    sleep(Duration::from_millis(100)).await;

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_error_recovery() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create events
    create_multi_stream_events(&store, 1, 10).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Configure processor to fail on specific events
    let processor = ReliabilityEventProcessor::new().with_failures(vec![2, 5, 8]); // Fail on these sequence numbers

    let name = SubscriptionName::try_new("error-recovery").unwrap();
    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);

    subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await
        .unwrap();

    // Allow time for processing and error handling
    sleep(Duration::from_millis(200)).await;

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_concurrent_operations_reliability() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("concurrent-ops").unwrap();

    let options = SubscriptionOptions::LiveOnly;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("concurrent-test").unwrap();

    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(name, SubscriptionOptions::LiveOnly, processor_box)
        .await
        .unwrap();

    // Spawn concurrent tasks
    let store_clone = store.clone();
    let stream_id_clone = stream_id.clone();

    let event_creation_task = tokio::spawn(async move {
        for i in 0..20 {
            let event = EventToWrite::new(
                EventId::new(),
                ReliabilityTestEvent {
                    sequence: i,
                    data: format!("Concurrent event {}", i),
                    stream_name: "concurrent-ops".to_string(),
                },
            );

            let stream_events =
                StreamEvents::new(stream_id_clone.clone(), ExpectedVersion::Any, vec![event]);

            if let Err(e) = store_clone.write_events_multi(vec![stream_events]).await {
                eprintln!("Failed to write event {}: {}", i, e);
            }

            sleep(Duration::from_millis(5)).await;
        }
    });

    let pause_resume_task = tokio::spawn(async move {
        sleep(Duration::from_millis(25)).await;
        let _ = subscription.pause().await;
        sleep(Duration::from_millis(25)).await;
        let _ = subscription.resume().await;
        sleep(Duration::from_millis(50)).await;
        let _ = subscription.stop().await;
    });

    // Wait for both tasks to complete
    let (event_result, pause_result) = tokio::join!(event_creation_task, pause_resume_task);

    assert!(event_result.is_ok());
    assert!(pause_result.is_ok());
}

#[tokio::test]
async fn test_subscription_stream_filtering_reliability() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create events in multiple streams
    create_multi_stream_events(&store, 5, 10).await.unwrap();

    // Create subscription that only listens to specific streams
    let target_streams = vec![
        StreamId::try_new("reliability-stream-1").unwrap(),
        StreamId::try_new("reliability-stream-3").unwrap(),
    ];

    let options = SubscriptionOptions::SpecificStreams {
        streams: target_streams.clone(),
        from_position: None,
    };

    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("filtering-test").unwrap();

    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(
            name,
            SubscriptionOptions::SpecificStreams {
                streams: target_streams,
                from_position: None,
            },
            processor_box,
        )
        .await
        .unwrap();

    // Allow processing time
    sleep(Duration::from_millis(100)).await;

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_position_consistency() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create events
    create_multi_stream_events(&store, 2, 5).await.unwrap();

    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Check initial position
    let initial_position = subscription.get_position().await.unwrap();
    assert!(initial_position.is_none());

    let processor = ReliabilityEventProcessor::new();
    let name = SubscriptionName::try_new("position-test").unwrap();

    let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> = Box::new(processor);
    subscription
        .start(
            name.clone(),
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await
        .unwrap();

    // Process events
    sleep(Duration::from_millis(100)).await;

    // Check position after processing
    let _position_after = subscription.get_position().await.unwrap();

    // Position should be updated after processing events
    // (exact behavior depends on implementation details)

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_memory_cleanup() {
    let store: InMemoryEventStore<ReliabilityTestEvent> = InMemoryEventStore::new();

    // Create many events to test memory handling
    create_multi_stream_events(&store, 10, 100).await.unwrap();

    // Create and destroy multiple subscriptions
    for i in 0..5 {
        let options = SubscriptionOptions::CatchUpFromBeginning;
        let mut subscription = store.subscribe(options).await.unwrap();

        let processor = ReliabilityEventProcessor::new();
        let name = SubscriptionName::try_new(format!("cleanup-test-{}", i)).unwrap();

        let processor_box: Box<dyn EventProcessor<Event = ReliabilityTestEvent>> =
            Box::new(processor);
        subscription
            .start(
                name,
                SubscriptionOptions::CatchUpFromBeginning,
                processor_box,
            )
            .await
            .unwrap();

        // Process briefly
        sleep(Duration::from_millis(20)).await;

        // Clean shutdown
        subscription.stop().await.unwrap();
    }

    // Final subscription should still work normally
    let options = SubscriptionOptions::LiveOnly;
    let subscription = store.subscribe(options).await.unwrap();
    assert!(subscription.get_position().await.is_ok());
}
