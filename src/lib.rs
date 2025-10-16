mod command;
mod errors;
mod store;

// Re-export only the minimal public API needed for execute() signature
use command::CommandLogic;
use errors::CommandError;
use store::EventStore;

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
pub async fn execute<C, S>(_store: S, _command: C) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    unimplemented!()
}
