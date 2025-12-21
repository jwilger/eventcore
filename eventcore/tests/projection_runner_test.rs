//! Integration test for eventcore-dvp: Projection Runner with Local Coordinator
//!
//! Scenario: Developer creates minimal working projector
//! - Given developer implements Projector trait with only apply() and name() methods
//! - And developer has an EventStore that implements EventReader
//! - When developer creates LocalCoordinator::new()
//! - And developer creates ProjectionRunner with projector, coordinator, and event store
//! - And developer calls runner.run()
//! - Then projector starts and processes events
//! - And all configuration uses sensible defaults
//! - And developer can get a working projection with minimal code

use eventcore::{
    Event, EventStore, InMemoryEventStore, LocalCoordinator, ProjectionRunner, Projector, StreamId,
    StreamPosition, StreamVersion, StreamWrites,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A simple event type for testing projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CounterIncremented {
    counter_id: StreamId,
}

impl Event for CounterIncremented {
    fn stream_id(&self) -> &StreamId {
        &self.counter_id
    }
}

/// Minimal projector that counts events.
/// Implements only the required methods: apply() and name().
struct EventCounterProjector {
    count: Arc<AtomicUsize>,
}

impl EventCounterProjector {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl Projector for EventCounterProjector {
    type Event = CounterIncremented;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "event-counter"
    }
}

#[tokio::test]
async fn minimal_projector_processes_events_with_sensible_defaults() {
    // Given: Developer has an event store with some events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed some events into the store
    let event1 = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes1 = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event1)
        .expect("append event");
    store
        .append_events(writes1)
        .await
        .expect("append to succeed");

    let event2 = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes2 = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(1))
        .expect("register stream")
        .append(event2)
        .expect("append event");
    store
        .append_events(writes2)
        .await
        .expect("second append to succeed");

    // And: Developer creates a minimal projector (just apply and name)
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // When: Developer creates LocalCoordinator with sensible defaults
    let coordinator = LocalCoordinator::new();

    // And: Developer creates ProjectionRunner with minimal configuration
    let runner = ProjectionRunner::new(projector, coordinator, &store);

    // And: Developer runs the projection (with timeout for test)
    tokio::time::timeout(std::time::Duration::from_secs(1), runner.run())
        .await
        .expect("runner should complete within timeout")
        .expect("runner should succeed");

    // Then: Both events were processed
    assert_eq!(event_count.load(Ordering::SeqCst), 2);
}
