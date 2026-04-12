//! Integration tests for the run_projection() public API.
//!
//! These tests verify the public-facing projection entry point:
//! - Leadership coordination via ProjectorCoordinator
//! - Event processing via EventReader
//! - Checkpoint management via CheckpointStore

use eventcore::{Event, Projector, StreamId, StreamPosition, run_projection};
use eventcore_memory::{InMemoryCheckpointStore, InMemoryEventStore, InMemoryProjectorCoordinator};
use eventcore_types::{
    CheckpointStore, EventFilter, EventPage, EventReader, EventStore, EventStoreError,
    EventStreamReader, EventStreamSlice, ProjectorCoordinator, StreamVersion, StreamWrites,
};
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

/// Minimal projector that counts events processed.
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
        let _ = self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "event-counter"
    }
}

// ============================================================================
// ADR-029: run_projection free function tests
// ============================================================================

/// Combined backend implementing EventReader + CheckpointStore + ProjectorCoordinator.
///
/// Per ADR-029, the `run_projection` free function accepts a single backend reference
/// that implements all three traits. This wrapper combines the separate in-memory
/// implementations for testing purposes.
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
    type Error = EventStoreError;

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
    ) -> impl Future<Output = Result<EventStreamReader<E>, EventStoreError>> + Send {
        self.event_store.read_stream(stream_id)
    }

    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<Output = Result<EventStreamSlice, EventStoreError>> + Send {
        self.event_store.append_events(writes)
    }
}

/// Integration test for ADR-029: run_projection returns LeadershipError when lock held
///
/// Scenario: run_projection returns LeadershipError when leadership cannot be acquired
/// - Given a backend where leadership is already held by another process
/// - When run_projection is called
/// - Then it should return ProjectionError::LeadershipError
#[tokio::test]
async fn run_projection_returns_leadership_error_when_lock_already_held() {
    // Given: A backend with leadership already held for the projector's subscription
    let backend = TestBackend::new();

    // Pre-acquire the lock to simulate another process holding leadership
    let _held_lock = backend
        .coordinator
        .try_acquire("event-counter")
        .await
        .expect("should acquire lock");

    // And: A projector that would use that same subscription name
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count);

    // When: run_projection is called
    let result = run_projection(projector, &backend).await;

    // Then: It should return a LeadershipError
    assert!(
        matches!(result, Err(eventcore::ProjectionError::LeadershipError(_))),
        "expected LeadershipError, got {:?}",
        result
    );
}

/// Integration test for ADR-029: Leadership guard held during event processing
///
/// Scenario: Leadership is maintained while run_projection processes events
/// - Given a backend with events to process
/// - And a projector that blocks during apply() to allow lock verification
/// - When run_projection is called and begins processing
/// - Then another attempt to acquire leadership for the same projector should fail
#[tokio::test]
async fn run_projection_holds_leadership_during_event_processing() {
    // Given: A shared backend that all tasks can access
    let backend = Arc::new(TestBackend::new());
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // And: Seed an event into the store
    let event = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    let _ = backend
        .event_store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // And: Synchronization primitives for coordinating with the projector
    // - started: signals that apply() has begun (guard is held)
    // - can_finish: allows apply() to complete after we've checked the lock
    let started = Arc::new(std::sync::Barrier::new(2));
    let can_finish = Arc::new(std::sync::Barrier::new(2));
    let projector = BlockingProjector::new(started.clone(), can_finish.clone());

    // When: run_projection is called in a separate thread (blocking projector needs real thread)
    let backend_clone = backend.clone();
    let projection_handle = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_projection(projector, backend_clone.as_ref()))
    });

    // And: Wait for the projector to start processing (it's now holding the guard)
    let _ = started.wait();

    // Then: Another attempt to acquire leadership for the same projector should fail
    let second_acquire_result = backend.coordinator.try_acquire("blocking-projector").await;
    assert!(
        second_acquire_result.is_err(),
        "second attempt to acquire leadership should fail while first projection is processing"
    );

    // Cleanup: Allow the projector to finish and wait for the thread
    let _ = can_finish.wait();
    let _ = projection_handle.join();
}

/// Projector that blocks during apply() using barriers for synchronization.
/// Used to test that leadership is held during event processing.
struct BlockingProjector {
    started: Arc<std::sync::Barrier>,
    can_finish: Arc<std::sync::Barrier>,
}

impl BlockingProjector {
    fn new(started: Arc<std::sync::Barrier>, can_finish: Arc<std::sync::Barrier>) -> Self {
        Self {
            started,
            can_finish,
        }
    }
}

impl Projector for BlockingProjector {
    type Event = CounterIncremented;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        // Signal that we've started processing (guard is held at this point)
        let _ = self.started.wait();
        // Wait for permission to finish (allows test to check the lock)
        let _ = self.can_finish.wait();
        Ok(())
    }

    fn name(&self) -> &str {
        "blocking-projector"
    }
}

/// Integration test for ADR-029: run_projection free function API
///
/// Scenario: Developer uses simplified run_projection API
/// - Given a projector implementing the Projector trait
/// - And a backend implementing EventReader + CheckpointStore + ProjectorCoordinator
/// - When run_projection is called with the projector and backend
/// - Then it should acquire leadership, process events, and manage checkpoints
#[tokio::test]
async fn run_projection_acquires_leadership_and_processes_events() {
    // Given: A backend implementing all required traits
    let backend = TestBackend::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // And: Seed one event into the store
    let event = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    let _ = backend
        .event_store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // And: A minimal projector that counts events
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // When: Developer calls run_projection with projector and backend
    let result = tokio::time::timeout(Duration::from_secs(1), run_projection(projector, &backend))
        .await
        .expect("run_projection should complete within timeout");

    // Then: run_projection succeeds
    assert!(result.is_ok(), "run_projection should succeed");

    // And: The event was processed
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        1,
        "projector should have processed one event"
    );
}
