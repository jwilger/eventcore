/// Error type for command execution failures.
///
/// This represents all possible failure modes during command execution:
/// - Stream resolution failures
/// - Event loading failures
/// - Business rule validation failures
/// - Event persistence failures
/// - Optimistic concurrency conflicts
#[derive(Debug)]
pub struct CommandError;