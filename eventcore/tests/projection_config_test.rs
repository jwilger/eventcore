//! Integration test for ADR-0037: ProjectionConfig via free function
//!
//! Scenario: run_projection_with_config with default config processes events
//! - Given a backend with seeded events
//! - When run_projection_with_config(projector, &backend, ProjectionConfig::default()) is called
//! - Then all events are processed (same behavior as run_projection)

use eventcore::{
    CheckpointStore, Event, EventFilter, EventPage, EventReader, EventStore, ProjectionConfig,
    StreamId, StreamPosition, StreamVersion, StreamWrites, run_projection_with_config,
};
use eventcore_memory::{InMemoryCheckpointStore, InMemoryEventStore, InMemoryProjectorCoordinator};
use eventcore_types::ProjectorCoordinator;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A simple event type for testing projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CounterIncremented {
    counter_id: StreamId,
}

impl Event for CounterIncremented {
    fn stream_id(&self) -> &StreamId {
        &self.counter_id
    }

    fn event_type_name() -> &'static str {
        "CounterIncremented"
    }
}

/// Minimal projector that counts events.
struct EventCounterProjector {
    count: Arc<AtomicUsize>,
}

impl EventCounterProjector {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl eventcore::Projector for EventCounterProjector {
    type Event = CounterIncremented;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let _ = self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "event-counter"
    }
}

/// Combined backend implementing all required traits for run_projection_with_config.
struct TestBackend {
    event_store: InMemoryEventStore,
    checkpoint_store: InMemoryCheckpointStore,
    coordinator: InMemoryProjectorCoordinator,
}

impl TestBackend {
    fn new() -> Self {
        Self {
            event_store: InMemoryEventStore::new(),
            checkpoint_store: InMemoryCheckpointStore::new(),
            coordinator: InMemoryProjectorCoordinator::new(),
        }
    }
}

impl EventReader for TestBackend {
    type Error = eventcore_types::EventStoreError;

    fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.event_store.read_events(filter, page)
    }
}

impl CheckpointStore for TestBackend {
    type Error = eventcore_memory::InMemoryCheckpointError;

    fn load(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<StreamPosition>, Self::Error>> + Send {
        self.checkpoint_store.load(name)
    }

    fn save(
        &self,
        name: &str,
        position: StreamPosition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.checkpoint_store.save(name, position)
    }
}

impl ProjectorCoordinator for TestBackend {
    type Error = eventcore_memory::InMemoryCoordinationError;
    type Guard = eventcore_memory::InMemoryCoordinationGuard;

    fn try_acquire(
        &self,
        subscription_name: &str,
    ) -> impl Future<Output = Result<Self::Guard, Self::Error>> + Send {
        self.coordinator.try_acquire(subscription_name)
    }
}

impl EventStore for TestBackend {
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<
        Output = Result<eventcore_types::EventStreamReader<E>, eventcore_types::EventStoreError>,
    > + Send {
        self.event_store.read_stream(stream_id)
    }

    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<
        Output = Result<eventcore_types::EventStreamSlice, eventcore_types::EventStoreError>,
    > + Send {
        self.event_store.append_events(writes)
    }
}

#[tokio::test]
async fn run_projection_with_config_with_default_config_processes_events() {
    // Given: A backend with seeded events
    let backend = TestBackend::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed two events into the store
    let event1 = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes1 = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event1)
        .expect("append event");
    let _ = backend
        .event_store
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
    let _ = backend
        .event_store
        .append_events(writes2)
        .await
        .expect("second append to succeed");

    // And: A minimal projector that counts events
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // When: run_projection_with_config is called with default config
    let config = ProjectionConfig::default();
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        run_projection_with_config(projector, &backend, config),
    )
    .await
    .expect("should complete within timeout");

    // Then: All events are processed (same behavior as run_projection)
    assert!(result.is_ok(), "run_projection_with_config should succeed");
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        2,
        "projector should have processed both events"
    );
}
