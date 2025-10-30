mod command;
mod errors;
mod store;

// Re-export only the minimal public API needed for execute() signature
pub use command::{CommandLogic, Event, NewEvents};
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
pub struct ExecutionResponse;

/// Execute a command against the event store.
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
/// # Errors
///
/// Returns [`CommandError`] if:
/// - Stream resolution fails
/// - Event loading fails
/// - Business rule validation fails (via command's `handle()`)
/// - Event persistence fails
/// - Optimistic concurrency conflicts occur
pub async fn execute<C, S>(store: S, command: C) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    const MAX_ATTEMPTS: u32 = 5;
    const BASE_DELAY_MS: u64 = 10;

    for attempt in 0..MAX_ATTEMPTS {
        // Read existing events from the command's stream
        let stream_id = command.stream_id().clone();
        let reader = store
            .read_stream::<C::Event>(stream_id)
            .await
            .map_err(CommandError::EventStoreError)?;

        // Capture the stream version (number of events) for optimistic concurrency control
        let expected_version = store::StreamVersion::new(reader.len());

        // Reconstruct state by folding events via apply()
        let state = reader
            .into_iter()
            .fold(C::State::default(), |acc, event| command.apply(acc, &event));

        // Call handle() with reconstructed state
        let new_events = command.handle(state)?;

        // Convert NewEvents to StreamWrites with version information and store atomically
        let writes: StreamWrites = Vec::from(new_events)
            .into_iter()
            .fold(StreamWrites::new(), |writes, event| {
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
            EventStoreError::VersionConflict => CommandError::ConcurrencyError(attempt + 1),
        });

        match result {
            Ok(_) => return Ok(ExecutionResponse),
            Err(CommandError::ConcurrencyError(_)) if attempt < MAX_ATTEMPTS - 1 => {
                tracing::warn!(
                    "Retrying after concurrency conflict (attempt {} of {})",
                    attempt + 1,
                    MAX_ATTEMPTS
                );

                // Calculate exponential backoff with jitter
                let base_delay = 2_u64
                    .checked_pow(attempt)
                    .and_then(|exp| BASE_DELAY_MS.checked_mul(exp))
                    .unwrap_or(u64::MAX);
                let jitter = 1.0 + (rand::random::<f64>() - 0.5) * 0.4; // ±20%
                let delay_ms = (base_delay as f64 * jitter) as u64;

                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue; // Retry
            }
            Err(CommandError::ConcurrencyError(_)) => {
                return Err(CommandError::ConcurrencyError(MAX_ATTEMPTS));
            }
            Err(e) => return Err(e), // Other permanent errors
        }
    }

    unreachable!("loop always returns before MAX_ATTEMPTS")
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

        fn stream_id(&self) -> &StreamId {
            &self.stream_id
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
        let result = execute(&store, command).await;

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

        fn stream_id(&self) -> &StreamId {
            &self.stream_id
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
        let result = execute(&store, command).await;

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
        execute(&store, command)
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
        let result = execute(&store, command).await;

        // Then: Command succeeds automatically (retry transparent to developer)
        assert!(
            result.is_ok(),
            "execute() should retry automatically and succeed, but got: {:?}",
            result
        );

        // And: Retry attempt should be logged for observability
        // From I-002 Scenario 2: "log shows 'Retry attempt 1/5 for stream...'"
        // Use tracing-test crate to capture and verify logs
        assert!(
            logs_contain("Retrying"),
            "logs should contain retry message"
        );

        // Verify log message contains attempt number and total
        logs_assert(|lines: &[&str]| {
            let retry_logs: Vec<_> = lines
                .iter()
                .filter(|line| line.contains("Retrying"))
                .collect();

            if retry_logs.len() != 1 {
                return Err(format!(
                    "Expected exactly one retry log entry, but found {}",
                    retry_logs.len()
                ));
            }

            let log_msg = retry_logs[0];
            if !log_msg.contains("attempt 1") {
                return Err(format!("Log should contain 'attempt 1', got: {}", log_msg));
            }
            if !log_msg.contains("5") {
                return Err(format!(
                    "Log should contain max attempts '5', got: {}",
                    log_msg
                ));
            }

            Ok(())
        });
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
        struct AlwaysConflictStore {
            inner: InMemoryEventStore,
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

        let store = AlwaysConflictStore {
            inner: InMemoryEventStore::new(),
        };

        // And: A simple test command
        let stream_id = StreamId::try_new("test-stream").expect("valid stream id");
        let command = MockCommand {
            stream_id,
            handle_called: Arc::new(AtomicBool::new(false)),
        };

        // When: Developer executes the command
        let result = execute(&store, command).await;

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
                error_msg, "concurrency conflict after 5 retry attempts",
                "error message should clearly explain that retries were exhausted"
            );
        }
    }
}
