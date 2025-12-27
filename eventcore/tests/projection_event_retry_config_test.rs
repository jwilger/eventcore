//! Integration test for eventcore-5tb: Event Retry Configuration
//!
//! Scenario: max_retry_attempts limits retries and escalates to Fatal
//! - Given developer configures EventRetryConfig with max_retry_attempts = 3
//! - And projector's on_error() always returns FailureStrategy::Retry
//! - When projector fails to process an event
//! - Then runner retries exactly 3 times (initial attempt + 3 retries = 4 total apply calls)
//! - And runner escalates to Fatal error after exhausting retries
//! - And checkpoint is NOT advanced past the failed event

use eventcore::{
    BackoffMultiplier, Event, EventRetryConfig, EventStore, FailureContext, FailureStrategy,
    LocalCoordinator, MaxRetryAttempts, PollMode, ProjectionRunner, Projector, StreamId,
    StreamPosition, StreamVersion, StreamWrites,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A simple event type for testing retry configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

/// Projector that always fails on apply() and always returns Retry from on_error().
/// This forces the runner's EventRetryConfig to handle retry limit enforcement.
struct AlwaysFailAlwaysRetryProjector {
    apply_count: Arc<AtomicUsize>,
}

impl AlwaysFailAlwaysRetryProjector {
    fn new(apply_count: Arc<AtomicUsize>) -> Self {
        Self { apply_count }
    }
}

impl Projector for AlwaysFailAlwaysRetryProjector {
    type Event = TestEvent;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.apply_count.fetch_add(1, Ordering::SeqCst);
        Err("transient error".to_string())
    }

    fn name(&self) -> &str {
        "always-fail-always-retry"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        // Always return Retry - let EventRetryConfig enforce the limit
        FailureStrategy::Retry
    }
}

#[tokio::test]
async fn event_retry_config_limits_retries_and_escalates_to_fatal() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: Projector that always fails and always returns Retry
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = AlwaysFailAlwaysRetryProjector::new(apply_count.clone());

    // Given: EventRetryConfig with max_retry_attempts = 3
    let retry_config = EventRetryConfig {
        max_retry_attempts: MaxRetryAttempts::new(3),
        retry_delay: Duration::from_millis(1), // Keep test fast
        retry_backoff_multiplier: BackoffMultiplier::try_new(1.0).expect("valid value"), // No backoff for this test
        max_retry_delay: Duration::from_millis(1),
    };

    // When: Run the projector with EventRetryConfig
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config);

    let result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: Runner returns error (escalated to Fatal after max retries)
    assert!(
        result.unwrap().is_err(),
        "runner should return error after exhausting retries"
    );

    // Then: apply() was called exactly 4 times (initial + 3 retries)
    let total_apply_calls = apply_count.load(Ordering::SeqCst);
    assert_eq!(
        total_apply_calls, 4,
        "apply() should be called 4 times: initial attempt + 3 retries"
    );
}

/// Projector that fails twice then succeeds.
/// Used to test default retry configuration behavior.
struct FailTwiceThenSucceedProjector {
    apply_count: Arc<AtomicUsize>,
}

impl FailTwiceThenSucceedProjector {
    fn new(apply_count: Arc<AtomicUsize>) -> Self {
        Self { apply_count }
    }
}

impl Projector for FailTwiceThenSucceedProjector {
    type Event = TestEvent;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let count = self.apply_count.fetch_add(1, Ordering::SeqCst);
        // Fail on first two attempts (count 0, 1), succeed on third (count 2)
        if count < 2 {
            Err("transient error".to_string())
        } else {
            Ok(())
        }
    }

    fn name(&self) -> &str {
        "fail-twice-then-succeed"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        // Always return Retry - projector wants to retry transient errors
        FailureStrategy::Retry
    }
}

#[tokio::test]
async fn default_retry_config_allows_retries_without_explicit_configuration() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: Projector that fails twice then succeeds
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = FailTwiceThenSucceedProjector::new(apply_count.clone());

    // When: Run the projector WITHOUT calling with_event_retry_config()
    // This should use EventRetryConfig::default() internally
    let coordinator = LocalCoordinator::new();
    let runner =
        ProjectionRunner::new(projector, coordinator, &store).with_poll_mode(PollMode::Batch);

    let result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: Runner succeeds (projection completes after 2 retries)
    assert!(
        result.unwrap().is_ok(),
        "runner should succeed using default retry config"
    );
}

/// Projector that fails once then succeeds.
/// Used to test that retry_delay is respected.
struct FailOnceThenSucceedProjector {
    apply_count: Arc<AtomicUsize>,
}

impl FailOnceThenSucceedProjector {
    fn new(apply_count: Arc<AtomicUsize>) -> Self {
        Self { apply_count }
    }
}

impl Projector for FailOnceThenSucceedProjector {
    type Event = TestEvent;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let count = self.apply_count.fetch_add(1, Ordering::SeqCst);
        // Fail on first attempt (count 0), succeed on second (count 1)
        if count == 0 {
            Err("transient error".to_string())
        } else {
            Ok(())
        }
    }

    fn name(&self) -> &str {
        "fail-once-then-succeed"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Retry
    }
}

#[tokio::test]
async fn retry_delay_is_respected_during_retry() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: Projector that fails once then succeeds
    let apply_count = Arc::new(AtomicUsize::new(0));
    let projector = FailOnceThenSucceedProjector::new(apply_count.clone());

    // Given: EventRetryConfig with retry_delay = 50ms
    let retry_delay = Duration::from_millis(50);
    let retry_config = EventRetryConfig {
        max_retry_attempts: MaxRetryAttempts::new(3),
        retry_delay,
        retry_backoff_multiplier: BackoffMultiplier::try_new(1.0).expect("valid value"), // No backoff for this test
        max_retry_delay: Duration::from_millis(50),
    };

    // When: Run the projector and measure elapsed time
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config);

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;
    let elapsed = start.elapsed();

    // Then: Runner succeeds after 1 retry
    assert!(
        result.unwrap().is_ok(),
        "runner should succeed after 1 retry"
    );

    // Then: Total elapsed time should be >= retry_delay
    // Allow some tolerance for test timing variability (±20ms)
    assert!(
        elapsed >= retry_delay - Duration::from_millis(20),
        "elapsed time ({:?}) should be >= retry_delay ({:?}) - 20ms tolerance",
        elapsed,
        retry_delay
    );
}

/// Projector that fails three times then succeeds.
/// Used to test exponential backoff configuration.
struct FailThreeTimesThenSucceedProjector {
    apply_count: Arc<AtomicUsize>,
    apply_times: Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
}

impl FailThreeTimesThenSucceedProjector {
    fn new(
        apply_count: Arc<AtomicUsize>,
        apply_times: Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    ) -> Self {
        Self {
            apply_count,
            apply_times,
        }
    }
}

impl Projector for FailThreeTimesThenSucceedProjector {
    type Event = TestEvent;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        // Record when this apply was called
        self.apply_times
            .lock()
            .unwrap()
            .push(std::time::Instant::now());

        let count = self.apply_count.fetch_add(1, Ordering::SeqCst);
        // Fail on first three attempts (count 0, 1, 2), succeed on fourth (count 3)
        if count < 3 {
            Err("transient error".to_string())
        } else {
            Ok(())
        }
    }

    fn name(&self) -> &str {
        "fail-three-times-then-succeed"
    }

    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Retry
    }
}

#[tokio::test]
async fn exponential_backoff_is_applied_during_retries() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: Projector that fails three times then succeeds
    let apply_count = Arc::new(AtomicUsize::new(0));
    let apply_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let projector =
        FailThreeTimesThenSucceedProjector::new(apply_count.clone(), apply_times.clone());

    // Given: EventRetryConfig with retry_delay=50ms, backoff_multiplier=2.0
    let retry_config = EventRetryConfig {
        max_retry_attempts: MaxRetryAttempts::new(5),
        retry_delay: Duration::from_millis(50),
        retry_backoff_multiplier: BackoffMultiplier::try_new(2.0).expect("valid value"),
        max_retry_delay: Duration::from_secs(10), // High enough not to cap our test
    };

    // When: Run the projector and measure timing
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config);

    let result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: Runner succeeds after 3 retries
    assert!(
        result.unwrap().is_ok(),
        "runner should succeed after 3 retries"
    );

    // Then: Verify exponential backoff timing
    let times = apply_times.lock().unwrap();
    assert_eq!(times.len(), 4, "should have 4 apply calls total");

    // Calculate delays between attempts
    // Formula: delay = retry_delay * (backoff_multiplier ^ (retry_count - 1))
    // First retry (retry_count=1): 50ms * (2.0 ^ 0) = 50ms
    // Second retry (retry_count=2): 50ms * (2.0 ^ 1) = 100ms
    // Third retry (retry_count=3): 50ms * (2.0 ^ 2) = 200ms

    let delay_1 = times[1].duration_since(times[0]);
    let delay_2 = times[2].duration_since(times[1]);
    let delay_3 = times[3].duration_since(times[2]);

    // Allow ±30ms tolerance for test timing variability
    let tolerance = Duration::from_millis(30);
    let expected_delay_1 = Duration::from_millis(50);
    let expected_delay_2 = Duration::from_millis(100);
    let expected_delay_3 = Duration::from_millis(200);

    assert!(
        delay_1 >= expected_delay_1 - tolerance && delay_1 <= expected_delay_1 + tolerance,
        "first retry delay ({:?}) should be ~50ms (±30ms)",
        delay_1
    );
    assert!(
        delay_2 >= expected_delay_2 - tolerance && delay_2 <= expected_delay_2 + tolerance,
        "second retry delay ({:?}) should be ~100ms (±30ms)",
        delay_2
    );
    assert!(
        delay_3 >= expected_delay_3 - tolerance && delay_3 <= expected_delay_3 + tolerance,
        "third retry delay ({:?}) should be ~200ms (±30ms)",
        delay_3
    );
}

#[tokio::test]
async fn max_retry_delay_caps_exponential_backoff() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event)
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: Projector that fails three times then succeeds
    let apply_count = Arc::new(AtomicUsize::new(0));
    let apply_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let projector =
        FailThreeTimesThenSucceedProjector::new(apply_count.clone(), apply_times.clone());

    // Given: EventRetryConfig with exponential backoff that would exceed max_retry_delay
    // retry_delay=100ms, multiplier=2.0, max_retry_delay=150ms (low cap)
    // Without cap: 100ms, 200ms, 400ms
    // With cap: 100ms, 150ms (capped from 200ms), 150ms (capped from 400ms)
    let retry_config = EventRetryConfig {
        max_retry_attempts: MaxRetryAttempts::new(5),
        retry_delay: Duration::from_millis(100),
        retry_backoff_multiplier: BackoffMultiplier::try_new(2.0).expect("valid value"),
        max_retry_delay: Duration::from_millis(150), // Cap to prevent unbounded growth
    };

    // When: Run the projector and measure timing
    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config);

    let result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: Runner succeeds after 3 retries
    assert!(
        result.unwrap().is_ok(),
        "runner should succeed after 3 retries"
    );

    // Then: Verify delays are capped at max_retry_delay
    let times = apply_times.lock().unwrap();
    assert_eq!(times.len(), 4, "should have 4 apply calls total");

    // Calculate delays between attempts
    // First retry (retry_count=1): 100ms * (2.0 ^ 0) = 100ms (uncapped)
    // Second retry (retry_count=2): 100ms * (2.0 ^ 1) = 200ms → capped to 150ms
    // Third retry (retry_count=3): 100ms * (2.0 ^ 2) = 400ms → capped to 150ms

    let delay_1 = times[1].duration_since(times[0]);
    let delay_2 = times[2].duration_since(times[1]);
    let delay_3 = times[3].duration_since(times[2]);

    // Allow ±30ms tolerance for test timing variability
    let tolerance = Duration::from_millis(30);
    let expected_delay_1 = Duration::from_millis(100);
    let expected_delay_2 = Duration::from_millis(150); // Capped
    let expected_delay_3 = Duration::from_millis(150); // Capped

    assert!(
        delay_1 >= expected_delay_1 - tolerance && delay_1 <= expected_delay_1 + tolerance,
        "first retry delay ({:?}) should be ~100ms (±30ms) - uncapped",
        delay_1
    );
    assert!(
        delay_2 >= expected_delay_2 - tolerance && delay_2 <= expected_delay_2 + tolerance,
        "second retry delay ({:?}) should be ~150ms (±30ms) - capped from 200ms",
        delay_2
    );
    assert!(
        delay_3 >= expected_delay_3 - tolerance && delay_3 <= expected_delay_3 + tolerance,
        "third retry delay ({:?}) should be ~150ms (±30ms) - capped from 400ms, backoff does not grow unbounded",
        delay_3
    );
}

/// Projector that can fail with either transient or permanent errors.
/// Uses on_error() to decide retry strategy based on error type.
struct ErrorTypeAwareProjector {
    apply_count: Arc<AtomicUsize>,
    error_type: ErrorType,
}

#[derive(Debug, Clone, Copy)]
enum ErrorType {
    Transient,
    Permanent,
}

impl ErrorTypeAwareProjector {
    fn new(apply_count: Arc<AtomicUsize>, error_type: ErrorType) -> Self {
        Self {
            apply_count,
            error_type,
        }
    }
}

impl Projector for ErrorTypeAwareProjector {
    type Event = TestEvent;
    type Error = String;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        self.apply_count.fetch_add(1, Ordering::SeqCst);
        match self.error_type {
            ErrorType::Transient => Err("network timeout".to_string()),
            ErrorType::Permanent => Err("schema violation".to_string()),
        }
    }

    fn name(&self) -> &str {
        "error-type-aware"
    }

    fn on_error(&mut self, ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        // Application logic: decide retry eligibility based on error type
        if ctx.error.contains("timeout") {
            FailureStrategy::Retry
        } else {
            FailureStrategy::Fatal
        }
    }
}

#[tokio::test]
async fn on_error_decides_retry_eligibility_config_only_applies_when_retry() {
    // Given: Event store with one event
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(event.clone())
        .expect("append event");
    store
        .append_events(writes)
        .await
        .expect("append to succeed");

    // Given: EventRetryConfig with max_retry_attempts = 3
    let retry_config = EventRetryConfig {
        max_retry_attempts: MaxRetryAttempts::new(3),
        retry_delay: Duration::from_millis(1),
        retry_backoff_multiplier: BackoffMultiplier::try_new(1.0).expect("valid value"),
        max_retry_delay: Duration::from_millis(1),
    };

    // When: Projector fails with transient error (on_error returns Retry)
    let transient_count = Arc::new(AtomicUsize::new(0));
    let transient_projector =
        ErrorTypeAwareProjector::new(transient_count.clone(), ErrorType::Transient);

    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(transient_projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config.clone());

    let transient_result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: EventRetryConfig is used - retries happen
    assert!(
        transient_result.unwrap().is_err(),
        "transient error should exhaust retries and return error"
    );
    assert_eq!(
        transient_count.load(Ordering::SeqCst),
        4,
        "transient error should trigger retries: initial + 3 retries = 4 total"
    );

    // When: Projector fails with permanent error (on_error returns Fatal)
    let permanent_count = Arc::new(AtomicUsize::new(0));
    let permanent_projector =
        ErrorTypeAwareProjector::new(permanent_count.clone(), ErrorType::Permanent);

    let coordinator = LocalCoordinator::new();
    let runner = ProjectionRunner::new(permanent_projector, coordinator, &store)
        .with_poll_mode(PollMode::Batch)
        .with_event_retry_config(retry_config);

    let permanent_result = tokio::time::timeout(Duration::from_secs(5), runner.run()).await;

    // Then: EventRetryConfig is NOT used - immediate failure, no retries
    assert!(
        permanent_result.unwrap().is_err(),
        "permanent error should return error immediately"
    );
    assert_eq!(
        permanent_count.load(Ordering::SeqCst),
        1,
        "permanent error should NOT trigger retries: only initial attempt"
    );
}
