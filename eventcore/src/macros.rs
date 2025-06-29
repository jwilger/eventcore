//! Declarative macros for EventCore commands.
//!
//! This module provides declarative macros that complement the procedural macros
//! from `eventcore-macros`, offering a more concise syntax for common patterns.

#[cfg(test)]
mod tests;

/// Declarative macro for defining commands with a unified structure.
///
/// This macro provides a concise way to define a command's state, events,
/// and business logic. It's designed to work alongside the `#[derive(Command)]`
/// procedural macro from `eventcore-macros`.
///
/// # Note
///
/// Due to the complexity of Rust's macro system and the need for proper type
/// name generation, this macro is currently a placeholder. The full implementation
/// would require either:
///
/// 1. Manual specification of all type names (State, Event, StreamSet)
/// 2. Use of a proc-macro that can properly generate type names
/// 3. Integration with the paste crate (which would add a dependency)
///
/// For now, users should use the `#[derive(Command)]` procedural macro
/// along with manual implementation of the Command trait methods.
///
/// # Future Syntax (Planned)
///
/// ```ignore
/// command! {
///     pub struct TransferMoney {
///         #[stream]
///         from_account: StreamId,
///         #[stream]
///         to_account: StreamId,
///         amount: Money,
///     }
///     
///     state {
///         from_balance: Money,
///         to_balance: Money,
///     }
///     
///     events {
///         MoneyDebited { account: AccountId, amount: Money },
///         MoneyCredited { account: AccountId, amount: Money },
///     }
///     
///     apply {
///         MoneyDebited { account, amount } => {
///             state.from_balance -= amount;
///         }
///         MoneyCredited { account, amount } => {
///             state.to_balance += amount;
///         }
///     }
///     
///     handle {
///         require!(state.from_balance >= input.amount, "Insufficient funds");
///         
///         emit!(input.from_account, MoneyDebited {
///             account: input.from_account.into(),
///             amount: input.amount,
///         });
///         
///         emit!(input.to_account, MoneyCredited {
///             account: input.to_account.into(),
///             amount: input.amount,
///         });
///     }
/// }
/// ```
#[macro_export]
macro_rules! command {
    ($($tt:tt)*) => {
        compile_error!("The command! macro is not yet implemented. Please use #[derive(Command)] from eventcore-macros instead.");
    };
}

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
            return Err($crate::errors::CommandError::BusinessRuleViolation(
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
        $events.push($crate::command::StreamWrite::new(
            $read_streams,
            $stream_id,
            $event,
        )?);
    };
}
