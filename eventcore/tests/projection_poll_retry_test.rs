//! Integration test for eventcore-a5a: Database poll retry with exponential backoff
//!
//! Scenario: Developer handles transient database errors during event polling
//! - Given projector is running in batch poll mode
//! - When database read fails transiently (connection timeout, etc)
//! - Then runner retries with exponential backoff
//! - And after max consecutive failures, propagates error to caller
//! - And caller can decide recovery strategy (restart, alert, etc)

use eventcore::{
    Event, EventReader, EventStore, InMemoryCheckpointStore, InMemoryEventStore, LocalCoordinator,
    PollMode, ProjectionRunner, Projector, StreamId, StreamPosition, StreamVersion, StreamWrites,
    SubscriptionQuery,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A simple event type for testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

/// Error type for the failing mock reader.
#[derive(Debug, Clone)]
struct MockDatabaseError(String);

impl std::fmt::Display for MockDatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MockDatabaseError {}

/// Mock EventReader that fails N times then succeeds.
/// Tracks consecutive poll failures and delays to verify backoff behavior.
struct FailNTimesReader {
    store: Arc<InMemoryEventStore>,
    failures_remaining: Arc<AtomicUsize>,
    poll_count: Arc<AtomicUsize>,
    poll_times: Arc<Mutex<Vec<Instant>>>,
}

impl EventReader for FailNTimesReader {
    type Error = MockDatabaseError;

    async fn read_all<E>(&self) -> Result<Vec<(E, StreamPosition)>, Self::Error>
    where
        E: Event,
    {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.poll_times.lock().unwrap().push(Instant::now());

        let remaining = self.failures_remaining.load(Ordering::SeqCst);
        if remaining > 0 {
            self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            // Simulate transient database error with a proper error type
            return Err(MockDatabaseError(
                "transient database connection timeout".to_string(),
            ));
        }

        // Delegate to wrapped store after failures exhausted
        self.store
            .read_all()
            .await
            .map_err(|_| MockDatabaseError("unexpected store error".to_string()))
    }

    async fn read_after<E>(
        &self,
        position: StreamPosition,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error>
    where
        E: Event,
    {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.poll_times.lock().unwrap().push(Instant::now());

        let remaining = self.failures_remaining.load(Ordering::SeqCst);
        if remaining > 0 {
            self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            // Simulate transient database error with a proper error type
            return Err(MockDatabaseError(
                "transient database connection timeout".to_string(),
            ));
        }

        // Delegate to wrapped store after failures exhausted
        self.store
            .read_after(position)
            .await
            .map_err(|_| MockDatabaseError("unexpected store error".to_string()))
    }

    async fn read_events_after<E>(
        &self,
        _query: SubscriptionQuery,
        _after_position: Option<StreamPosition>,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error>
    where
        E: Event,
    {
        unimplemented!()
    }
}

/// Minimal projector that tracks apply calls.
struct ApplyCounterProjector {
    apply_count: Arc<AtomicUsize>,
}

impl Projector for ApplyCounterProjector {
    type Event = TestEvent;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.apply_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "apply-counter"
    }
}

#[tokio::test]
async fn runner_retries_transient_database_errors_with_exponential_backoff() {
    // Given: Event store with one event
    let store = Arc::new(InMemoryEventStore::new());
    let stream_id = StreamId::try_new("test-1").expect("valid stream id");
    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store.append_events(writes).await.expect("append succeeds");

    // And: Mock reader that fails 3 times then succeeds
    let poll_count = Arc::new(AtomicUsize::new(0));
    let poll_times_handle = Arc::new(Mutex::new(Vec::new()));
    let reader = FailNTimesReader {
        store,
        failures_remaining: Arc::new(AtomicUsize::new(3)),
        poll_count: poll_count.clone(),
        poll_times: poll_times_handle.clone(),
    };

    // And: Projector that tracks apply calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: Runner processes with failing reader
    let coordinator = LocalCoordinator::new();
    let runner =
        ProjectionRunner::new(projector, coordinator, reader).with_poll_mode(PollMode::Batch);

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .expect("Batch mode should complete, not loop forever");

    // Then: Runner succeeds after retries
    assert!(result.is_ok());

    // And: Database was polled 4 times (3 failures + 1 success)
    assert_eq!(poll_count.load(Ordering::SeqCst), 4);

    // And: Event was successfully applied once
    assert_eq!(apply_count.load(Ordering::SeqCst), 1);

    // And: Polls had exponential backoff between them
    let poll_times = poll_times_handle.lock().unwrap();
    assert_eq!(poll_times.len(), 4);

    // Verify exponential backoff with formula: BASE_DELAY_MS * 2^(consecutive_failures - 1)
    // Expected delays: 10ms * 2^0 = 10ms, 10ms * 2^1 = 20ms, 10ms * 2^2 = 40ms
    // Allow tolerance for CI overhead but verify the formula is correct
    let delay_1 = poll_times[1].duration_since(poll_times[0]).as_millis();
    let delay_2 = poll_times[2].duration_since(poll_times[1]).as_millis();
    let delay_3 = poll_times[3].duration_since(poll_times[2]).as_millis();

    // First delay should be approximately 10ms (BASE_DELAY_MS * 2^0)
    // Range 5-18ms catches mutants that would produce 20ms or 40ms
    assert!(
        (5..=18).contains(&delay_1),
        "First delay should be ~10ms (2^0), got {}ms",
        delay_1
    );

    // Second delay should be approximately 20ms (BASE_DELAY_MS * 2^1)
    // Range 15-35ms catches mutants that would produce 40ms or 80ms
    assert!(
        (15..=35).contains(&delay_2),
        "Second delay should be ~20ms (2^1), got {}ms",
        delay_2
    );

    // Third delay should be approximately 40ms (BASE_DELAY_MS * 2^2)
    // Range 30-70ms catches mutants that would produce 80ms or 160ms
    assert!(
        (30..=70).contains(&delay_3),
        "Third delay should be ~40ms (2^2), got {}ms",
        delay_3
    );

    // Also verify exponential growth relationship
    assert!(
        delay_2 as f64 >= delay_1 as f64 * 1.5,
        "Second delay ({}ms) should be at least 1.5x first delay ({}ms)",
        delay_2,
        delay_1
    );
    assert!(
        delay_3 as f64 >= delay_2 as f64 * 1.5,
        "Third delay ({}ms) should be at least 1.5x second delay ({}ms)",
        delay_3,
        delay_2
    );
}

#[tokio::test]
async fn runner_propagates_error_after_max_consecutive_poll_failures() {
    // Given: Event store (content doesn't matter - reader always fails)
    let store = Arc::new(InMemoryEventStore::new());

    // And: Mock reader that fails 6 times (exceeds max retries of 5)
    let poll_count = Arc::new(AtomicUsize::new(0));
    let reader = FailNTimesReader {
        store,
        failures_remaining: Arc::new(AtomicUsize::new(6)),
        poll_count: poll_count.clone(),
        poll_times: Arc::new(Mutex::new(Vec::new())),
    };

    // And: Projector that tracks apply calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: Runner processes with perpetually failing reader
    let coordinator = LocalCoordinator::new();
    let runner =
        ProjectionRunner::new(projector, coordinator, reader).with_poll_mode(PollMode::Batch);

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .expect("Batch mode should complete, not loop forever");

    // Then: Runner returns error after max retries
    assert!(result.is_err());

    // And: Database was polled exactly max_retries + 1 times (5 retries after initial failure)
    // Initial attempt + 5 retries = 6 total polls
    assert_eq!(poll_count.load(Ordering::SeqCst), 6);

    // And: No events were applied (never got past database read)
    assert_eq!(apply_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runner_resets_consecutive_failure_count_on_successful_poll() {
    // Given: Event store with two events at different positions
    let store = Arc::new(InMemoryEventStore::new());
    let stream_id = StreamId::try_new("test-1").expect("valid stream id");

    // Append first event
    let event1 = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes1 = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event1)
        .expect("append event");
    store
        .append_events(writes1)
        .await
        .expect("first append succeeds");

    // And: Mock reader that fails 3 times
    // After 3 failures + 1 success = 4 polls, consecutive failure count should reset
    let poll_count = Arc::new(AtomicUsize::new(0));
    let reader = FailNTimesReader {
        store: store.clone(),
        failures_remaining: Arc::new(AtomicUsize::new(3)),
        poll_count: poll_count.clone(),
        poll_times: Arc::new(Mutex::new(Vec::new())),
    };

    // And: Checkpoint store to track progress
    let checkpoint_store = InMemoryCheckpointStore::new();

    // And: Projector that tracks apply calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: First run with failures then success
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, reader)
        .with_poll_mode(PollMode::Batch)
        .with_checkpoint_store(checkpoint_store.clone());

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .expect("Batch mode should complete, not loop forever");

    // Then: First run succeeds after retries
    assert!(result.is_ok());
    assert_eq!(poll_count.load(Ordering::SeqCst), 4); // 3 failures + 1 success
    assert_eq!(apply_count.load(Ordering::SeqCst), 1); // First event applied

    // And: Append second event to same store
    let event2 = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes2 = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(1))
        .expect("register stream")
        .append(event2)
        .expect("append event");
    store
        .append_events(writes2)
        .await
        .expect("second append succeeds");

    // And: Create new reader that fails 3 more times
    // This should NOT accumulate with previous failures - counter should reset
    let poll_count2 = Arc::new(AtomicUsize::new(0));
    let reader2 = FailNTimesReader {
        store: store.clone(),
        failures_remaining: Arc::new(AtomicUsize::new(3)),
        poll_count: poll_count2.clone(),
        poll_times: Arc::new(Mutex::new(Vec::new())),
    };

    // And: Create new projector with same apply counter
    let projector2 = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: Second run with failures then success
    let coordinator2 = LocalCoordinator::new();
    let runner2 = ProjectionRunner::new(projector2, coordinator2, reader2)
        .with_poll_mode(PollMode::Batch)
        .with_checkpoint_store(checkpoint_store.clone());

    let result2 = tokio::time::timeout(std::time::Duration::from_secs(5), runner2.run())
        .await
        .expect("Batch mode should complete, not loop forever");

    // Then: Second run also succeeds (failures didn't accumulate)
    assert!(result2.is_ok());
    assert_eq!(poll_count2.load(Ordering::SeqCst), 4); // 3 failures + 1 success
    assert_eq!(apply_count.load(Ordering::SeqCst), 2); // Second event applied
}

/// Wrapper that counts read calls without injecting failures.
/// Used to verify poll behavior (e.g., Batch mode polls exactly once).
struct PollCountingReader<S> {
    inner: Arc<S>,
    poll_count: Arc<AtomicUsize>,
}

impl<S> PollCountingReader<S> {
    fn new(inner: Arc<S>, poll_count: Arc<AtomicUsize>) -> Self {
        Self { inner, poll_count }
    }
}

impl<S: EventReader + Sync + Send> EventReader for PollCountingReader<S> {
    type Error = S::Error;

    async fn read_all<E: Event>(&self) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.inner.read_all().await
    }

    async fn read_after<E: Event>(
        &self,
        after_position: StreamPosition,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.inner.read_after(after_position).await
    }

    async fn read_events_after<E: Event>(
        &self,
        query: SubscriptionQuery,
        after_position: Option<StreamPosition>,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        self.inner.read_events_after(query, after_position).await
    }
}

#[tokio::test]
async fn batch_mode_exits_after_single_poll() {
    // Given: Store with one event
    let store = Arc::new(InMemoryEventStore::new());
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store.append_events(writes).await.expect("append succeeds");

    // And: Counting reader to track poll count
    let poll_count = Arc::new(AtomicUsize::new(0));
    let reader = PollCountingReader::new(store.clone(), poll_count.clone());

    // And: Projector that tracks apply calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: Runner processes in Batch mode (should exit after one pass)
    let coordinator = LocalCoordinator::new();
    let runner =
        ProjectionRunner::new(projector, coordinator, reader).with_poll_mode(PollMode::Batch);

    // Use timeout to catch infinite loop mutants quickly
    let result = tokio::time::timeout(std::time::Duration::from_millis(200), runner.run()).await;

    // Then: Runner completes within timeout (doesn't loop forever)
    assert!(
        result.is_ok(),
        "runner timed out - Batch mode should exit after one pass, not loop continuously"
    );

    // And: Runner succeeds
    assert!(result.unwrap().is_ok());

    // And: Database was polled EXACTLY once (not continuously looping)
    // This assertion catches the mutant that flips == to != at line 420
    assert_eq!(poll_count.load(Ordering::SeqCst), 1);

    // And: Event was successfully applied
    assert_eq!(apply_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn custom_retry_configuration_is_respected() {
    // Given: Store with one event
    let store = Arc::new(InMemoryEventStore::new());
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store.append_events(writes).await.expect("append succeeds");

    // And: Mock reader that fails 3 times (will exceed custom max_retries of 2)
    let poll_count = Arc::new(AtomicUsize::new(0));
    let poll_times_handle = Arc::new(Mutex::new(Vec::new()));
    let reader = FailNTimesReader {
        store,
        failures_remaining: Arc::new(AtomicUsize::new(3)),
        poll_count: poll_count.clone(),
        poll_times: poll_times_handle.clone(),
    };

    // And: Projector that tracks apply calls
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = ApplyCounterProjector {
        apply_count: apply_count.clone(),
    };

    // When: Runner with custom retry config (max_retries=2, base_delay=5ms)
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, reader)
        .with_poll_mode(PollMode::Batch)
        .with_max_retries(2)
        .with_base_delay_ms(5);

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .expect("Batch mode should complete, not loop forever");

    // Then: Runner returns error after custom max_retries (2)
    assert!(result.is_err(), "Expected error after 2 retries");

    // And: Database was polled exactly 3 times (initial + 2 retries)
    assert_eq!(poll_count.load(Ordering::SeqCst), 3);

    // And: Event was never applied (never got past read errors)
    assert_eq!(apply_count.load(Ordering::SeqCst), 0);

    // And: Verify custom base_delay was used (5ms, 10ms delays)
    let poll_times = poll_times_handle.lock().unwrap();
    assert_eq!(poll_times.len(), 3);

    let delay_1 = poll_times[1].duration_since(poll_times[0]).as_millis();
    let delay_2 = poll_times[2].duration_since(poll_times[1]).as_millis();

    // First delay: 5ms * 2^0 = 5ms (allow 3-12ms for timing variance)
    assert!(
        (3..=12).contains(&delay_1),
        "First delay should be ~5ms, got {}ms",
        delay_1
    );

    // Second delay: 5ms * 2^1 = 10ms (allow 6-18ms for timing variance)
    assert!(
        (6..=18).contains(&delay_2),
        "Second delay should be ~10ms, got {}ms",
        delay_2
    );
}
