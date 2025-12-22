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
    Event, EventReader, EventStore, InMemoryCheckpointStore, InMemoryEventStore, LocalCoordinator,
    PollMode, ProjectionRunner, Projector, StreamId, StreamPosition, StreamVersion, StreamWrites,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
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

/// Integration test for eventcore-dvp: Projection Runner with Checkpoint Resumption
///
/// Scenario: Developer projector resumes from checkpoint after restart
/// - Given projector previously processed events up to position 42
/// - And projector stored checkpoint at position 42
/// - When projector restarts and calls runner.run()
/// - Then runner polls for events after position 42
/// - And previously processed events are not reprocessed
#[tokio::test]
async fn projector_resumes_from_checkpoint_after_restart() {
    // Given: Developer has an event store with 5 events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 5 events into the store
    for i in 0..5 {
        let event = CounterIncremented {
            counter_id: counter_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(counter_id.clone(), StreamVersion::new(i))
            .expect("register stream")
            .append(event)
            .expect("append event");
        store
            .append_events(writes)
            .await
            .expect("append to succeed");
    }

    // And: A shared checkpoint store that persists across "restarts"
    // InMemoryCheckpointStore implements CheckpointStore trait
    let checkpoint_store = InMemoryCheckpointStore::new();

    // And: A projector that tracks which events it processes
    let processed_events = Arc::new(std::sync::Mutex::new(Vec::<StreamPosition>::new()));
    let projector = TrackingProjector::new(processed_events.clone());

    // When: Developer runs the projector the first time with checkpoint store
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_checkpoint_store(checkpoint_store.clone());

    tokio::time::timeout(std::time::Duration::from_secs(1), runner.run())
        .await
        .expect("runner should complete within timeout")
        .expect("runner should succeed");

    // Then: All 5 events were processed
    assert_eq!(processed_events.lock().unwrap().len(), 5);

    // Clear the tracking to simulate a fresh projector instance
    processed_events.lock().unwrap().clear();

    // When: Developer "restarts" - creates a new projector instance and runs again
    // The checkpoint store persists across restarts
    let restarted_projector = TrackingProjector::new(processed_events.clone());
    let coordinator2 = LocalCoordinator::new();

    // Use the same checkpoint store - it remembers where we left off
    let runner2 = ProjectionRunner::new(restarted_projector, coordinator2, &store)
        .with_checkpoint_store(checkpoint_store);

    tokio::time::timeout(std::time::Duration::from_secs(1), runner2.run())
        .await
        .expect("runner should complete within timeout")
        .expect("runner should succeed");

    // Then: No events were reprocessed (since no new events were added)
    assert_eq!(
        processed_events.lock().unwrap().len(),
        0,
        "previously processed events should not be reprocessed after restart"
    );
}

/// Integration test for eventcore-dvp: Empty poll handling with backoff
///
/// Scenario: Developer projector handles empty poll results
/// - Given projector is caught up with all events
/// - When runner polls and receives empty result
/// - Then runner waits before polling again (backoff)
/// - And runner does not call projector.apply()
#[tokio::test]
async fn runner_waits_before_polling_again_when_no_events() {
    // Given: An event store with no events (projector is caught up)
    let store = Arc::new(InMemoryEventStore::new());

    // And: A projector that tracks apply() calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCountingProjector::new(apply_count.clone());

    // And: A poll-counting wrapper around the store to observe poll behavior
    let poll_count = Arc::new(AtomicUsize::new(0));
    let counting_reader = PollCountingReader::new(store.clone(), poll_count.clone());

    // When: Developer creates runner in continuous polling mode
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, counting_reader)
        .with_poll_mode(PollMode::Continuous);

    // And: Runner runs for a short time with empty store
    // Use a cancellation token to stop after observing behavior
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    let runner_handle = tokio::spawn(async move {
        tokio::select! {
            result = runner.run() => result,
            _ = cancel_rx => Ok(()),
        }
    });

    // Wait long enough to observe multiple poll attempts with backoff
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let _ = cancel_tx.send(());
    runner_handle
        .await
        .expect("runner task panicked")
        .expect("runner failed");

    // Then: Store was polled multiple times (continuous polling)
    let polls = poll_count.load(Ordering::SeqCst);
    assert!(
        polls >= 2,
        "expected at least 2 polls during 150ms, got {}",
        polls
    );

    // And: apply() was never called (no events to process)
    assert_eq!(
        apply_count.load(Ordering::SeqCst),
        0,
        "apply() should not be called when there are no events"
    );

    // And: Backoff is happening (not spinning - with 150ms wait and default backoff,
    // we should see limited polls, not hundreds)
    assert!(
        polls < 20,
        "expected backoff to limit polls, but got {} polls in 150ms (spinning?)",
        polls
    );
}

/// Projector that counts apply() calls.
struct ApplyCountingProjector {
    count: Arc<AtomicUsize>,
}

impl ApplyCountingProjector {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl Projector for ApplyCountingProjector {
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
        "apply-counting-projector"
    }
}

/// Wrapper around EventReader that counts poll operations.
struct PollCountingReader<S> {
    inner: Arc<S>,
    poll_count: Arc<AtomicUsize>,
}

impl<S> PollCountingReader<S> {
    fn new(inner: Arc<S>, poll_count: Arc<AtomicUsize>) -> Self {
        Self { inner, poll_count }
    }
}

impl<S: EventReader + Sync> EventReader for PollCountingReader<S> {
    type Error = S::Error;

    fn read_all<E: Event>(
        &self,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.inner.read_all()
    }

    fn read_after<E: Event>(
        &self,
        after_position: StreamPosition,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.inner.read_after(after_position)
    }
}

/// Projector that tracks which events it processes (by position).
struct TrackingProjector {
    processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>,
}

impl TrackingProjector {
    fn new(processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>) -> Self {
        Self { processed }
    }
}

impl Projector for TrackingProjector {
    type Event = CounterIncremented;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.processed.lock().unwrap().push(position);
        Ok(())
    }

    fn name(&self) -> &str {
        "tracking-projector"
    }
}

/// Integration test for eventcore-dvp: LocalCoordinator for single-process leadership
///
/// Scenario: Developer uses LocalCoordinator for single-process deployment
/// - Given developer creates LocalCoordinator::new()
/// - When developer calls try_acquire()
/// - Then it returns Some(guard) (leadership always granted in single-process)
/// - And guard.is_valid() returns true
/// - When guard is dropped
/// - Then subsequent try_acquire() succeeds
#[tokio::test]
async fn local_coordinator_grants_leadership_without_contention() {
    // Given: Developer creates a LocalCoordinator for single-process deployment
    let coordinator = LocalCoordinator::new();

    // When: Developer tries to acquire leadership
    let guard = coordinator
        .try_acquire()
        .await
        .expect("LocalCoordinator should always grant leadership");

    // Then: Leadership is granted
    assert!(
        guard.is_valid(),
        "acquired guard should indicate valid leadership"
    );

    // When: Guard is dropped (RAII pattern releases leadership)
    drop(guard);

    // Then: Subsequent acquire succeeds (leadership can be re-acquired)
    let guard2 = coordinator
        .try_acquire()
        .await
        .expect("LocalCoordinator should grant leadership again after release");

    assert!(guard2.is_valid(), "re-acquired guard should also be valid");
}
