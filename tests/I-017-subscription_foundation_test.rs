use eventcore::{
    Event, EventStore, EventSubscription, InMemoryEventStore, StreamId, StreamVersion,
    StreamWrites, SubscriptionQuery,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

/// Test-specific domain events for subscription testing.
///
/// Simple events across multiple streams to verify temporal ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TestEvent {
    ValueRecorded { stream_id: StreamId, value: u32 },
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestEvent::ValueRecorded { stream_id, .. } => stream_id,
        }
    }
}

/// Integration test for I-017: Subscription Foundation
///
/// This test exercises subscription functionality from the library consumer
/// (application developer) perspective. It verifies that events from multiple
/// streams can be subscribed to and delivered in EventId (UUIDv7) order.
#[tokio::test]
async fn subscribes_to_all_events_in_temporal_order() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream IDs for multiple event streams
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");
    let stream_b = StreamId::try_new("stream-b").expect("valid stream id");
    let stream_c = StreamId::try_new("stream-c").expect("valid stream id");

    // And: Developer appends events to multiple streams
    // Events are appended in a specific order to verify temporal ordering
    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| w.register_stream(stream_b.clone(), StreamVersion::new(0)))
        .and_then(|w| w.register_stream(stream_c.clone(), StreamVersion::new(0)))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_b.clone(),
                value: 200,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 150,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_c.clone(),
                value: 300,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_b.clone(),
                value: 250,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_c.clone(),
                value: 350,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes to all events
    let subscription = store
        .subscribe(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects events from the subscription stream
    let events: Vec<TestEvent> = subscription.take(6).collect().await;

    // Then: All 6 events are delivered in EventId (UUIDv7) order
    // Since UUIDv7 is time-ordered and events were appended sequentially,
    // they should arrive in the same order they were appended
    assert_eq!(events.len(), 6, "should deliver all 6 events");

    assert_eq!(
        events[0],
        TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 100
        },
        "first event should be stream-a value 100"
    );

    assert_eq!(
        events[1],
        TestEvent::ValueRecorded {
            stream_id: stream_b.clone(),
            value: 200
        },
        "second event should be stream-b value 200"
    );

    assert_eq!(
        events[2],
        TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 150
        },
        "third event should be stream-a value 150"
    );

    assert_eq!(
        events[3],
        TestEvent::ValueRecorded {
            stream_id: stream_c.clone(),
            value: 300
        },
        "fourth event should be stream-c value 300"
    );

    assert_eq!(
        events[4],
        TestEvent::ValueRecorded {
            stream_id: stream_b.clone(),
            value: 250
        },
        "fifth event should be stream-b value 250"
    );

    assert_eq!(
        events[5],
        TestEvent::ValueRecorded {
            stream_id: stream_c.clone(),
            value: 350
        },
        "sixth event should be stream-c value 350"
    );
}
