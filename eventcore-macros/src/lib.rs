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

/// Derive macro for implementing the Command trait.
///
/// # Example
///
/// ```ignore
/// use eventcore_macros::Command;
/// use eventcore::types::StreamId;
///
/// #[derive(Command)]
/// struct TransferMoney {
///     #[stream]
///     from_account: StreamId,
///     #[stream]
///     to_account: StreamId,
///     amount: Money,
/// }
/// ```
///
/// This will automatically:
/// - Generate a type-safe StreamSet type
/// - Implement the `read_streams` method based on `#[stream]` attributes
/// - Generate boilerplate for the Command trait implementation
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
