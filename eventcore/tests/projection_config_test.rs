//! Integration tests for ADR-0037: ProjectionConfig via free function
//!
//! Scenario: run_projection_with_config with default config processes events
//! - Given a backend with seeded events
//! - When run_projection_with_config(projector, &backend, ProjectionConfig::default()) is called
//! - Then all events are processed (same behavior as run_projection)
//!
//! Scenario: custom_poll_interval_is_used_in_continuous_mode
//! - Given a backend with no events initially
//! - And config with continuous mode and custom poll_interval(50ms)
//! - When run_projection_with_config is spawned, and an event appended after a delay
//! - Then the event is processed (proves polling continued with custom interval)
//!
//! Scenario: event_retry_max_attempts_limits_retries_via_config
//! - Given a projector that always fails and returns FailureStrategy::Retry
//! - And config with event_retry_max_attempts(2)
//! - When events are seeded and run_projection_with_config is called
//! - Then projection fails with ProjectionError after exhausting retries
//!
//! Scenario: max_consecutive_poll_failures_stops_projection_via_config
//! - Given a backend that always fails on read_events
//! - And config with continuous mode and max_consecutive_poll_failures(3)
//! - When run_projection_with_config is called
//! - Then projection stops after consecutive poll failures with ProjectionError

use eventcore::{
    BackoffMultiplier, CheckpointStore, Event, EventFilter, EventPage, EventReader, EventStore,
    FailureContext, FailureStrategy, MaxConsecutiveFailures, MaxRetryAttempts, ProjectionConfig,
    Projector, StreamId, StreamPosition, StreamVersion, StreamWrites, run_projection_with_config,
};
use eventcore_memory::{InMemoryCheckpointStore, InMemoryEventStore, InMemoryProjectorCoordinator};
use eventcore_types::ProjectorCoordinator;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::num::NonZeroU32;
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

#[tokio::test]
async fn continuous_mode_processes_events_added_after_start() {
    // Given: A backend with no events initially
    // Leak the backend so it can be shared across the spawn boundary.
    let backend: &'static TestBackend = Box::leak(Box::new(TestBackend::new()));

    // And: A projector configured with ProjectionConfig::default().continuous()
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());
    let config = ProjectionConfig::default().continuous();

    // When: run_projection_with_config is spawned in a background task
    let task =
        tokio::spawn(async move { run_projection_with_config(projector, backend, config).await });

    // And: An event is appended to the store after a short delay
    tokio::time::sleep(Duration::from_millis(200)).await;
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");
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

    // Then: The projector processes the new event
    tokio::time::sleep(Duration::from_millis(500)).await;
    let count = event_count.load(Ordering::SeqCst);

    // And: The projection can be cancelled
    task.abort();
    let _ = task.await;

    assert_eq!(
        count, 1,
        "projector should have processed the event added after start"
    );
}

// === TDD Cycle 3: Configurable Poll Interval ===

#[tokio::test]
async fn custom_poll_interval_is_used_in_continuous_mode() {
    // Given: A backend with no events initially
    // Leak the backend so it can be shared across the spawn boundary.
    let backend: &'static TestBackend = Box::leak(Box::new(TestBackend::new()));

    // And: A projector configured with a custom poll interval
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());
    let config = ProjectionConfig::default()
        .continuous()
        .poll_interval(Duration::from_millis(50));

    // When: run_projection_with_config is spawned in a background task
    let task =
        tokio::spawn(async move { run_projection_with_config(projector, backend, config).await });

    // And: An event is appended to the store after a short delay
    tokio::time::sleep(Duration::from_millis(200)).await;
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");
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

    // Then: The projector processes the new event (proves polling with custom interval)
    tokio::time::sleep(Duration::from_millis(300)).await;
    let count = event_count.load(Ordering::SeqCst);

    // And: The projection can be cancelled
    task.abort();
    let _ = task.await;

    assert_eq!(
        count, 1,
        "projector should have processed the event added after start using custom poll interval"
    );
}

// === TDD Cycle 4: Configurable Event Retry ===

/// Projector that always fails on apply() and always returns Retry from on_error().
/// This forces the runner's EventRetryConfig to handle retry limit enforcement.
struct AlwaysFailRetryProjector {
    apply_count: Arc<AtomicUsize>,
}

impl AlwaysFailRetryProjector {
    fn new(apply_count: Arc<AtomicUsize>) -> Self {
        Self { apply_count }
    }
}

impl Projector for AlwaysFailRetryProjector {
    type Event = CounterIncremented;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let _ = self.apply_count.fetch_add(1, Ordering::SeqCst);
        Err("always-fail".to_string())
    }

    fn name(&self) -> &str {
        "always-fail-retry"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Retry
    }
}

#[tokio::test]
async fn event_retry_max_attempts_limits_retries_via_config() {
    // Given: A backend with one seeded event
    let backend = TestBackend::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");
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

    // And: A projector that always fails and always returns Retry
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = AlwaysFailRetryProjector::new(apply_count.clone());

    // And: Config with event_retry_max_attempts = 2 and minimal delays
    let config = ProjectionConfig::default()
        .event_retry_max_attempts(MaxRetryAttempts::new(2))
        .event_retry_delay(Duration::from_millis(1))
        .event_retry_backoff_multiplier(
            BackoffMultiplier::try_new(1.0).expect("valid backoff multiplier"),
        )
        .event_retry_max_delay(Duration::from_millis(1));

    // When: run_projection_with_config is called
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_projection_with_config(projector, &backend, config),
    )
    .await
    .expect("should complete within timeout");

    // Then: The projection fails with a ProjectionError after exhausting retries
    assert!(
        result.is_err(),
        "projection should fail after exhausting retries"
    );

    // And: apply() was called exactly 3 times (initial + 2 retries)
    let total_apply_calls = apply_count.load(Ordering::SeqCst);
    assert_eq!(
        total_apply_calls, 3,
        "apply() should be called 3 times: initial attempt + 2 retries"
    );
}

// === TDD Cycle 5: Poll Failure Recovery ===

/// Error type for the always-failing backend.
#[derive(Debug, Clone)]
struct AlwaysFailReadError(String);

impl std::fmt::Display for AlwaysFailReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AlwaysFailReadError {}

/// A backend that always fails on read_events but works for checkpoints and coordination.
struct FailingReadTestBackend {
    checkpoint_store: InMemoryCheckpointStore,
    coordinator: InMemoryProjectorCoordinator,
}

impl FailingReadTestBackend {
    fn new() -> Self {
        Self {
            checkpoint_store: InMemoryCheckpointStore::new(),
            coordinator: InMemoryProjectorCoordinator::new(),
        }
    }
}

impl EventReader for FailingReadTestBackend {
    type Error = AlwaysFailReadError;

    async fn read_events<E: Event>(
        &self,
        _filter: EventFilter,
        _page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        Err(AlwaysFailReadError(
            "simulated database failure".to_string(),
        ))
    }
}

impl CheckpointStore for FailingReadTestBackend {
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

impl ProjectorCoordinator for FailingReadTestBackend {
    type Error = eventcore_memory::InMemoryCoordinationError;
    type Guard = eventcore_memory::InMemoryCoordinationGuard;

    fn try_acquire(
        &self,
        subscription_name: &str,
    ) -> impl Future<Output = Result<Self::Guard, Self::Error>> + Send {
        self.coordinator.try_acquire(subscription_name)
    }
}

#[tokio::test]
async fn max_consecutive_poll_failures_stops_projection_via_config() {
    // Given: A backend that always fails on read_events
    let backend = FailingReadTestBackend::new();

    // And: A normal projector (won't get to process anything since reads always fail)
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = EventCounterProjector::new(event_count.clone());

    // And: Config with continuous mode and max_consecutive_poll_failures = 3
    let config = ProjectionConfig::default()
        .continuous()
        .max_consecutive_poll_failures(MaxConsecutiveFailures::new(
            NonZeroU32::new(3).expect("3 is non-zero"),
        ))
        .poll_failure_backoff(Duration::from_millis(1));

    // When: run_projection_with_config is called
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_projection_with_config(projector, &backend, config),
    )
    .await
    .expect("should complete within timeout");

    // Then: The projection stops after consecutive poll failures with a ProjectionError
    assert!(
        result.is_err(),
        "projection should fail after max consecutive poll failures"
    );

    // And: No events were processed (reads always failed)
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        0,
        "no events should have been processed since all reads failed"
    );
}
