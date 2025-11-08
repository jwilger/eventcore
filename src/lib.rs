mod command;
mod errors;
mod store;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

// Re-export only the minimal public API needed for execute() signature
pub use command::{CommandLogic, CommandStreams, CommandStreamsError, Event, NewEvents};
pub use errors::CommandError;
pub use store::EventStore;

// Re-export InMemoryEventStore for library consumers (per ADR-011)
// Re-export EventStore trait helper types for trait implementations (per ADR-010 compiler-driven evolution)
pub use store::{
    EventStoreError, EventStreamReader, EventStreamSlice, InMemoryEventStore, StreamId,
    StreamVersion, StreamWrites,
};

/// Represents the successful outcome of command execution.
///
/// This type is returned when a command completes successfully, including
/// state reconstruction, business rule validation, and atomic event persistence.
/// The specific data included in this response is yet to be determined based
/// on actual usage requirements.
#[derive(Debug)]
pub struct ExecutionResponse {
    attempts: NonZeroU32,
}

impl ExecutionResponse {
    pub fn new(attempts: NonZeroU32) -> Self {
        Self { attempts }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts.get()
    }
}

/// Defines the delay strategy between retry attempts.
///
/// Different backoff strategies are useful for different scenarios:
/// - Fixed: Predictable timing for rate-limited APIs
/// - Exponential: Reduces load during high-traffic periods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffStrategy {
    /// Fixed delay between all retry attempts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::BackoffStrategy;
    /// let strategy = BackoffStrategy::Fixed { delay_ms: 50 };
    /// ```
    Fixed {
        /// Delay in milliseconds between each retry attempt
        delay_ms: u64,
    },

    /// Exponential backoff with base delay multiplied by 2^attempt.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::BackoffStrategy;
    /// let strategy = BackoffStrategy::Exponential { base_ms: 10 };
    /// ```
    Exponential {
        /// Base delay in milliseconds (multiplied by 2^attempt)
        base_ms: u64,
    },
}

/// Callback trait for integrating with metrics systems during retry lifecycle.
///
/// Library consumers implement this trait to receive notifications about retry
/// attempts, enabling integration with metrics systems like Prometheus.
pub trait MetricsHook: Send + Sync {
    /// Called when a retry attempt is about to be made.
    ///
    /// # Parameters
    ///
    /// * `ctx` - Context about the retry attempt (streams, attempt number, delay)
    fn on_retry_attempt(&self, ctx: &RetryContext);
}

/// Context information passed to metrics hooks during retry lifecycle.
///
/// Provides structured data about the retry attempt for metrics collection.
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// The set of streams being retried (guaranteed non-empty)
    pub streams: Vec<StreamId>,
    /// The current retry attempt number (1-based)
    pub attempt: u32,
    /// The delay in milliseconds before this retry attempt
    pub delay_ms: u64,
}

/// Configuration for automatic retry behavior on concurrency conflicts.
///
/// RetryPolicy allows library consumers to customize how execute() handles
/// version conflicts during command execution. Uses method chaining for
/// ergonomic configuration.
///
/// # Examples
///
/// ```rust
/// # use eventcore::{RetryPolicy, BackoffStrategy};
/// // Custom retry policy with 2 retries (3 total attempts) instead of default 4 retries
/// let policy = RetryPolicy::new().max_retries(2);
///
/// // Custom retry policy with fixed backoff
/// let policy = RetryPolicy::new()
///     .max_retries(2)
///     .backoff_strategy(BackoffStrategy::Fixed { delay_ms: 50 });
/// ```
#[derive(Clone)]
pub struct RetryPolicy {
    max_retries: u32,
    backoff_strategy: BackoffStrategy,
    metrics_hook: Option<Arc<dyn MetricsHook>>,
}

impl RetryPolicy {
    /// Create a new RetryPolicy with default values.
    ///
    /// Default configuration matches I-002 hardcoded values:
    /// - max_retries: 4 (5 total attempts including initial)
    /// - backoff_strategy: Exponential with 10ms base
    /// - jitter: ±20% (applied during execution)
    pub fn new() -> Self {
        Self {
            max_retries: 4,
            backoff_strategy: BackoffStrategy::Exponential { base_ms: 10 },
            metrics_hook: None,
        }
    }

    /// Configure the maximum number of retry attempts.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::RetryPolicy;
    /// let policy = RetryPolicy::new().max_retries(2);
    /// ```
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Configure the backoff strategy for retry delays.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::{RetryPolicy, BackoffStrategy};
    /// let policy = RetryPolicy::new()
    ///     .backoff_strategy(BackoffStrategy::Fixed { delay_ms: 50 });
    /// ```
    pub fn backoff_strategy(mut self, strategy: BackoffStrategy) -> Self {
        self.backoff_strategy = strategy;
        self
    }

    /// Configure a metrics hook for retry lifecycle events.
    ///
    /// The hook will receive callbacks on each retry attempt with structured context data
    /// for metrics collection systems.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// # use eventcore::{RetryPolicy, MetricsHook};
    /// struct MyMetricsHook;
    /// impl MetricsHook for MyMetricsHook {
    ///     fn on_retry_attempt(&self, ctx: &RetryContext) {
    ///         // Record metrics
    ///     }
    /// }
    ///
    /// let policy = RetryPolicy::new()
    ///     .with_metrics_hook(MyMetricsHook);
    /// ```
    pub fn with_metrics_hook<H: MetricsHook + 'static>(mut self, hook: H) -> Self {
        self.metrics_hook = Some(Arc::new(hook));
        self
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate jitter factor from a random value in [0.0, 1.0].
///
/// Converts a uniformly distributed random value into a jitter factor
/// that provides ±20% variation around 1.0.
///
/// # Formula
///
/// `jitter_factor = 1.0 + (random_value - 0.5) * 0.4`
///
/// This produces a range of [0.8, 1.2]:
/// - When random_value = 0.0: jitter = 1.0 + (-0.5 * 0.4) = 0.8
/// - When random_value = 0.5: jitter = 1.0 + (0.0 * 0.4) = 1.0
/// - When random_value = 1.0: jitter = 1.0 + (0.5 * 0.4) = 1.2
///
/// # Arguments
///
/// * `random_value` - A uniformly distributed random value in [0.0, 1.0]
///
/// # Returns
///
/// A jitter factor in the range [0.8, 1.2]
fn calculate_jitter_factor(random_value: f64) -> f64 {
    1.0 + (random_value - 0.5) * 0.4
}

/// Apply jitter factor to a base delay value.
///
/// Multiplies the base delay by the jitter factor and converts to microseconds.
///
/// # Arguments
///
/// * `base_delay` - Base delay in milliseconds
/// * `jitter_factor` - Multiplicative factor to apply (typically in range [0.8, 1.2])
///
/// # Returns
///
/// Jittered delay in milliseconds as u64
fn apply_jitter(base_delay: u64, jitter_factor: f64) -> u64 {
    (base_delay as f64 * jitter_factor) as u64
}

/// Execute a command against the event store with a custom retry policy.
///
/// This is the primary entry point for EventCore. It orchestrates the complete
/// command execution workflow: loading state from multiple streams, validating
/// business rules, and atomically committing resulting events.
///
/// # Type Parameters
///
/// * `C` - A command implementing [`CommandLogic`] that defines the business operation
/// * `S` - An event store implementing [`EventStore`] for persistence
///
/// # Parameters
///
/// * `store` - The event store for reading/writing events
/// * `command` - The command to execute
/// * `policy` - Retry policy configuration (max attempts, backoff strategy, etc.)
///
/// # Errors
///
/// Returns [`CommandError`] if:
/// - Stream resolution fails
/// - Event loading fails
/// - Business rule validation fails (via command's `handle()`)
/// - Event persistence fails
/// - Optimistic concurrency conflicts occur after exhausting retries
#[tracing::instrument(name = "execute", skip_all, fields())]
pub async fn execute<C, S>(
    store: S,
    command: C,
    policy: RetryPolicy,
) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    for attempt in 0..=policy.max_retries {
        let stream_ids: Vec<StreamId> = {
            let declared_streams = command.streams();
            declared_streams.iter().cloned().collect()
        };
        let mut expected_versions: HashMap<StreamId, StreamVersion> =
            HashMap::with_capacity(stream_ids.len());
        let mut state = C::State::default();

        for stream_id in stream_ids.iter() {
            let reader = store
                .read_stream::<C::Event>(stream_id.clone())
                .await
                .map_err(CommandError::EventStoreError)?;
            let expected_version = StreamVersion::new(reader.len());
            expected_versions.insert(stream_id.clone(), expected_version);
            state = reader
                .into_iter()
                .fold(state, |acc, event| command.apply(acc, &event));
        }

        let new_events = command.handle(state)?;

        // Convert NewEvents to StreamWrites with per-stream version information and store atomically
        let writes: StreamWrites =
            Vec::from(new_events)
                .into_iter()
                .fold(StreamWrites::new(), |writes, event| {
                    let stream_id = event.stream_id();
                    let expected_version = *expected_versions.get(stream_id).unwrap_or_else(|| {
                        panic!(
                            "command emitted event for stream not declared via CommandLogic::streams(): {}",
                            stream_id.as_ref()
                        )
                    });
                    writes.append(event, expected_version)
                });

        // Convert EventStoreError variants to appropriate CommandError types.
        //
        // thiserror's #[from] only implements the From trait, which has signature
        // `fn from(e: T) -> Self` - it cannot pattern match on enum variants.
        // Every EventStoreError would become CommandError::EventStoreError(e).
        //
        // We need variant-specific routing:
        //   - VersionConflict → ConcurrencyError (different CommandError variant!)
        //   - Other errors → EventStoreError(e)
        //
        // Manual map_err with match is the idiomatic solution for this.
        let result = store.append_events(writes).await.map_err(|e| match e {
            EventStoreError::VersionConflict => CommandError::ConcurrencyError(attempt),
        });

        match result {
            Ok(_) => {
                tracing::info!("command execution succeeded");
                return Ok(ExecutionResponse::new(
                    NonZeroU32::new(attempt + 1).expect("attempts are 1-based"),
                ));
            }
            Err(CommandError::ConcurrencyError(_)) if attempt < policy.max_retries => {
                // Calculate backoff delay based on strategy
                let delay_ms = match &policy.backoff_strategy {
                    BackoffStrategy::Fixed { delay_ms } => *delay_ms,
                    BackoffStrategy::Exponential { base_ms } => {
                        let base_delay = 2_u64
                            .checked_pow(attempt)
                            .and_then(|exp| base_ms.checked_mul(exp))
                            .unwrap_or(u64::MAX);
                        let random_value = rand::random::<f64>();
                        let jitter_factor = calculate_jitter_factor(random_value);
                        apply_jitter(base_delay, jitter_factor)
                    }
                };

                tracing::warn!(
                    attempt = attempt + 1,
                    delay_ms = delay_ms,
                    streams = ?stream_ids.as_slice(),
                    "retrying command after concurrency conflict"
                );

                // Call metrics hook if configured
                if let Some(hook) = &policy.metrics_hook {
                    let ctx = RetryContext {
                        streams: stream_ids.clone(),
                        attempt: attempt + 1,
                        delay_ms,
                    };
                    hook.on_retry_attempt(&ctx);
                }

                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue; // Retry
            }
            Err(CommandError::ConcurrencyError(_)) => {
                tracing::error!(
                    max_retries = policy.max_retries,
                    streams = ?stream_ids.as_slice()
                );
                return Err(CommandError::ConcurrencyError(policy.max_retries));
            }
            Err(e) => return Err(e), // Other permanent errors
        }
    }

    unreachable!("loop always returns before max_retries")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use std::sync::atomic::{AtomicBool, Ordering};

    /// Test-specific event type for unit testing.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestEvent {
        stream_id: StreamId,
    }

    impl Event for TestEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    /// Mock command that tracks whether handle() was called.
    ///
    /// This command uses an Arc<AtomicBool> to verify that execute()
    /// actually invokes the command's handle() method.
    struct MockCommand {
        stream_id: StreamId,
        handle_called: Arc<AtomicBool>,
    }

    impl CommandLogic for MockCommand {
        type Event = TestEvent;
        type State = ();

        fn streams(&self) -> CommandStreams {
            CommandStreams::single(self.stream_id.clone())
        }

        fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
            state
        }

        fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
            self.handle_called.store(true, Ordering::SeqCst);
            Ok(NewEvents::default())
        }
    }

    /// Unit test: Verify execute() calls command.handle()
    ///
    /// This test ensures that the execute() function actually invokes
    /// the command's handle() method as part of the command execution workflow.
    /// This is a fundamental requirement: commands must have their business
    /// logic (handle method) executed.
    #[tokio::test]
    async fn test_execute_calls_command_handle() {
        // Given: An in-memory event store
        let store = InMemoryEventStore::new();

        // And: A mock command that tracks handle() calls
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let handle_called = Arc::new(AtomicBool::new(false));
        let command = MockCommand {
            stream_id,
            handle_called: Arc::clone(&handle_called),
        };

        // When: Developer executes the command
        let result = execute(&store, command, RetryPolicy::new()).await;

        // Then: Command execution succeeds
        assert!(result.is_ok(), "execute() should succeed");

        // And: The command's handle() method was called
        assert!(
            handle_called.load(Ordering::SeqCst),
            "execute() must call command.handle()"
        );
    }

    /// Test event type with a value field for state reconstruction testing.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestEventWithValue {
        stream_id: StreamId,
        value: i32,
    }

    impl Event for TestEventWithValue {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    /// Test state that accumulates values from events.
    #[derive(Default, Clone, Debug, PartialEq)]
    struct TestState {
        value: i32,
    }

    /// Mock command that captures the state passed to handle() for inspection.
    struct StateCapturingCommand {
        stream_id: StreamId,
        captured_state: Arc<std::sync::Mutex<Option<TestState>>>,
    }

    impl CommandLogic for StateCapturingCommand {
        type Event = TestEventWithValue;
        type State = TestState;

        fn streams(&self) -> CommandStreams {
            CommandStreams::single(self.stream_id.clone())
        }

        fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
            state.value += event.value;
            state
        }

        fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
            // Capture the state that was passed to handle()
            *self.captured_state.lock().unwrap() = Some(state);
            Ok(NewEvents::default())
        }
    }

    /// Unit test: Verify read_stream() failures propagate as EventStoreError.
    ///
    /// This test ensures that when the event store's read_stream() operation
    /// fails (e.g., network error, database unavailable), the error is correctly
    /// classified as CommandError::EventStoreError rather than being incorrectly
    /// mapped to CommandError::BusinessRuleViolation.
    ///
    /// Storage failures are infrastructure concerns, not domain rule violations.
    #[tokio::test]
    async fn test_read_stream_failure_propagates_as_event_store_error() {
        // Given: A mock event store that fails on read_stream()
        struct FailingEventStore;

        impl EventStore for FailingEventStore {
            async fn read_stream<E: crate::Event>(
                &self,
                _stream_id: StreamId,
            ) -> Result<EventStreamReader<E>, EventStoreError> {
                Err(EventStoreError::VersionConflict)
            }

            async fn append_events(
                &self,
                _writes: StreamWrites,
            ) -> Result<EventStreamSlice, EventStoreError> {
                unimplemented!("Not needed for this test")
            }
        }

        let store = FailingEventStore;

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes the command with a failing store
        let result = execute(&store, command, RetryPolicy::new()).await;

        // Then: Execution fails with EventStoreError, not BusinessRuleViolation
        assert!(
            matches!(result, Err(CommandError::EventStoreError(_))),
            "read_stream() failure should propagate as CommandError::EventStoreError, got: {:?}",
            result
        );
    }

    /// Unit test: Verify execute() reconstructs state from existing events.
    ///
    /// This test ensures that execute() reads existing events from the stream,
    /// applies them via command.apply() to build the current state, and passes
    /// that reconstructed state to command.handle().
    ///
    /// This is critical for commands that make decisions based on prior state
    /// (e.g., Withdraw checking balance from previous Deposit events).
    #[tokio::test]
    async fn test_execute_reconstructs_state_from_existing_events() {
        // Given: An event store with a pre-existing event in a stream
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("account-123").expect("valid stream id");

        // And: Seed the stream with an initial event (value = 50)
        let seed_event = TestEventWithValue {
            stream_id: stream_id.clone(),
            value: 50,
        };
        let writes = StreamWrites::new().append(seed_event, StreamVersion::new(0));
        store
            .append_events(writes)
            .await
            .expect("seed event to be stored");

        // And: A command that captures what state was passed to handle()
        let captured_state = Arc::new(std::sync::Mutex::new(None));
        let command = StateCapturingCommand {
            stream_id: stream_id.clone(),
            captured_state: captured_state.clone(),
        };

        // When: Developer executes the command
        execute(&store, command, RetryPolicy::new())
            .await
            .expect("command execution to succeed");

        // Then: handle() received reconstructed state (not default state)
        let final_state = captured_state.lock().unwrap().clone().unwrap();
        assert_eq!(
            final_state.value, 50,
            "execute() must reconstruct state from existing events before calling handle()"
        );
    }

    /// Integration test: Verify execute() automatically retries on version conflict.
    ///
    /// This test ensures that when a command encounters a version conflict
    /// (ConcurrencyError), execute() automatically retries the command and
    /// succeeds transparently. The developer should never see the ConcurrencyError
    /// for transient conflicts that can be resolved by retry.
    ///
    /// This is critical for multi-user scenarios where concurrent commands may
    /// conflict temporarily but can succeed on retry with updated state.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_execute_retries_automatically_on_version_conflict() {
        // Given: An event store that injects exactly one version conflict
        use tokio::sync::Mutex;

        struct ConflictOnceStore {
            inner: InMemoryEventStore,
            conflict_injected: Arc<Mutex<bool>>,
        }

        impl EventStore for ConflictOnceStore {
            async fn read_stream<E: crate::Event>(
                &self,
                stream_id: StreamId,
            ) -> Result<EventStreamReader<E>, EventStoreError> {
                self.inner.read_stream(stream_id).await
            }

            async fn append_events(
                &self,
                writes: StreamWrites,
            ) -> Result<EventStreamSlice, EventStoreError> {
                let mut injected = self.conflict_injected.lock().await;
                if !*injected {
                    // First call: inject conflict
                    *injected = true;
                    Err(EventStoreError::VersionConflict)
                } else {
                    // Subsequent calls: succeed normally
                    self.inner.append_events(writes).await
                }
            }
        }

        let store = ConflictOnceStore {
            inner: InMemoryEventStore::new(),
            conflict_injected: Arc::new(Mutex::new(false)),
        };

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes the command
        let result = execute(&store, command, RetryPolicy::new()).await;

        // Then: Command succeeds automatically (retry transparent to developer)
        assert!(
            result.is_ok(),
            "execute() should retry automatically and succeed, but got: {:?}",
            result
        );

        // And: Retry attempt should be logged with structured fields for observability
        assert!(
            logs_contain("attempt="),
            "logs should contain structured attempt field"
        );
        assert!(
            logs_contain("delay_ms="),
            "logs should contain structured delay_ms field"
        );
        assert!(
            logs_contain("streams="),
            "logs should contain structured streams field"
        );
    }

    /// Integration test: Verify execute() returns error after exhausting retries.
    ///
    /// This test ensures that when a command encounters persistent version conflicts
    /// (more conflicts than max retry attempts), execute() exhausts all retries and
    /// returns a ConcurrencyError to the developer. This is the failure case where
    /// automatic retry cannot resolve the conflict.
    ///
    /// The developer should receive a clear ConcurrencyError indicating that retries
    /// were attempted but all failed.
    #[tokio::test]
    async fn test_execute_returns_error_after_exhausting_retries() {
        // Given: An event store that ALWAYS fails with version conflicts
        let store = AlwaysConflictStore::new();

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes the command
        let result = execute(&store, command, RetryPolicy::new()).await;

        // Then: ConcurrencyError is returned (retries exhausted)
        assert!(
            matches!(result, Err(CommandError::ConcurrencyError(_))),
            "should return ConcurrencyError after exhausting retries, but got: {:?}",
            result
        );

        // And: Error message contains retry context
        let error = result.unwrap_err();
        if let CommandError::ConcurrencyError(_) = &error {
            let error_msg = error.to_string();
            assert_eq!(
                error_msg, "concurrency conflict after 4 retry attempts",
                "error message should clearly explain that retries were exhausted"
            );
        }
    }

    /// Integration test: Verify execute() respects custom retry policy max_retries.
    ///
    /// This test ensures that library consumers can configure the maximum number
    /// of retry attempts via a RetryPolicy parameter to execute(). The test verifies
    /// that when a developer specifies max_retries=1 (not the default 4), execute()
    /// respects this configuration and fails after exactly 1 retry (2 total attempts).
    ///
    /// This is the simplest test for I-003 (Configurable Retry Policies) - testing
    /// the most basic configuration parameter from the library consumer's perspective.
    #[tokio::test]
    async fn test_execute_with_custom_retry_policy() {
        // Given: An event store that ALWAYS conflicts (reuse from I-002 test)
        let store = AlwaysConflictStore::new();

        // And: A retry policy with max 1 retry (2 total attempts, not default 4 retries)
        let policy = RetryPolicy::new().max_retries(1);

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes with custom policy
        let result = execute(&store, command, policy).await;

        // Then: Fails after exactly 1 retry (2 total attempts)
        assert!(
            matches!(result, Err(CommandError::ConcurrencyError(1))),
            "should fail after exactly 1 retry as configured in policy, but got: {:?}",
            result
        );
    }

    /// Test helper: Event store that ALWAYS returns version conflicts.
    ///
    /// This store simulates persistent conflicts where retry will never succeed.
    /// Useful for testing retry exhaustion and error handling.
    struct AlwaysConflictStore {
        inner: InMemoryEventStore,
    }

    impl AlwaysConflictStore {
        fn new() -> Self {
            Self {
                inner: InMemoryEventStore::new(),
            }
        }
    }

    impl EventStore for AlwaysConflictStore {
        async fn read_stream<E: crate::Event>(
            &self,
            stream_id: StreamId,
        ) -> Result<EventStreamReader<E>, EventStoreError> {
            // Delegate to inner store for reading (returns empty stream)
            self.inner.read_stream(stream_id).await
        }

        async fn append_events(
            &self,
            _writes: StreamWrites,
        ) -> Result<EventStreamSlice, EventStoreError> {
            // ALWAYS return VersionConflict - simulates persistent conflicts
            Err(EventStoreError::VersionConflict)
        }
    }

    /// Test helper: Event store that conflicts N times before succeeding.
    ///
    /// This is a generalized version of ConflictOnceStore that allows testing
    /// different retry scenarios by controlling exactly how many conflicts occur.
    struct ConflictNTimesStore {
        inner: InMemoryEventStore,
        conflict_count: Arc<tokio::sync::Mutex<u32>>,
        conflicts_to_inject: u32,
    }

    impl ConflictNTimesStore {
        fn new(conflicts_to_inject: u32) -> Self {
            Self {
                inner: InMemoryEventStore::new(),
                conflict_count: Arc::new(tokio::sync::Mutex::new(0)),
                conflicts_to_inject,
            }
        }
    }

    impl EventStore for ConflictNTimesStore {
        async fn read_stream<E: crate::Event>(
            &self,
            stream_id: StreamId,
        ) -> Result<EventStreamReader<E>, EventStoreError> {
            self.inner.read_stream(stream_id).await
        }

        async fn append_events(
            &self,
            writes: StreamWrites,
        ) -> Result<EventStreamSlice, EventStoreError> {
            let mut count = self.conflict_count.lock().await;
            if *count < self.conflicts_to_inject {
                // Inject conflict
                *count += 1;
                Err(EventStoreError::VersionConflict)
            } else {
                // Succeed normally
                self.inner.append_events(writes).await
            }
        }
    }

    /// Integration test: Verify execute() uses fixed backoff delay (no exponential growth).
    ///
    /// This test ensures that library consumers can configure a fixed delay between
    /// retry attempts instead of the default exponential backoff. When a developer
    /// specifies BackoffStrategy::Fixed with a 50ms delay, each retry should wait
    /// exactly 50ms (not 10ms, 20ms, 40ms, etc. from exponential backoff).
    ///
    /// This is critical for scenarios where predictable retry timing is more important
    /// than exponential backoff (e.g., rate-limited APIs with known retry windows).
    #[tokio::test]
    async fn test_execute_with_fixed_backoff_strategy() {
        // Given: A retry policy with fixed 50ms backoff (not exponential)
        let policy = RetryPolicy::new()
            .max_retries(2)
            .backoff_strategy(BackoffStrategy::Fixed { delay_ms: 50 });

        // And: An event store that conflicts twice then succeeds
        let store = ConflictNTimesStore::new(2);

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes with fixed backoff policy
        let start = std::time::Instant::now();
        let result = execute(&store, command, policy).await;
        let elapsed = start.elapsed();

        // Then: Command succeeds after 2 retries (3 total attempts)
        assert!(result.is_ok(), "command should succeed after 2 retries");

        // And: Total delay is ~100ms (2 retries × 50ms fixed)
        // Allow ±30ms tolerance for test timing variance
        assert!(
            elapsed.as_millis() >= 70 && elapsed.as_millis() <= 130,
            "expected ~100ms for 2 retries with 50ms fixed backoff, got {}ms",
            elapsed.as_millis()
        );
    }

    /// Integration test: Verify execute() disables retry when max_retries is zero.
    ///
    /// This test ensures that library consumers can disable automatic retry entirely
    /// by setting max_retries(0). This is useful in testing scenarios where developers
    /// want immediate failure on ConcurrencyError without retry overhead.
    ///
    /// When max_retries=0, execute() should return ConcurrencyError immediately on
    /// the first conflict without attempting any retries or backoff delays.
    #[tokio::test]
    async fn test_execute_with_zero_max_retries_disables_retry() {
        // Given: A retry policy with max_retries set to 0 (no retry)
        let policy = RetryPolicy::new().max_retries(0);

        // And: An event store that ALWAYS conflicts
        let store = AlwaysConflictStore::new();

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes with zero max_retries
        let start = std::time::Instant::now();
        let result = execute(&store, command, policy).await;
        let elapsed = start.elapsed();

        // Then: Returns ConcurrencyError immediately (no retry attempts)
        assert!(
            matches!(result, Err(CommandError::ConcurrencyError(0))),
            "should return ConcurrencyError(0) for zero max_retries, but got: {:?}",
            result
        );

        // And: Executes quickly (no backoff delays)
        assert!(
            elapsed.as_millis() < 10,
            "expected <10ms for immediate failure, got {}ms",
            elapsed.as_millis()
        );
    }

    /// Integration test: Verify execute() emits comprehensive tracing spans and events.
    ///
    /// This test ensures that execute() provides production-ready observability through
    /// structured tracing. Operations teams need visibility into retry behavior, timing,
    /// and success/failure outcomes for debugging and monitoring.
    ///
    /// Tests tracing requirements from I-003 (Configurable Retry Policies):
    /// - Execution span with structured fields (stream_id, command type)
    /// - Retry warning events with structured fields (attempt, delay_ms)
    /// - Success event when command completes
    /// - Error event when retries exhausted
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_execute_emits_tracing_spans_and_events() {
        // Given: An event store that conflicts twice then succeeds
        let store = ConflictNTimesStore::new(2);

        // And: A retry policy that allows 3 retries (4 total attempts)
        let policy = RetryPolicy::new().max_retries(3);

        // And: A test command with identifiable stream
        let stream_id = StreamId::try_new("account-123").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes the command (will retry twice then succeed)
        let result = execute(&store, command, policy.clone()).await;

        // Then: Command succeeds after retries
        assert!(
            result.is_ok(),
            "command should succeed after 2 retries, got: {:?}",
            result
        );

        // And: Execution span created
        assert!(
            logs_contain(":execute:"),
            "should create execution span named 'execute'"
        );

        // And: Success event logged after retries succeed
        // This WILL FAIL until we add tracing::info! for success
        assert!(
            logs_contain("command execution succeeded") || logs_contain("execution complete"),
            "should log success event when command completes"
        )
    }

    /// Integration test: Verify retry warnings include structured fields.
    ///
    /// This test ensures retry warnings emit structured fields (attempt, delay_ms, stream_id)
    /// instead of just formatted strings. Structured fields enable metrics systems and log
    /// aggregation tools to extract machine-readable data for dashboards and alerts.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_retry_warnings_include_structured_fields() {
        // Given: Event store that conflicts 2 times before succeeding
        let store = ConflictNTimesStore::new(2);

        // And: Policy allowing 3 retries
        let policy = RetryPolicy::new().max_retries(3);

        // And: A command with identifiable stream
        let stream_id = StreamId::try_new("test-stream-123").expect("valid stream id");
        let command = MockCommand {
            stream_id: stream_id.clone(),
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Execute command that will retry twice
        let result = execute(&store, command, policy).await;

        // Then: Command succeeds after retries
        assert!(result.is_ok(), "command should succeed after 2 retries");

        // And: Logs contain structured field data
        // Note: tracing-test shows fields as "key=value" in log output
        assert!(
            logs_contain("attempt="),
            "logs should contain attempt field"
        );
        assert!(
            logs_contain("delay_ms="),
            "logs should contain delay_ms field"
        );
        assert!(
            logs_contain("streams="),
            "logs should contain streams field"
        );
    }

    /// Integration test: Verify error event logged when retries exhausted.
    ///
    /// This test ensures that when all retry attempts are exhausted, an error
    /// event is logged with structured fields for debugging and monitoring.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_error_event_when_retries_exhausted() {
        // Given: Event store that always conflicts
        let store = AlwaysConflictStore::new();

        // And: Policy allowing only 2 retries
        let policy = RetryPolicy::new().max_retries(2);

        // And: A test command
        let stream_id = StreamId::try_new("always-fails").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Execute command that will exhaust all retries
        let result = execute(&store, command, policy).await;

        // Then: Execution fails
        assert!(
            matches!(result, Err(CommandError::ConcurrencyError(2))),
            "should fail after exhausting retries"
        );

        // And: Error event was logged with structured fields
        assert!(
            logs_contain("ERROR"),
            "should log error event when retries exhausted"
        );
        assert!(
            logs_contain("max_retries="),
            "error event should include max_retries field"
        );
        assert!(
            logs_contain("streams="),
            "error event should include streams field"
        );
    }

    /// Integration test: Verify metrics hook receives retry lifecycle events.
    ///
    /// This test ensures that library consumers can integrate with metrics systems
    /// like Prometheus by implementing a MetricsHook trait. The hook should receive
    /// callbacks at key retry lifecycle points (attempt, success, failure) with
    /// structured context data.
    ///
    /// This enables operations teams to track retry rates, failure rates, and
    /// backoff delays in dashboards and alerts separate from tracing logs.
    #[tokio::test]
    async fn test_metrics_hook_called_during_retry() {
        // Given: A mock metrics hook that counts retry attempts
        use std::sync::atomic::AtomicUsize;

        struct MockMetricsHook {
            retry_count: Arc<AtomicUsize>,
        }

        impl MetricsHook for MockMetricsHook {
            fn on_retry_attempt(&self, _ctx: &RetryContext) {
                self.retry_count.fetch_add(1, Ordering::SeqCst);
            }
        }

        let retry_count = Arc::new(AtomicUsize::new(0));
        let hook = MockMetricsHook {
            retry_count: Arc::clone(&retry_count),
        };

        // And: A retry policy configured with the metrics hook
        let policy = RetryPolicy::new().max_retries(2).with_metrics_hook(hook);

        // And: Event store that conflicts once before succeeding
        let store = ConflictNTimesStore::new(1);

        // And: A test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Execute command that will retry once
        let result = execute(&store, command, policy).await;

        // Then: Command succeeds after one retry
        assert!(result.is_ok(), "command should succeed after retry");

        // And: Metrics hook was called exactly once for the retry attempt
        assert_eq!(
            retry_count.load(Ordering::SeqCst),
            1,
            "metrics hook should be called once for the single retry attempt"
        );
    }

    #[cfg(test)]
    mod jitter_tests {
        use super::*;

        /// Unit test: Verify minimum jitter factor calculation (0.8).
        ///
        /// When random_value = 0.0, the formula should produce:
        /// 1.0 + (0.0 - 0.5) * 0.4 = 1.0 + (-0.5 * 0.4) = 1.0 - 0.2 = 0.8
        #[test]
        fn test_calculate_jitter_factor_minimum() {
            let result = calculate_jitter_factor(0.0);
            assert_eq!(result, 0.8);
        }

        /// Unit test: Verify no jitter factor (1.0).
        ///
        /// When random_value = 0.5, the formula should produce:
        /// 1.0 + (0.5 - 0.5) * 0.4 = 1.0 + 0.0 = 1.0
        #[test]
        fn test_calculate_jitter_factor_no_jitter() {
            let result = calculate_jitter_factor(0.5);
            assert_eq!(result, 1.0);
        }

        /// Unit test: Verify maximum jitter factor calculation (1.2).
        ///
        /// When random_value = 1.0, the formula should produce:
        /// 1.0 + (1.0 - 0.5) * 0.4 = 1.0 + (0.5 * 0.4) = 1.0 + 0.2 = 1.2
        #[test]
        fn test_calculate_jitter_factor_maximum() {
            let result = calculate_jitter_factor(1.0);
            assert_eq!(result, 1.2);
        }

        /// Unit test: Verify minimum jitter application (80% of base).
        ///
        /// When base_delay = 100 and jitter_factor = 0.8:
        /// 100 * 0.8 = 80
        #[test]
        fn test_apply_jitter_minimum() {
            let result = apply_jitter(100, 0.8);
            assert_eq!(result, 80);
        }

        /// Unit test: Verify no jitter application (100% of base).
        ///
        /// When base_delay = 100 and jitter_factor = 1.0:
        /// 100 * 1.0 = 100
        #[test]
        fn test_apply_jitter_no_jitter() {
            let result = apply_jitter(100, 1.0);
            assert_eq!(result, 100);
        }

        /// Unit test: Verify maximum jitter application (120% of base).
        ///
        /// When base_delay = 100 and jitter_factor = 1.2:
        /// 100 * 1.2 = 120
        #[test]
        fn test_apply_jitter_maximum() {
            let result = apply_jitter(100, 1.2);
            assert_eq!(result, 120);
        }

        /// Unit test: Verify zero base delay handling.
        ///
        /// When base_delay = 0, regardless of jitter_factor:
        /// 0 * 1.0 = 0
        #[test]
        fn test_apply_jitter_zero_base_delay() {
            let result = apply_jitter(0, 1.0);
            assert_eq!(result, 0);
        }

        /// Unit test: Verify large value jitter application.
        ///
        /// When base_delay = 10000 and jitter_factor = 1.1:
        /// 10000 * 1.1 = 11000
        #[test]
        fn test_apply_jitter_large_values() {
            let result = apply_jitter(10000, 1.1);
            assert_eq!(result, 11000);
        }
    }
}
