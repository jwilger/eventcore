//! Helper macros for EventCore commands.
//!
//! This module provides helper macros that work with the `#[derive(Command)]`
//! procedural macro from `eventcore-macros` to reduce boilerplate in command implementations.

#[cfg(test)]
mod tests;

/// Helper macro to check business rules in command handlers.
///
/// This macro provides a convenient way to enforce business rules in command
/// implementations. If the condition is false, it returns a `BusinessRuleViolation`
/// error with the provided message.
///
/// # Example
///
/// ```ignore
/// use eventcore::require;
///
/// async fn handle(
///     &self,
///     read_streams: ReadStreams<Self::StreamSet>,
///     state: Self::State,
///     input: Self::Input,
///     stream_resolver: &mut StreamResolver,
/// ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///     require!(state.balance >= input.amount, "Insufficient funds");
///     require!(state.is_active, "Account is not active");
///     
///     // ... rest of the logic
/// }
/// ```
#[macro_export]
macro_rules! require {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err($crate::CommandError::BusinessRuleViolation(
                $message.to_string(),
            ));
        }
    };
}

/// Helper macro to simplify event creation in command handlers.
///
/// This macro provides a more concise syntax for creating `StreamWrite` instances
/// with proper error handling. It's particularly useful when you need to create
/// multiple events in a command handler.
///
/// # Example
///
/// ```ignore
/// use eventcore::emit;
///
/// async fn handle(
///     &self,
///     read_streams: ReadStreams<Self::StreamSet>,
///     state: Self::State,
///     input: Self::Input,
///     stream_resolver: &mut StreamResolver,
/// ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///     let mut events = vec![];
///     
///     emit!(events, &read_streams, input.account_stream, AccountDebited {
///         amount: input.amount,
///         reference: input.reference,
///     });
///     
///     emit!(events, &read_streams, input.target_stream, AccountCredited {
///         amount: input.amount,
///         reference: input.reference,
///     });
///     
///     Ok(events)
/// }
/// ```
#[macro_export]
macro_rules! emit {
    ($events:expr, $read_streams:expr, $stream_id:expr, $event:expr) => {
        $events.push($crate::StreamWrite::new($read_streams, $stream_id, $event)?);
    };
}
