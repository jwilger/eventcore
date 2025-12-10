use eventcore::{
    Event, EventStore, EventSubscription, InMemoryEventStore, StreamId, StreamPrefix,
    StreamVersion, StreamWrites, SubscriptionQuery,
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

#[tokio::test]
async fn filters_events_by_stream_prefix() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream IDs with different prefixes
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");
    let account_456 = StreamId::try_new("account-456").expect("valid stream id");
    let order_789 = StreamId::try_new("order-789").expect("valid stream id");

    // And: Developer appends events to streams with different prefixes
    let writes = StreamWrites::new()
        .register_stream(account_123.clone(), StreamVersion::new(0))
        .and_then(|w| w.register_stream(account_456.clone(), StreamVersion::new(0)))
        .and_then(|w| w.register_stream(order_789.clone(), StreamVersion::new(0)))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: account_123.clone(),
                value: 100,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: order_789.clone(),
                value: 500,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: account_456.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes with stream prefix filter for "account-"
    let subscription = store
        .subscribe(SubscriptionQuery::all().filter_stream_prefix(StreamPrefix::new("account-")))
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects events from the subscription stream
    let events: Vec<TestEvent> = subscription.take(2).collect().await;

    // Then: Only events from streams starting with "account-" are delivered
    assert_eq!(
        events.len(),
        2,
        "should deliver only 2 events from account-* streams"
    );

    assert_eq!(
        events[0],
        TestEvent::ValueRecorded {
            stream_id: account_123,
            value: 100
        },
        "first event should be from account-123"
    );

    assert_eq!(
        events[1],
        TestEvent::ValueRecorded {
            stream_id: account_456,
            value: 200
        },
        "second event should be from account-456"
    );
}
