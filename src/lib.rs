mod command;
mod errors;
mod store;

// Re-export only the minimal public API needed for execute() signature
pub use command::{CommandLogic, Event, NewEvents};
pub use errors::CommandError;
pub use store::EventStore;

// Re-export InMemoryEventStore for library consumers (per ADR-011)
pub use store::{InMemoryEventStore, StreamId, StreamWrites};

/// Represents the successful outcome of command execution.
///
/// This type is returned when a command completes successfully, including
/// state reconstruction, business rule validation, and atomic event persistence.
/// The specific data included in this response is yet to be determined based
/// on actual usage requirements.
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
    // Read existing events from the command's stream
    let stream_id = command.stream_id().clone();
    let reader = store
        .read_stream::<C::Event>(stream_id)
        .await
        .map_err(|_| CommandError::BusinessRuleViolation("failed to read stream".into()))?;

    // QUESTION: should we add a `fold` method to the reader? Is there a trait that already has
    // that method signature?
    //
    // Reconstruct state by folding events via apply()
    let state = reader
        .iter()
        .fold(C::State::default(), |acc, event| command.apply(acc, event));

    // Call handle() with reconstructed state
    let new_events = command.handle(state)?;

    // Convert NewEvents to StreamWrites and store atomically
    let writes: StreamWrites = Vec::from(new_events).into_iter().collect();
    store.append_events(writes).await?;

    Ok(ExecutionResponse)
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
        let writes: StreamWrites = vec![seed_event].into_iter().collect();
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
}
