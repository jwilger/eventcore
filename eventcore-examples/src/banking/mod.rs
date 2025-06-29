//! Banking example demonstrating account management and money transfers
//!
//! This example shows how to implement a banking domain using EventCore with:
//! - Type-safe money handling with the Money type
//! - Account management with AccountId
//! - Money transfers with TransferId
//! - Commands for opening accounts and transferring money
//! - Projections for account balances

pub mod commands;
pub mod events;
pub mod projections;
pub mod types;

#[cfg(test)]
mod tests;

// Re-export commonly used types
pub use commands::{OpenAccountCommand, TransferMoneyCommand};
pub use events::{AccountOpened, MoneyTransferred};
pub use projections::{AccountBalanceProjection, AccountBalanceProjectionImpl};
pub use types::{AccountId, Money, TransferId};
