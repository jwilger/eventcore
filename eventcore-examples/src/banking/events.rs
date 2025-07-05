//! Events for the banking domain
//!
//! These events capture all state changes in the banking system.

use crate::banking::types::{AccountHolder, AccountId, Money, TransferId};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// Event emitted when a new account is opened
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountOpened {
    /// The unique identifier for the account
    pub account_id: AccountId,
    /// Information about the account holder
    pub holder: AccountHolder,
    /// Initial balance (usually zero)
    pub initial_balance: Money,
}

/// Event emitted when money is transferred between accounts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyTransferred {
    /// The unique identifier for this transfer
    pub transfer_id: TransferId,
    /// Account sending the money
    pub from_account: AccountId,
    /// Account receiving the money
    pub to_account: AccountId,
    /// Amount being transferred
    pub amount: Money,
    /// Optional description/reference for the transfer
    pub description: Option<String>,
}

/// Event emitted when money is deposited into an account
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyDeposited {
    /// The account receiving the deposit
    pub account_id: AccountId,
    /// Amount being deposited
    pub amount: Money,
    /// Reference/description for the deposit
    pub reference: String,
}

/// Event emitted when money is withdrawn from an account
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyWithdrawn {
    /// The account money is withdrawn from
    pub account_id: AccountId,
    /// Amount being withdrawn
    pub amount: Money,
    /// Reference/description for the withdrawal
    pub reference: String,
}

/// All possible events in the banking domain
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BankingEvent {
    /// Account was opened
    AccountOpened(AccountOpened),
    /// Money was transferred
    MoneyTransferred(MoneyTransferred),
    /// Money was deposited
    MoneyDeposited(MoneyDeposited),
    /// Money was withdrawn
    MoneyWithdrawn(MoneyWithdrawn),
}

impl From<AccountOpened> for BankingEvent {
    fn from(event: AccountOpened) -> Self {
        Self::AccountOpened(event)
    }
}

impl From<MoneyTransferred> for BankingEvent {
    fn from(event: MoneyTransferred) -> Self {
        Self::MoneyTransferred(event)
    }
}

impl From<MoneyDeposited> for BankingEvent {
    fn from(event: MoneyDeposited) -> Self {
        Self::MoneyDeposited(event)
    }
}

impl From<MoneyWithdrawn> for BankingEvent {
    fn from(event: MoneyWithdrawn) -> Self {
        Self::MoneyWithdrawn(event)
    }
}

// Implement TryFrom for BankingEvent reference (required by CommandExecutor)
impl TryFrom<&Self> for BankingEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}
