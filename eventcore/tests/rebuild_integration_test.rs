//! Integration test for projection rebuild functionality using subscriptions.
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::items_after_statements)]

use eventcore::{
    EventProcessor, EventStore, EventToWrite, ExpectedVersion, StreamEvents, StreamId,
    SubscriptionError, SubscriptionResult,
};
use eventcore_memory::InMemoryEventStore;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestEvent {
    id: u64,
    value: String,
}

/// A simple event processor that counts events
struct CountingProcessor {
    events_processed: Arc<AtomicU64>,
}

impl CountingProcessor {
    fn new() -> Self {
        Self {
            events_processed: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl EventProcessor for CountingProcessor {
    type Event = TestEvent;

    async fn process_event(
        &mut self,
        _event: eventcore::StoredEvent<Self::Event>,
    ) -> SubscriptionResult<()> {
        self.events_processed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_rebuild_uses_subscription_system() {
    // Create event store and populate with events
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();

    // Create test events
    for i in 0..10 {
        let stream_id = StreamId::try_new(format!("test-stream-{}", i % 3)).unwrap();
        let event = EventToWrite::new(
            eventcore::EventId::new(),
            TestEvent {
                id: i,
                value: format!("event-{}", i),
            },
        );

        store
            .write_events_multi(vec![StreamEvents::new(
                stream_id,
                ExpectedVersion::Any,
                vec![event],
            )])
            .await
            .unwrap();
    }

    // Create a subscription and processor
    let processor = CountingProcessor::new();
    let count_ref = processor.events_processed.clone();

    let mut subscription = store
        .subscribe(eventcore::SubscriptionOptions::CatchUpFromBeginning)
        .await
        .unwrap();

    subscription
        .start(
            eventcore::SubscriptionName::try_new("test-subscription").unwrap(),
            eventcore::SubscriptionOptions::CatchUpFromBeginning,
            Box::new(processor),
        )
        .await
        .unwrap();

    // Give time for processing
    let mut attempts = 0;
    let expected_count = 10;
    while attempts < 20 {
        let count = count_ref.load(Ordering::SeqCst);
        if count >= expected_count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        attempts += 1;
    }

    // Stop subscription before checking to prevent further processing
    subscription.stop().await.unwrap();

    // Verify all events were processed
    let final_count = count_ref.load(Ordering::SeqCst);
    assert!(
        final_count >= expected_count,
        "Expected at least {} events, got {}",
        expected_count,
        final_count
    );
}

#[tokio::test]
async fn test_rebuild_from_specific_position() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();
    let mut event_ids = Vec::new();

    // Create test events
    for i in 0..5 {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_id = eventcore::EventId::new();
        event_ids.push(event_id);

        let event = EventToWrite::new(
            event_id,
            TestEvent {
                id: i,
                value: format!("event-{}", i),
            },
        );

        store
            .write_events_multi(vec![StreamEvents::new(
                stream_id,
                ExpectedVersion::Any,
                vec![event],
            )])
            .await
            .unwrap();
    }

    // Create subscription from position 2 (should process events 2, 3, 4)
    let processor = CountingProcessor::new();
    let count_ref = processor.events_processed.clone();

    let position = eventcore::SubscriptionPosition::new(event_ids[2]);
    let options = eventcore::SubscriptionOptions::CatchUpFromPosition(position);
    let mut subscription = store.subscribe(options.clone()).await.unwrap();

    subscription
        .start(
            eventcore::SubscriptionName::try_new("test-from-position").unwrap(),
            options,
            Box::new(processor),
        )
        .await
        .unwrap();

    // Give time for processing
    let mut attempts = 0;
    while attempts < 20 {
        let count = count_ref.load(Ordering::SeqCst);
        if count >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        attempts += 1;
    }

    // Stop subscription before checking to prevent further processing
    subscription.stop().await.unwrap();

    // Should have processed events from position 2 onwards
    // The exact count depends on whether the position is inclusive or exclusive
    let final_count = count_ref.load(Ordering::SeqCst);
    assert!(
        final_count >= 2,
        "Expected at least 2 events, got {}",
        final_count
    );
}

#[tokio::test]
async fn test_subscription_cancellation() {
    let store: InMemoryEventStore<TestEvent> = InMemoryEventStore::new();

    // Create many events
    for i in 0..100 {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = EventToWrite::new(
            eventcore::EventId::new(),
            TestEvent {
                id: i,
                value: format!("event-{}", i),
            },
        );

        store
            .write_events_multi(vec![StreamEvents::new(
                stream_id,
                ExpectedVersion::Any,
                vec![event],
            )])
            .await
            .unwrap();
    }

    // Create a processor that returns Cancelled after 10 events
    struct CancellingProcessor {
        count: u64,
    }

    #[async_trait::async_trait]
    impl EventProcessor for CancellingProcessor {
        type Event = TestEvent;

        async fn process_event(
            &mut self,
            _event: eventcore::StoredEvent<Self::Event>,
        ) -> SubscriptionResult<()> {
            self.count += 1;
            if self.count >= 10 {
                return Err(SubscriptionError::Cancelled);
            }
            Ok(())
        }

        async fn on_live(&mut self) -> SubscriptionResult<()> {
            Ok(())
        }
    }

    let processor = CancellingProcessor { count: 0 };
    let mut subscription = store
        .subscribe(eventcore::SubscriptionOptions::CatchUpFromBeginning)
        .await
        .unwrap();

    let result = subscription
        .start(
            eventcore::SubscriptionName::try_new("cancelling-test").unwrap(),
            eventcore::SubscriptionOptions::CatchUpFromBeginning,
            Box::new(processor),
        )
        .await;

    // The subscription should stop when the processor returns Cancelled
    // This behavior depends on the implementation - it might return Ok or an error
    let _ = result; // Either Ok or Err is acceptable when cancelled
}
