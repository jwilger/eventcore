//! Procedural macros for the EventCore event sourcing library.
//!
//! This crate provides derive macros and attribute macros to reduce boilerplate
//! when implementing commands and other EventCore patterns.

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod command;
mod stream_set;
mod utils;

use command::expand_derive_command;

/// Derive macro for implementing the CommandStreams trait.
///
/// This macro generates a complete implementation of `CommandStreams` based on
/// fields marked with `#[stream]`. You only need to implement `CommandLogic`
/// for your domain logic.
///
/// # Example
///
/// ```ignore
/// use eventcore_macros::Command;
/// use eventcore::{CommandLogic, prelude::*};
///
/// #[derive(Command, Clone)]
/// struct TransferMoney {
///     #[stream]
///     from_account: StreamId,
///     #[stream]
///     to_account: StreamId,
///     amount: Money,
/// }
///
/// #[async_trait]
/// impl CommandLogic for TransferMoney {
///     type State = AccountBalances;
///     type Event = BankingEvent;
///
///     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
///         // Your event folding logic
///     }
///
///     async fn handle(
///         &self,
///         read_streams: ReadStreams<Self::StreamSet>,
///         state: Self::State,
///         input: Self::Input,
///         stream_resolver: &mut StreamResolver,
///     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///         // Your business logic
///     }
/// }
/// ```
///
/// This automatically generates:
/// - Complete `CommandStreams` trait implementation
/// - `type Input = Self` (the command struct serves as input)
/// - `type StreamSet = TransferMoneyStreamSet` (phantom type for stream access)
/// - `fn read_streams()` implementation extracting from `#[stream]` fields
#[proc_macro_derive(Command, attributes(stream))]
pub fn derive_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_command(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Internal module for testing macro expansion.
#[cfg(test)]
mod tests {
    // Note: Procedural macros cannot be tested directly in unit tests
    // because they require the proc macro bridge which is only available
    // when the macro is actually being used as a procedural macro.
    //
    // Proper testing should be done through integration tests using
    // the trybuild crate or by testing the internal functions directly.

    #[test]
    fn test_macro_crate_compiles() {
        // Simple test to ensure the crate compiles
        // The test itself is the compilation check
        let _placeholder = 1 + 1;
    }
}
