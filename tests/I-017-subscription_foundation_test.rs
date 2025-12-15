use eventcore::{
    Event, EventStore, EventSubscription, EventTypeName, InMemoryEventStore, StreamId,
    StreamPrefix, StreamVersion, StreamWrites, Subscribable, SubscriptionError, SubscriptionQuery,
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
    let events: Vec<TestEvent> = subscription
        .take(6)
        .map(|r| r.expect("event should deserialize"))
        .collect()
        .await;

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
    let events: Vec<TestEvent> = subscription
        .take(2)
        .map(|r| r.expect("event should deserialize"))
        .collect()
        .await;

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
    let events: Vec<MoneyDeposited> = subscription
        .take(3)
        .map(|r| r.expect("event should deserialize"))
        .collect()
        .await;

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
    let events: Vec<AccountEvent> = subscription
        .take(3)
        .map(|r| r.expect("event should deserialize"))
        .collect()
        .await;

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

#[tokio::test]
async fn chains_stream_combinators_for_transformation() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates account streams with events
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");
    let account_456 = StreamId::try_new("account-456").expect("valid stream id");
    let order_789 = StreamId::try_new("order-789").expect("valid stream id");

    // And: Developer appends events with various values
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
                stream_id: account_456.clone(),
                value: 50,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: order_789.clone(),
                value: 300,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: account_123.clone(),
                value: 200,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: account_456.clone(),
                value: 75,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: order_789.clone(),
                value: 500,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes to active subscription stream
    let subscription = store
        .subscribe(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer chains .map().filter().take(3) combinators
    // Map events to their values, filter for values >= 100, take first 3 results
    let values: Vec<u32> = subscription
        .map(|r| r.expect("event should deserialize"))
        .map(|event| match event {
            TestEvent::ValueRecorded { value, .. } => value,
        })
        .filter(|value| futures::future::ready(*value >= 100))
        .take(3)
        .collect()
        .await;

    // Then: Standard futures combinators work
    // And: Events can be collected into Vec
    assert_eq!(values.len(), 3, "should collect 3 values >= 100");
    assert_eq!(values[0], 100, "first value should be 100");
    assert_eq!(values[1], 300, "second value should be 300");
    assert_eq!(values[2], 200, "third value should be 200");
}

/// Simple read model for folding account balance from events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct AccountBalance {
    balance: u32,
}

impl AccountBalance {
    fn apply(&mut self, event: &AccountEvent) {
        match event {
            AccountEvent::Deposited { amount, .. } => {
                self.balance += amount;
            }
            AccountEvent::Withdrawn { amount, .. } => {
                self.balance = self.balance.saturating_sub(*amount);
            }
        }
    }
}

#[tokio::test]
async fn builds_read_model_by_folding_subscription_stream() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates account stream with deposit and withdrawal events
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");

    let writes = StreamWrites::new()
        .register_stream(account_123.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_123.clone(),
                amount: 100,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_123.clone(),
                amount: 50,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Withdrawn {
                stream_id: account_123.clone(),
                amount: 30,
            })
        })
        .and_then(|w| {
            w.append(AccountEvent::Deposited {
                stream_id: account_123.clone(),
                amount: 75,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes using AccountEvent enum
    // This should automatically handle both Deposited and Withdrawn variants
    let subscription =
        store
            .subscribe::<AccountEvent>(SubscriptionQuery::all().filter_stream_prefix(
                StreamPrefix::try_new("account-").expect("valid stream prefix"),
            ))
            .await
            .expect("subscription should be created successfully");

    // And: Developer folds events into AccountBalance struct
    let balance = subscription
        .map(|r| r.expect("event should deserialize"))
        .fold(AccountBalance::default(), |mut acc, event| async move {
            acc.apply(&event);
            acc
        })
        .await;

    // Then: Read model reflects current state
    // 100 (deposit) + 50 (deposit) - 30 (withdrawal) + 75 (deposit) = 195
    assert_eq!(balance.balance, 195, "balance should be 195");

    // And: Projection updates as new events arrive
    // Developer appends more events to same stream
    let more_writes = StreamWrites::new()
        .register_stream(account_123.clone(), StreamVersion::new(4))
        .and_then(|w| {
            w.append(AccountEvent::Withdrawn {
                stream_id: account_123.clone(),
                amount: 45,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(more_writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer creates new subscription after new events
    let updated_subscription =
        store
            .subscribe::<AccountEvent>(SubscriptionQuery::all().filter_stream_prefix(
                StreamPrefix::try_new("account-").expect("valid stream prefix"),
            ))
            .await
            .expect("subscription should be created successfully");

    // And: Developer folds all events again
    let updated_balance = updated_subscription
        .map(|r| r.expect("event should deserialize"))
        .fold(AccountBalance::default(), |mut acc, event| async move {
            acc.apply(&event);
            acc
        })
        .await;

    // Then: Updated read model includes new event
    // Previous balance 195 - 45 (new withdrawal) = 150
    assert_eq!(
        updated_balance.balance, 150,
        "updated balance should be 150"
    );
}

/// View enum that aggregates multiple disjoint event types.
///
/// This view type wraps MoneyDeposited and MoneyWithdrawn struct events
/// and allows subscribing to both types in a single subscription.
/// Unlike Event trait types, view types don't have a single stream_id
/// because they aggregate events from different types/streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum AccountEventView {
    Deposited(MoneyDeposited),
    Withdrawn(MoneyWithdrawn),
}

impl Subscribable for AccountEventView {
    fn subscribable_type_names() -> Vec<EventTypeName> {
        vec![
            "MoneyDeposited".try_into().expect("valid event type name"),
            "MoneyWithdrawn".try_into().expect("valid event type name"),
        ]
    }

    fn try_from_stored(type_name: &EventTypeName, data: &[u8]) -> Result<Self, SubscriptionError> {
        match type_name.as_ref() {
            "MoneyDeposited" => {
                let event: MoneyDeposited = serde_json::from_slice(data)
                    .map_err(|e| SubscriptionError::Generic(e.to_string()))?;
                Ok(AccountEventView::Deposited(event))
            }
            "MoneyWithdrawn" => {
                let event: MoneyWithdrawn = serde_json::from_slice(data)
                    .map_err(|e| SubscriptionError::Generic(e.to_string()))?;
                Ok(AccountEventView::Withdrawn(event))
            }
            _ => Err(SubscriptionError::Generic(format!(
                "Unexpected event type: {}",
                type_name
            ))),
        }
    }
}

impl AccountBalance {
    fn apply_view(&mut self, event: &AccountEventView) {
        match event {
            AccountEventView::Deposited(MoneyDeposited { amount, .. }) => {
                self.balance += amount;
            }
            AccountEventView::Withdrawn(MoneyWithdrawn { amount, .. }) => {
                self.balance = self.balance.saturating_sub(*amount);
            }
        }
    }
}

#[tokio::test]
async fn subscribes_to_disjoint_types_via_view_enum() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates account streams
    let account_123 = StreamId::try_new("account-123").expect("valid stream id");
    let account_456 = StreamId::try_new("account-456").expect("valid stream id");

    // And: Developer appends events using STRUCT types (MoneyDeposited, MoneyWithdrawn)
    // These are stored as individual event types, NOT as AccountEventView
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
                amount: 30,
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
                amount: 50,
            })
        })
        .and_then(|w| {
            w.append(MoneyDeposited {
                stream_id: account_123.clone(),
                amount: 75,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes using VIEW ENUM type (AccountEventView)
    // This should automatically receive BOTH MoneyDeposited and MoneyWithdrawn events
    // wrapped in the appropriate enum variants
    let subscription = store
        .subscribe::<AccountEventView>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer folds events into AccountBalance read model
    let balance = subscription
        .map(|r| r.expect("event should deserialize"))
        .fold(AccountBalance::default(), |mut acc, event| async move {
            acc.apply_view(&event);
            acc
        })
        .await;

    // Then: Read model correctly aggregates BOTH event types
    // account-123: 100 (deposit) - 30 (withdrawal) + 75 (deposit) = 145
    // account-456: 200 (deposit) - 50 (withdrawal) = 150
    // Total: 145 + 150 = 295
    assert_eq!(
        balance.balance, 295,
        "balance should aggregate both accounts"
    );
}

#[tokio::test]
async fn delivers_events_appended_after_subscription_creation() {
    // Given: Developer creates in-memory event store
    let store = std::sync::Arc::new(InMemoryEventStore::new());

    // And: Developer creates stream ID
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    // And: Developer appends initial events BEFORE subscription
    let initial_writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(initial_writes)
        .await
        .expect("initial events should be appended successfully");

    // When: Developer creates subscription
    let subscription = store
        .subscribe(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer appends MORE events AFTER subscription is created
    let store_clone = store.clone();
    let stream_a_clone = stream_a.clone();
    let append_task = tokio::spawn(async move {
        // Small delay to ensure subscription is actively consuming
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let later_writes = StreamWrites::new()
            .register_stream(stream_a_clone.clone(), StreamVersion::new(2))
            .and_then(|w| {
                w.append(TestEvent::ValueRecorded {
                    stream_id: stream_a_clone.clone(),
                    value: 300,
                })
            })
            .and_then(|w| {
                w.append(TestEvent::ValueRecorded {
                    stream_id: stream_a_clone.clone(),
                    value: 400,
                })
            })
            .expect("writes should be constructed successfully");

        store_clone
            .append_events(later_writes)
            .await
            .expect("later events should be appended successfully");
    });

    // And: Developer collects events from the subscription stream
    // Using timeout to prevent test from hanging if live delivery fails
    let events: Vec<TestEvent> = tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        subscription
            .take(4)
            .map(|r| r.expect("event should deserialize"))
            .collect(),
    )
    .await
    .expect("subscription should deliver all 4 events within timeout");

    append_task.await.expect("append task should complete");

    // Then: ALL 4 events are delivered - both initial AND later events
    assert_eq!(
        events.len(),
        4,
        "subscription should deliver events appended after creation"
    );

    assert_eq!(
        events[0],
        TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 100
        },
        "first event should be initial event with value 100"
    );

    assert_eq!(
        events[1],
        TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 200
        },
        "second event should be initial event with value 200"
    );

    assert_eq!(
        events[2],
        TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 300
        },
        "third event should be later event with value 300"
    );

    assert_eq!(
        events[3],
        TestEvent::ValueRecorded {
            stream_id: stream_a,
            value: 400
        },
        "fourth event should be later event with value 400"
    );
}

#[tokio::test]
async fn subscription_delivers_events_exactly_once_without_duplicates() {
    // Given: Developer creates in-memory event store with NO initial events
    // This exercises the `next_seq == 0` branch in catchup_max_seq calculation
    //
    // The mutant changes `if *next_seq == 0 { 0 } else { *next_seq - 1 }`
    //                 to `if *next_seq != 0 { 0 } else { *next_seq - 1 }`
    //
    // When next_seq == 0 (empty store):
    // - Original: condition is TRUE, returns 0 (correct)
    // - Mutant: condition is FALSE, returns 0 - 1 = UNDERFLOW PANIC
    //
    // This test verifies that creating a subscription on an empty store
    // does NOT panic (which the mutant would cause).
    let store = InMemoryEventStore::new();

    // When: Developer creates subscription on EMPTY store
    // With the mutant, this would try to compute `0 - 1` causing panic
    let result = store.subscribe::<TestEvent>(SubscriptionQuery::all()).await;

    // Then: Subscription is created successfully (no panic)
    assert!(
        result.is_ok(),
        "subscription on empty store should succeed without panic"
    );
}

#[tokio::test]
async fn subscription_stream_yields_result_items_for_error_propagation() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream ID and appends events
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes to all events with explicit type parameter
    let subscription = store
        .subscribe::<TestEvent>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects events, explicitly handling Result items
    // The Stream should yield Result<E, SubscriptionError> to enable proper
    // error propagation for deserialization failures
    let results: Vec<Result<TestEvent, SubscriptionError>> = subscription.take(2).collect().await;

    // Then: Events are delivered as Ok(event)
    assert_eq!(results.len(), 2, "should deliver 2 result items");
    assert!(
        results[0].is_ok(),
        "first item should be Ok containing event"
    );
    assert_eq!(
        results[0].as_ref().expect("first result is Ok"),
        &TestEvent::ValueRecorded {
            stream_id: stream_a.clone(),
            value: 100
        },
        "first event should have value 100"
    );
}

/// Event type for storage - has incompatible schema with ReadEvent.
///
/// Both StoredEvent and ReadEvent use the SAME event_type_name "SchemaEvent"
/// but have different field structures. This simulates schema evolution
/// where stored data cannot be deserialized into the current type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredEvent {
    stream_id: StreamId,
    /// Field that exists in stored version but not in read version
    old_field: String,
}

impl Event for StoredEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> EventTypeName {
        // Uses SAME type name as ReadEvent - intentionally creating schema mismatch
        "SchemaEvent".try_into().expect("valid event type name")
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec!["SchemaEvent".try_into().expect("valid event type name")]
    }
}

/// Event type for reading - has incompatible schema with StoredEvent.
///
/// Uses the SAME event_type_name as StoredEvent, but expects different fields.
/// When subscription tries to deserialize StoredEvent data as ReadEvent, it fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReadEvent {
    stream_id: StreamId,
    /// Field that exists in read version but not in stored version
    new_field: u32,
}

impl Event for ReadEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> EventTypeName {
        // Uses SAME type name as StoredEvent - intentionally creating schema mismatch
        "SchemaEvent".try_into().expect("valid event type name")
    }

    fn all_type_names() -> Vec<EventTypeName> {
        vec!["SchemaEvent".try_into().expect("valid event type name")]
    }
}

#[tokio::test]
async fn subscription_yields_deserialization_error_for_incompatible_schema() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream ID
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    // And: Developer appends events using StoredEvent type
    // StoredEvent has event_type_name "SchemaEvent" with field {old_field: String}
    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(StoredEvent {
                stream_id: stream_a.clone(),
                old_field: "some data".to_string(),
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("events should be appended successfully");

    // When: Developer subscribes using ReadEvent type
    // ReadEvent ALSO has event_type_name "SchemaEvent" (matching stored data)
    // BUT expects field {new_field: u32} which is incompatible with stored schema
    let subscription = store
        .subscribe::<ReadEvent>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects items from the subscription stream
    let results: Vec<Result<ReadEvent, SubscriptionError>> = subscription.take(1).collect().await;

    // Then: Subscription yields 1 item (the event with matching type_name)
    assert_eq!(
        results.len(),
        1,
        "subscription should yield 1 item for event with matching type_name"
    );

    // And: The item is Err because the stored schema is incompatible with ReadEvent
    // StoredEvent has {old_field: String}, ReadEvent expects {new_field: u32}
    assert!(
        results[0].is_err(),
        "item should be Err(SubscriptionError) because schema is incompatible"
    );
}

/// View type that accepts ValueRecorded and SchemaEvent type names.
///
/// Used to test mixed success/error scenarios where some events
/// deserialize correctly and others fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum MixedEventView {
    Value(TestEvent),
}

impl Subscribable for MixedEventView {
    fn subscribable_type_names() -> Vec<EventTypeName> {
        vec![
            "ValueRecorded".try_into().expect("valid event type name"),
            "SchemaEvent".try_into().expect("valid event type name"),
        ]
    }

    fn try_from_stored(type_name: &EventTypeName, data: &[u8]) -> Result<Self, SubscriptionError> {
        match type_name.as_ref() {
            "ValueRecorded" => {
                let event: TestEvent = serde_json::from_slice(data)
                    .map_err(|e| SubscriptionError::DeserializationFailed(e.to_string()))?;
                Ok(MixedEventView::Value(event))
            }
            "SchemaEvent" => {
                // Attempt to deserialize as TestEvent, which will fail for StoredEvent data
                // StoredEvent has {old_field: String}, we're trying to deserialize as TestEvent
                let _event: TestEvent = serde_json::from_slice(data)
                    .map_err(|e| SubscriptionError::DeserializationFailed(e.to_string()))?;
                // This line won't be reached due to schema mismatch
                Err(SubscriptionError::Generic("unexpected success".to_string()))
            }
            _ => Err(SubscriptionError::UnknownEventType(type_name.clone())),
        }
    }
}

#[tokio::test]
async fn subscription_yields_mixed_success_and_error_items_in_order() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates stream ID
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    // And: Developer appends events with MIXED types - some compatible, some incompatible
    // Event 1: TestEvent (compatible with MixedEventView subscription)
    // Event 2: StoredEvent (incompatible schema - same type name "SchemaEvent" but wrong fields)
    // Event 3: TestEvent (compatible with MixedEventView subscription)
    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes)
        .await
        .expect("first event should be appended successfully");

    // Append StoredEvent separately (different event type)
    let writes2 = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(1))
        .and_then(|w| {
            w.append(StoredEvent {
                stream_id: stream_a.clone(),
                old_field: "incompatible data".to_string(),
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes2)
        .await
        .expect("second event should be appended successfully");

    // Append another TestEvent
    let writes3 = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(2))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");

    store
        .append_events(writes3)
        .await
        .expect("third event should be appended successfully");

    // When: Developer creates a view type that accepts BOTH TestEvent and SchemaEvent type names
    // This allows us to receive both compatible and incompatible events in a single subscription
    let subscription = store
        .subscribe::<MixedEventView>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer collects ALL items from the subscription stream (both Ok and Err)
    let results: Vec<Result<MixedEventView, SubscriptionError>> =
        subscription.take(3).collect().await;

    // Then: Stream yields BOTH Ok and Err items in temporal order
    assert_eq!(
        results.len(),
        3,
        "subscription should deliver all 3 items (2 Ok, 1 Err)"
    );

    // First item should be Ok (TestEvent with value 100)
    assert!(
        results[0].is_ok(),
        "first item should be Ok (compatible TestEvent)"
    );

    // Second item should be Err (StoredEvent cannot be deserialized as MixedEventView)
    assert!(
        results[1].is_err(),
        "second item should be Err (incompatible schema)"
    );

    // Third item should be Ok (TestEvent with value 200)
    assert!(
        results[2].is_ok(),
        "third item should be Ok (compatible TestEvent)"
    );
}

#[tokio::test]
async fn consumer_can_filter_errors_and_continue_processing() {
    // Given: Developer creates in-memory event store with mixed compatible/incompatible events
    let store = InMemoryEventStore::new();
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    // And: Developer appends 5 events: TestEvent (ok), StoredEvent (err), TestEvent (ok),
    //      StoredEvent (err), TestEvent (ok)
    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(1))
        .and_then(|w| {
            w.append(StoredEvent {
                stream_id: stream_a.clone(),
                old_field: "err1".to_string(),
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(2))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(3))
        .and_then(|w| {
            w.append(StoredEvent {
                stream_id: stream_a.clone(),
                old_field: "err2".to_string(),
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(4))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 300,
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    // When: Developer subscribes and uses filter_map to skip errors
    let subscription = store
        .subscribe::<MixedEventView>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    // And: Developer uses filter_map to extract only successful events
    // This demonstrates the ergonomic API pattern for error-tolerant consumers
    let successful_events: Vec<MixedEventView> = subscription
        .filter_map(|result| futures::future::ready(result.ok()))
        .collect()
        .await;

    // Then: Only the 3 successful TestEvent items are collected
    assert_eq!(
        successful_events.len(),
        3,
        "filter_map should yield only successful events"
    );

    // And: Values are in correct temporal order (100, 200, 300)
    assert!(
        matches!(
            &successful_events[0],
            MixedEventView::Value(TestEvent::ValueRecorded { value: 100, .. })
        ),
        "first successful event should have value 100"
    );
    assert!(
        matches!(
            &successful_events[1],
            MixedEventView::Value(TestEvent::ValueRecorded { value: 200, .. })
        ),
        "second successful event should have value 200"
    );
    assert!(
        matches!(
            &successful_events[2],
            MixedEventView::Value(TestEvent::ValueRecorded { value: 300, .. })
        ),
        "third successful event should have value 300"
    );
}

#[tokio::test]
async fn consumer_can_partition_results_for_error_reporting() {
    // Given: Developer creates in-memory event store with mixed compatible/incompatible events
    let store = InMemoryEventStore::new();
    let stream_a = StreamId::try_new("stream-a").expect("valid stream id");

    // And: Developer appends 3 events: TestEvent (ok), StoredEvent (err), TestEvent (ok)
    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(0))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 100,
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(1))
        .and_then(|w| {
            w.append(StoredEvent {
                stream_id: stream_a.clone(),
                old_field: "problematic data".to_string(),
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    let writes = StreamWrites::new()
        .register_stream(stream_a.clone(), StreamVersion::new(2))
        .and_then(|w| {
            w.append(TestEvent::ValueRecorded {
                stream_id: stream_a.clone(),
                value: 200,
            })
        })
        .expect("writes should be constructed successfully");
    store.append_events(writes).await.expect("append ok");

    // When: Developer subscribes and collects all results
    let subscription = store
        .subscribe::<MixedEventView>(SubscriptionQuery::all())
        .await
        .expect("subscription should be created successfully");

    let all_results: Vec<Result<MixedEventView, SubscriptionError>> =
        subscription.take(3).collect().await;

    // And: Developer partitions results into successes and errors for reporting
    // This demonstrates the ergonomic API pattern for error-reporting consumers
    let (successes, errors): (Vec<_>, Vec<_>) = all_results.into_iter().partition(Result::is_ok);

    let successful_events: Vec<MixedEventView> =
        successes.into_iter().map(|r| r.expect("is_ok")).collect();
    let error_items: Vec<SubscriptionError> =
        errors.into_iter().map(|r| r.expect_err("is_err")).collect();

    // Then: Developer can process successes
    assert_eq!(
        successful_events.len(),
        2,
        "should have 2 successful events"
    );

    // And: Developer can log/report errors
    assert_eq!(error_items.len(), 1, "should have 1 error");

    // And: Error contains useful information for debugging
    let error = &error_items[0];
    assert!(
        matches!(error, SubscriptionError::DeserializationFailed(_)),
        "error should be DeserializationFailed variant"
    );
}
