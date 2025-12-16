use eventcore::{EventSubscription, InMemoryEventStore, StreamPrefix, SubscriptionQuery};

/// Integration test for eventcore-a43: Subscription Observability
///
/// This test verifies that creating a subscription emits a tracing span
/// with the subscription query parameters. This enables operations teams
/// to monitor subscription creation, filter patterns, and debug subscription
/// behavior in production systems.
///
/// Following the pattern from I-012 observability tests for command execution.
#[tokio::test]
#[tracing_test::traced_test]
async fn subscribe_emits_tracing_span_with_query_parameters() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer builds a subscription query with stream prefix filter
    let query = SubscriptionQuery::all()
        .filter_stream_prefix(StreamPrefix::try_new("account-").expect("valid stream prefix"));

    // When: Developer creates a subscription
    let _subscription = store
        .subscribe::<DummyEvent>(query)
        .await
        .expect("subscription should be created successfully");

    // Then: A tracing span named "subscribe" is recorded
    assert!(
        logs_contain("subscribe"),
        "should emit a span named 'subscribe' when creating subscription"
    );

    // And: The span includes the stream_prefix filter parameter
    assert!(
        logs_contain("stream_prefix="),
        "subscribe span should include stream_prefix field"
    );

    // And: The stream_prefix field contains the actual filter value
    assert!(
        logs_contain("account-"),
        "stream_prefix field should contain the filter value"
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn subscribe_emits_event_type_filter_in_span() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer builds a subscription query with event type name filter
    let query = SubscriptionQuery::all()
        .filter_event_type_name("MoneyDeposited".try_into().expect("valid event type name"));

    // When: Developer creates a subscription
    let _subscription = store
        .subscribe::<DummyEvent>(query)
        .await
        .expect("subscription should be created successfully");

    // Then: The span includes the event_type_filter parameter
    assert!(
        logs_contain("event_type_filter="),
        "subscribe span should include event_type_filter field"
    );

    // And: The event_type_filter field contains the actual filter value
    assert!(
        logs_contain("MoneyDeposited"),
        "event_type_filter field should contain the filter value"
    );
}

/// Minimal dummy event for subscription testing.
///
/// Only needed to satisfy the Subscribable type parameter - test focuses
/// on tracing spans, not event delivery.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DummyEvent {
    id: String,
}

impl eventcore::Event for DummyEvent {
    fn stream_id(&self) -> &eventcore::StreamId {
        // This won't be called - test only exercises subscribe() tracing
        unimplemented!("DummyEvent.stream_id not needed for tracing test")
    }

    fn event_type_name(&self) -> eventcore::EventTypeName {
        "DummyEvent".try_into().expect("valid event type name")
    }

    fn all_type_names() -> Vec<eventcore::EventTypeName> {
        vec!["DummyEvent".try_into().expect("valid event type name")]
    }
}
