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
    // Call command.handle() with default state
    let state = C::State::default();
    let new_events = command.handle(state)?;

    // Convert NewEvents to StreamWrites
    let mut writes = StreamWrites::new();
    for event in Vec::from(new_events) {
        writes = writes.append(event);
    }

    // Store events atomically
    store
        .append_events(writes)
        .await
        .map_err(|_| CommandError::EventStoreError)?;

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
        handle_called: Arc<AtomicBool>,
    }

    impl CommandLogic for MockCommand {
        type Event = TestEvent;
        type State = ();

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
        let handle_called = Arc::new(AtomicBool::new(false));
        let command = MockCommand {
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
}
