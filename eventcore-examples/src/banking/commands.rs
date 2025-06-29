//! Commands for the banking domain
//!
//! This module demonstrates the aggregate-per-command pattern where each command
//! owns its state model and can read from/write to multiple streams atomically.

use crate::banking::{
    events::{AccountOpened, BankingEvent, MoneyTransferred},
    types::{AccountHolder, AccountId, Money, TransferId},
};
use async_trait::async_trait;
use eventcore::{Command, CommandError, CommandResult, StoredEvent, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors specific to banking commands
#[derive(Debug, Error)]
pub enum BankingError {
    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(AccountId),

    /// Account already exists
    #[error("Account already exists: {0}")]
    AccountAlreadyExists(AccountId),

    /// Insufficient funds for operation
    #[error("Insufficient funds in account {account}: balance {balance}, requested {requested}")]
    InsufficientFunds {
        /// Account with insufficient funds
        account: AccountId,
        /// Current balance
        balance: Money,
        /// Requested amount
        requested: Money,
    },

    /// Transfer to same account
    #[error("Cannot transfer to same account: {0}")]
    SameAccountTransfer(AccountId),
}

/// Input for opening a new account - validates input at construction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAccountInput {
    /// Account ID - must be valid according to `AccountId` rules
    pub account_id: AccountId,
    /// Account holder information
    pub holder: AccountHolder,
    /// Initial deposit amount (can be zero)
    pub initial_deposit: Money,
}

impl OpenAccountInput {
    /// Creates a new `OpenAccountInput` with validation
    ///
    /// This smart constructor ensures all inputs are valid at construction time.
    /// No further validation is needed once this type is created.
    pub fn new(account_id: AccountId, holder: AccountHolder, initial_deposit: Money) -> Self {
        // All types are already validated through their smart constructors
        Self {
            account_id,
            holder,
            initial_deposit,
        }
    }
}

/// State for the `OpenAccount` command
#[derive(Debug, Default, Clone)]
pub struct OpenAccountState {
    /// Track which accounts already exist
    pub existing_accounts: HashMap<AccountId, bool>,
}

/// Command to open a new bank account
#[derive(Debug, Clone)]
pub struct OpenAccountCommand;

#[async_trait]
impl Command for OpenAccountCommand {
    type Input = OpenAccountInput;
    type State = OpenAccountState;
    type Event = BankingEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Read from the account's stream to check if it exists
        vec![StreamId::try_new(format!("account-{}", input.account_id)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let BankingEvent::AccountOpened(e) = &event.payload {
            state.existing_accounts.insert(e.account_id.clone(), true);
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        // Check if account already exists
        if state.existing_accounts.contains_key(&input.account_id) {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::AccountAlreadyExists(input.account_id).to_string(),
            ));
        }

        // Create the account opened event
        let event = AccountOpened {
            account_id: input.account_id.clone(),
            holder: input.holder,
            initial_balance: input.initial_deposit,
        };

        // Write to the account's stream
        let stream_id = StreamId::try_new(format!("account-{}", input.account_id)).unwrap();

        Ok(vec![(stream_id, BankingEvent::from(event))])
    }
}

/// Input for transferring money - validates input at construction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferMoneyInput {
    /// Unique transfer ID
    pub transfer_id: TransferId,
    /// Account to transfer from
    pub from_account: AccountId,
    /// Account to transfer to
    pub to_account: AccountId,
    /// Amount to transfer
    pub amount: Money,
    /// Optional description
    pub description: Option<String>,
}

impl TransferMoneyInput {
    /// Creates a new `TransferMoneyInput` with validation
    ///
    /// # Errors
    ///
    /// Returns an error if from and to accounts are the same
    pub fn new(
        transfer_id: TransferId,
        from_account: AccountId,
        to_account: AccountId,
        amount: Money,
        description: Option<String>,
    ) -> Result<Self, BankingError> {
        // Validate that from and to accounts are different
        if from_account == to_account {
            return Err(BankingError::SameAccountTransfer(from_account));
        }

        Ok(Self {
            transfer_id,
            from_account,
            to_account,
            amount,
            description,
        })
    }
}

/// State for the `TransferMoney` command
#[derive(Debug, Default, Clone)]
pub struct TransferMoneyState {
    /// Account balances
    pub balances: HashMap<AccountId, Money>,
    /// Track completed transfers to ensure idempotency
    pub completed_transfers: HashMap<TransferId, bool>,
}

/// Command to transfer money between accounts
///
/// This demonstrates multi-stream atomic operations - the command reads from
/// and writes to multiple account streams atomically.
#[derive(Debug, Clone)]
pub struct TransferMoneyCommand;

#[async_trait]
impl Command for TransferMoneyCommand {
    type Input = TransferMoneyInput;
    type State = TransferMoneyState;
    type Event = BankingEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Read from both account streams and the transfer log
        vec![
            StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
            StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
            StreamId::try_new("transfers".to_string()).unwrap(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::AccountOpened(e) => {
                state
                    .balances
                    .insert(e.account_id.clone(), e.initial_balance);
            }
            BankingEvent::MoneyTransferred(e) => {
                // Update balances based on transfer
                if let Some(from_balance) = state.balances.get_mut(&e.from_account) {
                    // Safe to unwrap because we validated sufficient funds
                    *from_balance = from_balance.subtract(&e.amount).unwrap();
                }

                if let Some(to_balance) = state.balances.get_mut(&e.to_account) {
                    // Safe to unwrap because addition of valid money values is safe
                    *to_balance = to_balance.add(&e.amount).unwrap();
                }

                // Mark transfer as completed
                state
                    .completed_transfers
                    .insert(e.transfer_id.clone(), true);
            }
            BankingEvent::MoneyDeposited(e) => {
                if let Some(balance) = state.balances.get_mut(&e.account_id) {
                    *balance = balance.add(&e.amount).unwrap();
                }
            }
            BankingEvent::MoneyWithdrawn(e) => {
                if let Some(balance) = state.balances.get_mut(&e.account_id) {
                    *balance = balance.subtract(&e.amount).unwrap();
                }
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        // Check for idempotency - if transfer already completed, return success
        if state.completed_transfers.contains_key(&input.transfer_id) {
            return Ok(vec![]); // Already processed, no new events
        }

        // Check if both accounts exist
        let from_balance = state.balances.get(&input.from_account).ok_or_else(|| {
            CommandError::BusinessRuleViolation(
                BankingError::AccountNotFound(input.from_account.clone()).to_string(),
            )
        })?;

        if !state.balances.contains_key(&input.to_account) {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::AccountNotFound(input.to_account.clone()).to_string(),
            ));
        }

        // Check sufficient funds
        if from_balance.subtract(&input.amount).is_err() {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::InsufficientFunds {
                    account: input.from_account.clone(),
                    balance: *from_balance,
                    requested: input.amount,
                }
                .to_string(),
            ));
        }

        // Create the transfer event
        let event = MoneyTransferred {
            transfer_id: input.transfer_id,
            from_account: input.from_account.clone(),
            to_account: input.to_account.clone(),
            amount: input.amount,
            description: input.description,
        };

        // Write to all affected streams for atomicity
        let events = vec![
            (
                StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
                BankingEvent::from(event.clone()),
            ),
            (
                StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
                BankingEvent::from(event.clone()),
            ),
            (
                StreamId::try_new("transfers".to_string()).unwrap(),
                BankingEvent::from(event),
            ),
        ];

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banking::types::CustomerName;

    fn create_test_holder() -> AccountHolder {
        AccountHolder {
            name: CustomerName::try_new("John Doe".to_string()).unwrap(),
            email: "john@example.com".to_string(),
        }
    }

    #[test]
    fn transfer_input_rejects_same_account() {
        let account_id = AccountId::generate();
        let result = TransferMoneyInput::new(
            TransferId::generate(),
            account_id.clone(),
            account_id,
            Money::from_cents(1000).unwrap(),
            None,
        );

        assert!(matches!(result, Err(BankingError::SameAccountTransfer(_))));
    }

    #[test]
    fn open_account_input_valid() {
        let input =
            OpenAccountInput::new(AccountId::generate(), create_test_holder(), Money::zero());

        // If we can create it, it's valid
        assert_eq!(input.initial_deposit, Money::zero());
    }

    #[test]
    fn transfer_input_valid() {
        let from = AccountId::generate();
        let to = AccountId::generate();

        let input = TransferMoneyInput::new(
            TransferId::generate(),
            from,
            to,
            Money::from_cents(5000).unwrap(),
            Some("Test transfer".to_string()),
        )
        .unwrap();

        assert_eq!(input.amount, Money::from_cents(5000).unwrap());
    }
}
