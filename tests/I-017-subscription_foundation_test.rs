use eventcore::{
    Event, EventStore, EventSubscription, EventTypeName, InMemoryEventStore, StreamId,
    StreamPrefix, StreamVersion, StreamWrites, SubscriptionQuery,
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

    fn event_type_name(&self) -> EventTypeName {
        match self {
            TestEvent::ValueRecorded { .. } => {
                "ValueRecorded".try_into().expect("valid event type name")
            }
        }
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec!["ValueRecorded".try_into().expect("valid event type name")]
    }
}

/// Account domain event for event type filtering tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MoneyDeposited {
    stream_id: StreamId,
    amount: u32,
}

impl Event for MoneyDeposited {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> EventTypeName {
        "MoneyDeposited".try_into().expect("valid event type name")
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec!["MoneyDeposited".try_into().expect("valid event type name")]
    }
}

/// Account domain event for event type filtering tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MoneyWithdrawn {
    stream_id: StreamId,
    amount: u32,
}

impl Event for MoneyWithdrawn {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> EventTypeName {
        "MoneyWithdrawn".try_into().expect("valid event type name")
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec!["MoneyWithdrawn".try_into().expect("valid event type name")]
    }
}

/// Account domain events as an enum with multiple variants.
///
/// This demonstrates the need for event_type_name() - all variants share
/// the same TypeId (AccountEvent), but represent different event types
/// that should be filterable independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum AccountEvent {
    Deposited { stream_id: StreamId, amount: u32 },
    Withdrawn { stream_id: StreamId, amount: u32 },
}

impl Event for AccountEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            AccountEvent::Deposited { stream_id, .. } => stream_id,
            AccountEvent::Withdrawn { stream_id, .. } => stream_id,
        }
    }

    fn event_type_name(&self) -> EventTypeName {
        match self {
            AccountEvent::Deposited { .. } => {
                "Deposited".try_into().expect("valid event type name")
            }
            AccountEvent::Withdrawn { .. } => {
                "Withdrawn".try_into().expect("valid event type name")
            }
        }
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec![
            "Deposited".try_into().expect("valid event type name"),
            "Withdrawn".try_into().expect("valid event type name"),
        ]
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
    let subscription =
        store
            .subscribe(SubscriptionQuery::all().filter_stream_prefix(
                StreamPrefix::try_new("account-").expect("valid stream prefix"),
            ))
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

#[tokio::test]
async fn filters_events_by_event_type() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream IDs for account streams
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");
    let account_456 = StreamId::try_new("account-456").expect("valid stream id");

    // And: Developer appends events of different types to account streams
    // Mix MoneyDeposited and MoneyWithdrawn events to verify type filtering
    let writes = StreamWrites::new()
        .register_stream(account_123.clone(), StreamVersion::new(0))
        .and_then(|w| w.register_stream(account_456.clone(), StreamVersion::new(0)))
        .and_then(|w| {
            w.append(MoneyDeposited {
                stream_id: account_123.clone(),
                amount: 100,
            })
        })
        .and_then(|w| {
            w.append(MoneyWithdrawn {
                stream_id: account_123.clone(),
                amount: 50,
            })
        })
        .and_then(|w| {
            w.append(MoneyDeposited {
                stream_id: account_456.clone(),
                amount: 200,
            })
        })
        .and_then(|w| {
            w.append(MoneyWithdrawn {
                stream_id: account_456.clone(),
                amount: 75,
            })
        })
        .and_then(|w| {
            w.append(MoneyDeposited {
                stream_id: account_123.clone(),
                amount: 150,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes with event type filter for MoneyDeposited
    let subscription =
        store
            .subscribe(SubscriptionQuery::all().filter_event_type_name(
                "MoneyDeposited".try_into().expect("valid event type name"),
            ))
            .await
            .expect("subscription should be created successfully");

    // And: Developer collects events from the subscription stream
    let events: Vec<MoneyDeposited> = subscription.take(3).collect().await;

    // Then: Only MoneyDeposited events are delivered, MoneyWithdrawn events are filtered out
    assert_eq!(
        events.len(),
        3,
        "should deliver only 3 MoneyDeposited events"
    );

    assert_eq!(
        events[0],
        MoneyDeposited {
            stream_id: account_123.clone(),
            amount: 100
        },
        "first event should be account-123 deposit of 100"
    );

    assert_eq!(
        events[1],
        MoneyDeposited {
            stream_id: account_456.clone(),
            amount: 200
        },
        "second event should be account-456 deposit of 200"
    );

    assert_eq!(
        events[2],
        MoneyDeposited {
            stream_id: account_123,
            amount: 150
        },
        "third event should be account-123 deposit of 150"
    );
}

#[tokio::test]
async fn filters_events_by_event_type_name_for_enum_variants() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream IDs for account streams
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");
    let account_456 = StreamId::try_new("account-456").expect("valid stream id");

    // And: Developer appends events using enum variants (AccountEvent::Deposited, AccountEvent::Withdrawn)
    // This demonstrates the problem: TypeId-based filtering treats all enum variants as the same type
    let writes = StreamWrites::new()
        .register_stream(account_123.clone(), StreamVersion::new(0))
        .and_then(|w| w.register_stream(account_456.clone(), StreamVersion::new(0)))
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_123.clone(),
                amount: 100,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Withdrawn {
                stream_id: account_123.clone(),
                amount: 50,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_456.clone(),
                amount: 200,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Withdrawn {
                stream_id: account_456.clone(),
                amount: 75,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_123.clone(),
                amount: 150,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes with event type name filter for "Deposited" variant
    // Uses string-based filter instead of TypeId to distinguish enum variants
    let subscription = store
        .subscribe::<AccountEvent>(
            SubscriptionQuery::all()
                .filter_event_type_name("Deposited".try_into().expect("valid event type name")),
        )
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects events from the subscription stream
    let events: Vec<AccountEvent> = subscription.take(3).collect().await;

    // Then: Only Deposited variant events are delivered, Withdrawn events are filtered out
    assert_eq!(events.len(), 3, "should deliver only 3 Deposited events");

    assert_eq!(
        events[0],
        AccountEvent::Deposited {
            stream_id: account_123.clone(),
            amount: 100
        },
        "first event should be account-123 deposit of 100"
    );

    assert_eq!(
        events[1],
        AccountEvent::Deposited {
            stream_id: account_456.clone(),
            amount: 200
        },
        "second event should be account-456 deposit of 200"
    );

    assert_eq!(
        events[2],
        AccountEvent::Deposited {
            stream_id: account_123,
            amount: 150
        },
        "third event should be account-123 deposit of 150"
    );
}

#[test]
fn struct_event_type_returns_single_type_name() {
    // Given: Developer uses a struct event type (MoneyDeposited)

    // When: Developer calls all_type_names() associated function
    let type_names = MoneyDeposited::all_type_names();

    // Then: Returns a single-element vec with the struct's type name
    assert_eq!(
        type_names,
        vec!["MoneyDeposited".try_into().expect("valid event type name")],
        "struct event should return its single type name"
    );
}

#[test]
fn enum_event_type_returns_all_variant_names() {
    // Given: Developer uses an enum event type (AccountEvent) with variants

    // When: Developer calls all_type_names() associated function
    let type_names = AccountEvent::all_type_names();

    // Then: Returns vec with all variant names
    assert_eq!(
        type_names,
        vec![
            "Deposited".try_into().expect("valid event type name"),
            "Withdrawn".try_into().expect("valid event type name")
        ],
        "enum event should return all variant names"
    );
}
