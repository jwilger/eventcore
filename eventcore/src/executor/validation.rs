//! Command validation logic and utilities.
//! 
//! This module contains validation functions for ensuring commands and execution
//! contexts meet the requirements for safe execution.

use crate::command::{Command, CommandResult};
use crate::errors::CommandError;
use crate::executor::stream_discovery::StreamDiscoveryContext;

/// Validates that the iteration limit has not been exceeded for stream discovery.
pub(crate) fn validate_iteration_limit<C: Command>(
    context: &StreamDiscoveryContext,
) -> CommandResult<()> {
    if context.iteration() > context.max_iterations() {
        return Err(CommandError::ValidationFailed(format!(
            "Command '{}' exceeded maximum stream discovery iterations ({}). This suggests the command is continuously discovering new streams. Current streams: {:?}",
            std::any::type_name::<C>(),
            context.max_iterations(),
            context.stream_ids().iter().map(std::convert::AsRef::as_ref).collect::<Vec<_>>()
        )));
    }
    Ok(())
}