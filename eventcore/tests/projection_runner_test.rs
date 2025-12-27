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
    BatchSize, Event, EventFilter, EventPage, EventReader, EventStore, FailureContext,
    FailureStrategy, GuardTrait, HasTryAcquire, InMemoryCheckpointStore, LocalCoordinator,
    PollConfig, PollMode, ProjectionRunner, Projector, StreamId, StreamPosition, StreamVersion,
    StreamWrites,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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

/// Helper function to seed N events and return their positions.
/// Returns a Vec of (event, position) tuples.
async fn seed_events_and_get_positions(
    store: &InMemoryEventStore,
    counter_id: &StreamId,
    count: usize,
) -> Vec<StreamPosition> {
    // Append events
    for i in 0..count {
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

    // Read back all events to get their positions
    let all_events = store
        .read_events::<CounterIncremented>(
            EventFilter::all(),
            EventPage::first(BatchSize::new(100)),
        )
        .await
        .expect("read events to succeed");

    all_events.into_iter().map(|(_event, pos)| pos).collect()
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

    fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.inner.read_events(filter, page)
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

/// Integration test for eventcore-dvp: Fatal error handling in projection runner
///
/// Scenario: Projector encounters fatal error and stops processing
/// - Given projector configured to fail on specific event with FailureStrategy::Fatal
/// - And event store contains events before and after the failing event
/// - When runner processes events and encounters the failure
/// - Then runner stops immediately and returns error
/// - And checkpoint is updated only up to event before failure
/// - And events after the failure are not processed
#[tokio::test]
async fn runner_stops_on_fatal_error_and_preserves_checkpoint() {
    // Given: Event store with multiple events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 5 events and get their positions
    let positions = seed_events_and_get_positions(&store, &counter_id, 5).await;

    // And: A checkpoint store to track progress
    let checkpoint_store = InMemoryCheckpointStore::new();

    // And: A projector that fails fatally on the 3rd event (index 2)
    let processed_events = Arc::new(std::sync::Mutex::new(Vec::<StreamPosition>::new()));
    let projector = FatalErrorProjector::new(
        processed_events.clone(),
        positions[2], // Fail on 3rd event
    );

    // When: Developer runs the projector with checkpoint store
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_checkpoint_store(checkpoint_store.clone());

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), runner.run()).await;

    // Then: Runner returns an error (fatal error from projector)
    assert!(
        result.is_ok(),
        "runner should complete within timeout (not hang)"
    );
    assert!(
        result.unwrap().is_err(),
        "runner should return error on fatal failure"
    );

    // And: Only events before the failure were processed
    {
        let processed = processed_events.lock().unwrap();
        assert_eq!(
            processed.len(),
            2,
            "only events at positions 0 and 1 should be processed before fatal error at position 2"
        );
    }

    // And: Checkpoint was saved up to the last successfully processed event
    // When we restart with a fresh projector, it should resume from checkpoint
    // and immediately hit the fatal error again at the same position
    let restarted_processed = Arc::new(std::sync::Mutex::new(Vec::<StreamPosition>::new()));
    let restarted_projector = FatalErrorProjector::new(
        restarted_processed.clone(),
        positions[2], // Same failure point
    );

    let coordinator2 = LocalCoordinator::new();
    let runner2 = ProjectionRunner::new(restarted_projector, coordinator2, &store)
        .with_checkpoint_store(checkpoint_store);

    let result2 = tokio::time::timeout(std::time::Duration::from_secs(1), runner2.run()).await;

    // Then: Restarted runner also fails (checkpoint prevented reprocessing positions 0-1)
    assert!(
        result2.unwrap().is_err(),
        "restarted runner should also fail on same event"
    );

    // And: No events were reprocessed (checkpoint preserved)
    {
        let restarted = restarted_processed.lock().unwrap();
        assert_eq!(
            restarted.len(),
            0,
            "checkpoint should prevent reprocessing positions 0-1; runner fails at position 2"
        );
    }
}

/// Projector that fails with a fatal error at a specific position.
/// Uses on_error() to return FailureStrategy::Fatal.
struct FatalErrorProjector {
    processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>,
    fail_at_position: StreamPosition,
}

impl FatalErrorProjector {
    fn new(
        processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>,
        fail_at_position: StreamPosition,
    ) -> Self {
        Self {
            processed,
            fail_at_position,
        }
    }
}

impl Projector for FatalErrorProjector {
    type Event = CounterIncremented;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        if position == self.fail_at_position {
            return Err(format!("fatal error at position {}", position));
        }
        self.processed.lock().unwrap().push(position);
        Ok(())
    }

    fn name(&self) -> &str {
        "fatal-error-projector"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Fatal
    }
}

/// Integration test for eventcore-dvp: Skip failure strategy in projection runner
///
/// Scenario: Projector encounters error but chooses to skip and continue
/// - Given projector configured to fail on specific event with FailureStrategy::Skip
/// - And event store contains events before and after the failing event
/// - When runner processes events and encounters the failure
/// - Then runner logs the error but continues processing
/// - And checkpoint is updated past the skipped event
/// - And events after the failure are processed successfully
#[tokio::test]
async fn runner_skips_failed_event_and_continues_processing() {
    // Given: Event store with 5 events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 5 events and get their positions
    let positions = seed_events_and_get_positions(&store, &counter_id, 5).await;

    // And: A checkpoint store to track progress
    let checkpoint_store = InMemoryCheckpointStore::new();

    // And: A projector that fails at the 3rd event (index 2) but returns Skip strategy
    let processed_events = Arc::new(std::sync::Mutex::new(Vec::<StreamPosition>::new()));
    let projector = SkipErrorProjector::new(
        processed_events.clone(),
        positions[2], // Fail on 3rd event
    );

    // When: Developer runs the projector with checkpoint store
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_checkpoint_store(checkpoint_store.clone());

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), runner.run()).await;

    // Then: Runner completes successfully (does not return error)
    assert!(
        result.is_ok(),
        "runner should complete within timeout (not hang)"
    );
    assert!(
        result.unwrap().is_ok(),
        "runner should succeed even though event at position 2 failed (Skip strategy)"
    );

    // And: Events at indices 0, 1, 3, 4 were processed (4 events total)
    // Event at index 2 was skipped
    {
        let processed = processed_events.lock().unwrap();
        assert_eq!(
            processed.len(),
            4,
            "4 events should be processed (indices 0, 1, 3, 4); index 2 skipped"
        );
        assert_eq!(
            processed[0], positions[0],
            "first processed event should be at positions[0]"
        );
        assert_eq!(
            processed[1], positions[1],
            "second processed event should be at positions[1]"
        );
        assert_eq!(
            processed[2], positions[3],
            "third processed event should be at positions[3] (skipped index 2)"
        );
        assert_eq!(
            processed[3], positions[4],
            "fourth processed event should be at positions[4]"
        );
    }

    // And: Checkpoint was saved at last position (past the skipped event)
    // Verify by restarting - no events should be reprocessed
    let restarted_processed = Arc::new(std::sync::Mutex::new(Vec::<StreamPosition>::new()));
    let restarted_projector = SkipErrorProjector::new(
        restarted_processed.clone(),
        positions[2], // Same failure point (but won't be reached on restart)
    );

    let coordinator2 = LocalCoordinator::new();
    let runner2 = ProjectionRunner::new(restarted_projector, coordinator2, &store)
        .with_checkpoint_store(checkpoint_store);

    let result2 = tokio::time::timeout(std::time::Duration::from_secs(1), runner2.run()).await;

    // Then: Restarted runner completes successfully
    assert!(
        result2.unwrap().is_ok(),
        "restarted runner should succeed (checkpoint at position 4)"
    );

    // And: No events were reprocessed
    {
        let restarted = restarted_processed.lock().unwrap();
        assert_eq!(
            restarted.len(),
            0,
            "checkpoint at position 4 should prevent reprocessing all events"
        );
    }
}

/// Projector that fails at a specific position but returns Skip strategy.
/// Uses on_error() to return FailureStrategy::Skip.
struct SkipErrorProjector {
    processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>,
    fail_at_position: StreamPosition,
}

impl SkipErrorProjector {
    fn new(
        processed: Arc<std::sync::Mutex<Vec<StreamPosition>>>,
        fail_at_position: StreamPosition,
    ) -> Self {
        Self {
            processed,
            fail_at_position,
        }
    }
}

impl Projector for SkipErrorProjector {
    type Event = CounterIncremented;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        if position == self.fail_at_position {
            return Err(format!("error at position {} (will skip)", position));
        }
        self.processed.lock().unwrap().push(position);
        Ok(())
    }

    fn name(&self) -> &str {
        "skip-error-projector"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Skip
    }
}

/// Integration test for eventcore-dvp: Retry with escalation to Fatal
///
/// Scenario: Projector retries failed event with escalation to Fatal
/// - Given projector configured to fail on specific event with FailureStrategy::Retry
/// - And retry configuration has max_retries limit
/// - When runner processes events and encounters the failure
/// - Then runner retries the event (apply called multiple times for same event)
/// - And on_error receives incrementing retry counts (0, 1, 2, ...)
/// - And after max retries exceeded, runner escalates to Fatal and stops
/// - And checkpoint is NOT updated past the failed event
#[tokio::test]
async fn runner_retries_failed_event_then_escalates_to_fatal() {
    // Given: Event store with 5 events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 5 events and get their positions
    let positions = seed_events_and_get_positions(&store, &counter_id, 5).await;

    // And: A checkpoint store to track progress
    let checkpoint_store = InMemoryCheckpointStore::new();

    // And: A projector that tracks retry behavior
    let retry_log = Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = RetryThenFatalProjector::new(
        retry_log.clone(),
        apply_count.clone(),
        positions[2], // Fail on 3rd event
        3,            // Max 3 retries before escalating to Fatal
    );

    // When: Developer runs the projector with checkpoint store
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_checkpoint_store(checkpoint_store.clone());

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run()).await;

    // Then: Runner returns an error after retries exhausted
    assert!(
        result.is_ok(),
        "runner should complete within timeout (not hang)"
    );
    assert!(
        result.unwrap().is_err(),
        "runner should return error after retries exhausted"
    );

    // And: on_error was called multiple times with incrementing retry counts
    {
        let retry_counts = retry_log.lock().unwrap();
        assert_eq!(
            retry_counts.len(),
            4,
            "on_error should be called 4 times (initial failure + 3 retries)"
        );
        assert_eq!(
            *retry_counts,
            vec![0, 1, 2, 3],
            "retry counts should increment from 0 to 3"
        );
    }

    // And: apply() was called multiple times for position 2 (retries)
    // Initial attempt + 3 retries = 4 attempts at position 2
    // Plus 2 successful events at positions 0 and 1
    {
        let total_apply_calls = apply_count.load(Ordering::SeqCst);
        assert_eq!(
            total_apply_calls, 6,
            "apply should be called 2 times (positions 0,1) + 4 times (position 2 retries)"
        );
    }

    // And: Checkpoint was saved up to the last successfully processed event
    // Verify by restarting - should process positions 0-1, then fail at position 2 again
    let restarted_retry_log = Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
    let restarted_apply_count = Arc::new(AtomicUsize::new(0));
    let restarted_projector = RetryThenFatalProjector::new(
        restarted_retry_log.clone(),
        restarted_apply_count.clone(),
        positions[2], // Same failure point
        3,
    );

    let coordinator2 = LocalCoordinator::new();
    let runner2 = ProjectionRunner::new(restarted_projector, coordinator2, &store)
        .with_checkpoint_store(checkpoint_store);

    let result2 = tokio::time::timeout(std::time::Duration::from_secs(5), runner2.run()).await;

    // Then: Restarted runner also fails after retries
    assert!(
        result2.unwrap().is_err(),
        "restarted runner should also fail after retries exhausted"
    );

    // And: No events at positions 0-1 were reprocessed (checkpoint preserved)
    // Only position 2 is retried again
    {
        let restarted_apply = restarted_apply_count.load(Ordering::SeqCst);
        assert_eq!(
            restarted_apply, 4,
            "checkpoint should prevent reprocessing positions 0-1; only position 2 retried 4 times"
        );
    }
}

/// Projector that retries N times then escalates to Fatal.
/// Tracks retry counts passed to on_error() and number of apply() calls.
struct RetryThenFatalProjector {
    retry_log: Arc<std::sync::Mutex<Vec<u32>>>,
    apply_count: Arc<AtomicUsize>,
    fail_at_position: StreamPosition,
    max_retries: u32,
}

impl RetryThenFatalProjector {
    fn new(
        retry_log: Arc<std::sync::Mutex<Vec<u32>>>,
        apply_count: Arc<AtomicUsize>,
        fail_at_position: StreamPosition,
        max_retries: u32,
    ) -> Self {
        Self {
            retry_log,
            apply_count,
            fail_at_position,
            max_retries,
        }
    }
}

impl Projector for RetryThenFatalProjector {
    type Event = CounterIncremented;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.apply_count.fetch_add(1, Ordering::SeqCst);
        if position == self.fail_at_position {
            return Err(format!("transient error at position {}", position));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "retry-then-fatal-projector"
    }

    fn on_error(&mut self, ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        // Track the retry count we received from the runner
        self.retry_log.lock().unwrap().push(ctx.retry_count.into());

        // Return Retry until max_retries exceeded, then escalate to Fatal
        let retry_count_u32: u32 = ctx.retry_count.into();
        if retry_count_u32 < self.max_retries {
            FailureStrategy::Retry
        } else {
            FailureStrategy::Fatal
        }
    }
}

/// Integration test for eventcore-f71: Default poll configuration
///
/// Scenario: Developer uses default poll configuration
/// - Given developer creates ProjectionRunner without explicit PollConfig
/// - When runner executes polling loop
/// - Then runner uses sensible defaults (e.g., 100ms poll interval)
/// - And runner works correctly without configuration
#[tokio::test]
async fn runner_uses_default_poll_config_when_not_specified() {
    // Given: Developer has an event store with an event
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    let event = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // And: A minimal projector
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // When: Developer creates ProjectionRunner WITHOUT specifying PollConfig
    // (relying on default configuration via PollConfig::default())
    let coordinator = LocalCoordinator::new();
    let default_config = PollConfig::default();
    let runner =
        ProjectionRunner::new(projector, coordinator, &store).with_poll_config(default_config);

    // And: Runner executes
    tokio::time::timeout(std::time::Duration::from_secs(1), runner.run())
        .await
        .expect("runner should complete within timeout")
        .expect("runner should succeed");

    // Then: Runner used default poll configuration and processed the event successfully
    assert_eq!(event_count.load(Ordering::SeqCst), 1);
}

/// Integration test for EventReader blanket implementation
///
/// Scenario: EventReader blanket impl forwards read_events() correctly
/// - Given event store with some events
/// - When read_events() is called on a REFERENCE to the store (&store)
/// - Then the blanket impl forwards to the underlying store's read_events()
/// - And returns filtered/paginated events (not an empty vec)
///
/// This test catches mutations in the blanket impl that would
/// change `(*self).read_events(...)` to `Ok(vec![])`.
#[tokio::test]
async fn event_reader_blanket_impl_forwards_read_events_correctly() {
    // Given: Event store with 3 events
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 3 events into the store
    for i in 0..3 {
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

    // When: read_events() is called on a reference to the store
    // This exercises the blanket impl: impl<T: EventReader + Sync> EventReader for &T
    let store_ref: &InMemoryEventStore = &store;
    let filter = EventFilter::all();
    let page = EventPage::first(BatchSize::new(10));
    let events: Vec<(CounterIncremented, StreamPosition)> = store_ref
        .read_events(filter, page)
        .await
        .expect("read should succeed");

    // Then: The blanket impl forwards correctly and returns all events
    assert_eq!(
        events.len(),
        3,
        "blanket impl should forward read_events() and return all 3 events, not an empty vec"
    );
}

/// Integration test for eventcore-f71: Custom poll interval configuration
///
/// Scenario: Developer configures custom poll interval
/// - Given developer creates PollConfig with poll_interval of 500ms
/// - When runner polls and finds events
/// - Then runner waits 500ms before next poll
/// - And polling cadence matches configuration
#[tokio::test]
async fn runner_respects_custom_poll_interval_when_events_found() {
    // Given: Event store with an event
    let store = Arc::new(InMemoryEventStore::new());
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    let event = CounterIncremented {
        counter_id: counter_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(counter_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // And: A projector that counts events
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // And: A poll-counting wrapper to track poll timing
    let poll_times = Arc::new(std::sync::Mutex::new(Vec::<Instant>::new()));
    let counting_reader = TimingPollCountingReader::new(store.clone(), poll_times.clone());

    // When: Developer creates runner with custom poll_interval of 500ms
    let config = PollConfig {
        poll_interval: Duration::from_millis(500),
        ..Default::default()
    };
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, counting_reader)
        .with_poll_config(config)
        .with_poll_mode(PollMode::Continuous);

    // And: Runner runs for enough time to observe multiple polls with events
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    let runner_handle = tokio::spawn(async move {
        tokio::select! {
            result = runner.run() => result,
            _ = cancel_rx => Ok(()),
        }
    });

    // Wait long enough for at least 2 polls (500ms interval * 2 = 1000ms, plus buffer)
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let _ = cancel_tx.send(());
    runner_handle
        .await
        .expect("runner task panicked")
        .expect("runner failed");

    // Then: The interval between polls should be approximately poll_interval duration
    let times = poll_times.lock().unwrap();
    assert!(
        times.len() >= 2,
        "expected at least 2 polls, got {}",
        times.len()
    );

    // Verify interval between first and second poll is approximately 500ms
    let interval = times[1].duration_since(times[0]).as_millis();
    assert!(
        (450..=550).contains(&interval),
        "expected poll interval ~500ms when events found, got {}ms",
        interval
    );
}

/// Integration test for eventcore-f71: Custom empty poll backoff
///
/// Scenario: Developer configures empty poll backoff
/// - Given developer creates PollConfig with empty_poll_backoff of 1 second
/// - When runner polls and finds no new events
/// - Then runner waits 1 second before next poll
/// - And backoff reduces unnecessary database load
#[tokio::test]
async fn runner_respects_custom_empty_poll_backoff_when_no_events() {
    // Given: An event store with NO events (empty poll results)
    let store = Arc::new(InMemoryEventStore::new());

    // And: A projector that counts events
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // And: A poll-counting wrapper to track poll timing
    let poll_times = Arc::new(std::sync::Mutex::new(Vec::<Instant>::new()));
    let counting_reader = TimingPollCountingReader::new(store.clone(), poll_times.clone());

    // When: Developer creates runner with custom empty_poll_backoff of 1 second
    let config = PollConfig {
        empty_poll_backoff: Duration::from_secs(1),
        ..Default::default()
    };
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, counting_reader)
        .with_poll_config(config)
        .with_poll_mode(PollMode::Continuous);

    // And: Runner runs for enough time to observe multiple empty polls
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    let runner_handle = tokio::spawn(async move {
        tokio::select! {
            result = runner.run() => result,
            _ = cancel_rx => Ok(()),
        }
    });

    // Wait long enough for at least 2 polls (1 second interval * 2 = 2 seconds, plus buffer)
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let _ = cancel_tx.send(());
    runner_handle
        .await
        .expect("runner task panicked")
        .expect("runner failed");

    // Then: The interval between polls should be approximately 1 second (empty_poll_backoff)
    let times = poll_times.lock().unwrap();
    assert!(
        times.len() >= 2,
        "expected at least 2 polls, got {}",
        times.len()
    );

    // Verify interval between first and second poll is approximately 1000ms (±10% tolerance)
    let interval = times[1].duration_since(times[0]).as_millis();
    assert!(
        (900..=1100).contains(&interval),
        "expected empty poll backoff ~1000ms when no events found, got {}ms",
        interval
    );
}

/// Wrapper around EventReader that tracks poll timing.
struct TimingPollCountingReader<S> {
    inner: Arc<S>,
    poll_times: Arc<std::sync::Mutex<Vec<Instant>>>,
}

impl<S> TimingPollCountingReader<S> {
    fn new(inner: Arc<S>, poll_times: Arc<std::sync::Mutex<Vec<Instant>>>) -> Self {
        Self { inner, poll_times }
    }
}

impl<S: EventReader + Sync> EventReader for TimingPollCountingReader<S> {
    type Error = S::Error;

    fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.poll_times.lock().unwrap().push(Instant::now());
        self.inner.read_events(filter, page)
    }
}
