//! Integration test for eventcore-a5a: Database poll retry with exponential backoff
//!
//! Scenario: Developer handles transient database errors during event polling
//! - Given projector is running in continuous poll mode
//! - When database read fails transiently (connection timeout, etc)
//! - Then runner retries with exponential backoff
//! - And after max consecutive failures, propagates error to caller
//! - And caller can decide recovery strategy (restart, alert, etc)

use eventcore::{
    Event, EventReader, EventStore, InMemoryCheckpointStore, InMemoryEventStore, LocalCoordinator,
    PollMode, ProjectionRunner, Projector, StreamId, StreamPosition, StreamVersion, StreamWrites,
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

/// Mock EventReader that fails N times then delegates to wrapped store.
/// Tracks consecutive poll failures and delays to verify backoff behavior.
struct FailNTimesReader<S> {
    inner: S,
    failures_remaining: Arc<AtomicUsize>,
    poll_count: Arc<AtomicUsize>,
    poll_times: Arc<Mutex<Vec<Instant>>>,
}

impl<S> EventReader for FailNTimesReader<S>
where
    S: EventReader + Sync,
{
    type Error = S::Error;

    async fn read_all<E>(&self) -> Result<Vec<(E, StreamPosition)>, Self::Error>
    where
        E: Event,
    {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        self.poll_times.lock().unwrap().push(Instant::now());

        let remaining = self.failures_remaining.load(Ordering::SeqCst);
        if remaining > 0 {
            self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            // Simulate transient database error
            // TODO: This needs to return S::Error, but we don't know how to construct it
            // We need a way to inject failures that matches the error type
            panic!("transient database connection timeout");
        }

        // Delegate to wrapped store after failures exhausted
        self.inner.read_all().await
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
            // Simulate transient database error
            // TODO: This needs to return S::Error, but we don't know how to construct it
            // We need a way to inject failures that matches the error type
            panic!("transient database connection timeout");
        }

        // Delegate to wrapped store after failures exhausted
        self.inner.read_after(position).await
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
    let store = InMemoryEventStore::new();
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
        inner: &store,
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

    let result = runner.run().await;

    // Then: Runner succeeds after retries
    assert!(result.is_ok());

    // And: Database was polled 4 times (3 failures + 1 success)
    assert_eq!(poll_count.load(Ordering::SeqCst), 4);

    // And: Event was successfully applied once
    assert_eq!(apply_count.load(Ordering::SeqCst), 1);

    // And: Polls had exponential backoff between them
    let poll_times = poll_times_handle.lock().unwrap();
    assert_eq!(poll_times.len(), 4);

    // Verify exponential backoff: each delay should roughly double
    // First retry: ~10ms, second: ~20ms, third: ~40ms
    let delay_1 = poll_times[1].duration_since(poll_times[0]).as_millis();
    let delay_2 = poll_times[2].duration_since(poll_times[1]).as_millis();
    let delay_3 = poll_times[3].duration_since(poll_times[2]).as_millis();

    // Allow tolerance for timing variance (±50%)
    assert!((5..=20).contains(&delay_1), "First delay: {}ms", delay_1);
    assert!((10..=40).contains(&delay_2), "Second delay: {}ms", delay_2);
    assert!((20..=80).contains(&delay_3), "Third delay: {}ms", delay_3);
    assert!(
        delay_2 > delay_1,
        "Second delay should be greater than first"
    );
    assert!(
        delay_3 > delay_2,
        "Third delay should be greater than second"
    );
}

#[tokio::test]
async fn runner_propagates_error_after_max_consecutive_poll_failures() {
    // Given: Event store (content doesn't matter - reader always fails)
    let store = InMemoryEventStore::new();

    // And: Mock reader that fails 6 times (exceeds max retries of 5)
    let poll_count = Arc::new(AtomicUsize::new(0));
    let reader = FailNTimesReader {
        inner: &store,
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

    let result = runner.run().await;

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
    let store = InMemoryEventStore::new();
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
        inner: &store,
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

    let result = runner.run().await;

    // Then: First run succeeds after retries
    assert!(result.is_ok());
    assert_eq!(poll_count.load(Ordering::SeqCst), 4); // 3 failures + 1 success
    assert_eq!(apply_count.load(Ordering::SeqCst), 1); // First event applied

    // And: Append second event
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
        inner: &store,
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

    let result2 = runner2.run().await;

    // Then: Second run also succeeds (failures didn't accumulate)
    assert!(result2.is_ok());
    assert_eq!(poll_count2.load(Ordering::SeqCst), 4); // 3 failures + 1 success
    assert_eq!(apply_count.load(Ordering::SeqCst), 2); // Second event applied
}
