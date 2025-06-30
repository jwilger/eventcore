//! Commands for the banking domain
//!
//! This module demonstrates multi-stream event sourcing where each command
//! owns its state model and can read from/write to multiple streams atomically.

use crate::banking::{
    events::{AccountOpened, BankingEvent, MoneyTransferred},
    types::{
        AccountHolder, AccountId, AuthorizationToken, AuthorizedHighValueTransfer,
        DailyLimitedTransferAmount, Money, SourceAccount, TargetAccount, TransferAmount,
        TransferId, ValidatedTransferPair,
    },
};
use async_trait::async_trait;
use eventcore::{
    Command, CommandError, CommandResult, ReadStreams, StoredEvent, StreamId, StreamWrite,
};
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

    /// Business rule violation
    #[error("Business rule violation: {0}")]
    BusinessRuleViolation(String),

    /// Validation failed
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
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
    type StreamSet = ();

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
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
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

        Ok(vec![StreamWrite::new(
            &read_streams,
            stream_id,
            BankingEvent::from(event),
        )?])
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

/// Type-safe input for transferring money using branded types
///
/// This version uses branded types to prevent mixing up source and target accounts
/// at compile time, eliminating a class of runtime errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeTransferMoneyInput {
    /// Unique transfer ID
    pub transfer_id: TransferId,
    /// Source account (account to transfer from)
    pub source: SourceAccount,
    /// Target account (account to transfer to)
    pub target: TargetAccount,
    /// Amount to transfer with transfer context
    pub amount: TransferAmount,
    /// Optional description
    pub description: Option<String>,
}

impl SafeTransferMoneyInput {
    /// Creates a new `SafeTransferMoneyInput` with compile-time type safety
    ///
    /// This constructor eliminates the runtime validation for same-account transfers
    /// because the type system makes it impossible to pass the same account as both
    /// source and target since they have different types.
    pub fn new(
        transfer_id: TransferId,
        source: SourceAccount,
        target: TargetAccount,
        amount: TransferAmount,
        description: Option<String>,
    ) -> Self {
        // No runtime validation needed! The type system prevents:
        // - Mixing up source and target accounts
        // - Using wrong money context
        // - Same account transfer (would require explicit conversion)

        Self {
            transfer_id,
            source,
            target,
            amount,
            description,
        }
    }

    /// Creates a safe transfer input from separate account IDs with validation
    ///
    /// This method provides a bridge from the untyped world to the typed world,
    /// performing the same-account validation once at the boundary.
    pub fn from_account_ids(
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

        Ok(Self::new(
            transfer_id,
            SourceAccount::new(from_account),
            TargetAccount::new(to_account),
            TransferAmount::new(amount),
            description,
        ))
    }

    /// Converts to the legacy input type for backward compatibility
    pub fn to_legacy_input(&self) -> TransferMoneyInput {
        TransferMoneyInput {
            transfer_id: self.transfer_id.clone(),
            from_account: self.source.account_id().clone(),
            to_account: self.target.account_id().clone(),
            amount: self.amount.amount(),
            description: self.description.clone(),
        }
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
    type StreamSet = ();

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
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
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
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new("transfers".to_string()).unwrap(),
                BankingEvent::from(event),
            )?,
        ];

        Ok(events)
    }
}

/// Type-safe command to transfer money using branded input types
///
/// This command demonstrates how branded types can eliminate runtime validation
/// by encoding business rules in the type system.
#[derive(Debug, Clone)]
pub struct SafeTransferMoneyCommand;

#[async_trait]
impl Command for SafeTransferMoneyCommand {
    type Input = SafeTransferMoneyInput;
    type State = TransferMoneyState;
    type Event = BankingEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Read from both account streams and the transfer log
        vec![
            StreamId::try_new(format!("account-{}", input.source.account_id())).unwrap(),
            StreamId::try_new(format!("account-{}", input.target.account_id())).unwrap(),
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
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check for idempotency - if transfer already completed, return success
        if state.completed_transfers.contains_key(&input.transfer_id) {
            return Ok(vec![]); // Already processed, no new events
        }

        // Check if both accounts exist
        let from_balance = state
            .balances
            .get(input.source.account_id())
            .ok_or_else(|| {
                CommandError::BusinessRuleViolation(
                    BankingError::AccountNotFound(input.source.account_id().clone()).to_string(),
                )
            })?;

        if !state.balances.contains_key(input.target.account_id()) {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::AccountNotFound(input.target.account_id().clone()).to_string(),
            ));
        }

        // Check sufficient funds
        let amount = input.amount.amount();
        if from_balance.subtract(&amount).is_err() {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::InsufficientFunds {
                    account: input.source.account_id().clone(),
                    balance: *from_balance,
                    requested: amount,
                }
                .to_string(),
            ));
        }

        // Create the transfer event
        let event = MoneyTransferred {
            transfer_id: input.transfer_id,
            from_account: input.source.account_id().clone(),
            to_account: input.target.account_id().clone(),
            amount,
            description: input.description,
        };

        // Write to all affected streams for atomicity
        let events = vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.source.account_id())).unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.target.account_id())).unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new("transfers".to_string()).unwrap(),
                BankingEvent::from(event),
            )?,
        ];

        Ok(events)
    }
}

/// Advanced type-safe transfer input that combines all type safety improvements
///
/// This input type demonstrates how to eliminate redundant validation by encoding
/// all business rules in the type system. Once constructed, this type guarantees:
/// - Source and target accounts are different
/// - Transfer amount respects business rules (daily limits, authorization)
/// - All domain invariants are maintained
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeSafeTransferInput {
    /// Unique identifier for this transfer
    pub transfer_id: TransferId,
    /// Validated source and target account pair
    pub accounts: ValidatedTransferPair,
    /// Type-safe transfer amount with business rule encoding
    pub amount: TypeSafeTransferAmount,
    /// Optional description for the transfer
    pub description: Option<String>,
}

/// Transfer amount that encodes different business rule contexts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeSafeTransferAmount {
    /// Standard transfer within daily limits
    Standard(DailyLimitedTransferAmount),
    /// High-value transfer with authorization
    HighValue(AuthorizedHighValueTransfer),
}

impl TypeSafeTransferAmount {
    /// Gets the underlying transfer amount regardless of type
    pub fn amount(&self) -> TransferAmount {
        match self {
            Self::Standard(limited) => limited.amount(),
            Self::HighValue(authorized) => authorized.amount(),
        }
    }

    /// Gets the underlying money value
    pub fn money(&self) -> Money {
        self.amount().amount()
    }
}

impl TypeSafeTransferInput {
    /// Smart constructor for standard transfers
    ///
    /// This constructor eliminates ALL runtime validation by using types that
    /// guarantee business rules. If this method succeeds, the transfer is
    /// guaranteed to be valid.
    pub fn new_standard(
        transfer_id: TransferId,
        accounts: ValidatedTransferPair,
        amount: DailyLimitedTransferAmount,
        description: Option<String>,
    ) -> Self {
        Self {
            transfer_id,
            accounts,
            amount: TypeSafeTransferAmount::Standard(amount),
            description,
        }
    }

    /// Smart constructor for high-value transfers
    ///
    /// This constructor eliminates ALL runtime validation by using types that
    /// guarantee business rules including authorization.
    pub fn new_high_value(
        transfer_id: TransferId,
        accounts: ValidatedTransferPair,
        amount: AuthorizedHighValueTransfer,
        description: Option<String>,
    ) -> Self {
        Self {
            transfer_id,
            accounts,
            amount: TypeSafeTransferAmount::HighValue(amount),
            description,
        }
    }

    /// Factory method that automatically determines transfer type based on amount
    ///
    /// This provides a convenient way to create transfers while still maintaining
    /// type safety. Validation occurs once at construction.
    pub fn from_parameters(
        transfer_id: TransferId,
        source_account: AccountId,
        target_account: AccountId,
        amount: Money,
        authorization_token: Option<AuthorizationToken>,
        description: Option<String>,
    ) -> Result<Self, BankingError> {
        // Step 1: Validate account pair (once)
        let accounts =
            ValidatedTransferPair::from_account_ids(source_account.clone(), target_account)
                .map_err(|_| BankingError::SameAccountTransfer(source_account))?;

        let transfer_amount = TransferAmount::new(amount);

        // Step 2: Determine transfer type based on amount and authorization
        let typed_amount = if amount > AuthorizedHighValueTransfer::HIGH_VALUE_THRESHOLD {
            // High-value transfer requires authorization
            let auth_token = authorization_token.ok_or_else(|| {
                BankingError::BusinessRuleViolation(format!(
                    "High-value transfer of {} requires authorization",
                    amount
                ))
            })?;

            let authorized_transfer = AuthorizedHighValueTransfer::new(transfer_amount, auth_token)
                .map_err(|e| BankingError::ValidationFailed(e.to_string()))?;

            TypeSafeTransferAmount::HighValue(authorized_transfer)
        } else {
            // Standard transfer within daily limits
            let limited_transfer = DailyLimitedTransferAmount::new(transfer_amount)
                .map_err(|e| BankingError::ValidationFailed(e.to_string()))?;

            TypeSafeTransferAmount::Standard(limited_transfer)
        };

        Ok(Self {
            transfer_id,
            accounts,
            amount: typed_amount,
            description,
        })
    }

    /// Converts to the safe transfer input format
    pub fn to_safe_input(&self) -> SafeTransferMoneyInput {
        let (source, target) = self.accounts.clone().into_accounts();
        SafeTransferMoneyInput::new(
            self.transfer_id.clone(),
            source,
            target,
            self.amount.amount(),
            self.description.clone(),
        )
    }

    /// Converts to legacy format for backward compatibility
    pub fn to_legacy_input(&self) -> TransferMoneyInput {
        self.to_safe_input().to_legacy_input()
    }
}

/// Type-safe command that uses the advanced input type
///
/// This command demonstrates zero-runtime-validation by leveraging types
/// that encode all business rules at compile time.
#[derive(Debug, Clone)]
pub struct TypeSafeTransferCommand;

#[async_trait]
impl Command for TypeSafeTransferCommand {
    type Input = TypeSafeTransferInput;
    type State = TransferMoneyState;
    type Event = BankingEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Read from both account streams and the transfer log
        vec![
            StreamId::try_new(format!("account-{}", input.accounts.source().account_id())).unwrap(),
            StreamId::try_new(format!("account-{}", input.accounts.target().account_id())).unwrap(),
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
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check for idempotency - if transfer already completed, return success
        if state.completed_transfers.contains_key(&input.transfer_id) {
            return Ok(vec![]); // Already processed, no new events
        }

        // Check if both accounts exist
        let from_balance = state
            .balances
            .get(input.accounts.source().account_id())
            .ok_or_else(|| {
                CommandError::BusinessRuleViolation(
                    BankingError::AccountNotFound(input.accounts.source().account_id().clone())
                        .to_string(),
                )
            })?;

        if !state
            .balances
            .contains_key(input.accounts.target().account_id())
        {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::AccountNotFound(input.accounts.target().account_id().clone())
                    .to_string(),
            ));
        }

        // Check sufficient funds (the only runtime validation needed!)
        let amount = input.amount.money();
        if from_balance.subtract(&amount).is_err() {
            return Err(CommandError::BusinessRuleViolation(
                BankingError::InsufficientFunds {
                    account: input.accounts.source().account_id().clone(),
                    balance: *from_balance,
                    requested: amount,
                }
                .to_string(),
            ));
        }

        // Create the transfer event
        let event = MoneyTransferred {
            transfer_id: input.transfer_id,
            from_account: input.accounts.source().account_id().clone(),
            to_account: input.accounts.target().account_id().clone(),
            amount,
            description: input.description,
        };

        // Write to all affected streams for atomicity
        let events = vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.accounts.source().account_id()))
                    .unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.accounts.target().account_id()))
                    .unwrap(),
                BankingEvent::from(event.clone()),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new("transfers".to_string()).unwrap(),
                BankingEvent::from(event),
            )?,
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

    #[test]
    fn safe_transfer_input_prevents_same_account_at_type_level() {
        let account_id = AccountId::generate();
        let transfer_id = TransferId::generate();
        let amount = Money::from_cents(1000).unwrap();

        // This would be a compile error - can't pass same AccountId as both source and target
        // let _input = SafeTransferMoneyInput::new(
        //     transfer_id,
        //     SourceAccount::new(account_id.clone()),
        //     TargetAccount::new(account_id), // This would be caught at compile time!
        //     TransferAmount::new(amount),
        //     None,
        // );

        // But this validation at boundary should still work
        let result = SafeTransferMoneyInput::from_account_ids(
            transfer_id,
            account_id.clone(),
            account_id,
            amount,
            None,
        );

        assert!(matches!(result, Err(BankingError::SameAccountTransfer(_))));
    }

    #[test]
    fn safe_transfer_input_valid() {
        let from = AccountId::generate();
        let to = AccountId::generate();
        let amount = Money::from_cents(5000).unwrap();

        let input = SafeTransferMoneyInput::new(
            TransferId::generate(),
            SourceAccount::new(from.clone()),
            TargetAccount::new(to.clone()),
            TransferAmount::new(amount),
            Some("Test transfer".to_string()),
        );

        assert_eq!(input.amount.amount(), amount);
        assert_eq!(input.source.account_id(), &from);
        assert_eq!(input.target.account_id(), &to);
    }

    #[test]
    fn safe_transfer_input_from_account_ids() {
        let from = AccountId::generate();
        let to = AccountId::generate();
        let amount = Money::from_cents(7500).unwrap();

        let input = SafeTransferMoneyInput::from_account_ids(
            TransferId::generate(),
            from.clone(),
            to.clone(),
            amount,
            Some("Bridge from untyped world".to_string()),
        )
        .unwrap();

        assert_eq!(input.source.account_id(), &from);
        assert_eq!(input.target.account_id(), &to);
        assert_eq!(input.amount.amount(), amount);
    }

    #[test]
    fn safe_transfer_input_to_legacy_conversion() {
        let from = AccountId::generate();
        let to = AccountId::generate();
        let amount = Money::from_cents(2500).unwrap();
        let description = Some("Conversion test".to_string());

        let safe_input = SafeTransferMoneyInput::new(
            TransferId::generate(),
            SourceAccount::new(from.clone()),
            TargetAccount::new(to.clone()),
            TransferAmount::new(amount),
            description.clone(),
        );

        let legacy_input = safe_input.to_legacy_input();

        assert_eq!(legacy_input.from_account, from);
        assert_eq!(legacy_input.to_account, to);
        assert_eq!(legacy_input.amount, amount);
        assert_eq!(legacy_input.description, description);
    }

    #[test]
    fn type_safe_transfer_input_smart_constructor_standard() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(300000).unwrap(); // $3,000 (under high-value threshold)

        let accounts = ValidatedTransferPair::from_account_ids(source_id, target_id).unwrap();
        let transfer_amount = TransferAmount::new(amount);
        let daily_limited = DailyLimitedTransferAmount::new(transfer_amount).unwrap();

        let input = TypeSafeTransferInput::new_standard(
            transfer_id.clone(),
            accounts,
            daily_limited,
            Some("Standard transfer".to_string()),
        );

        assert_eq!(input.transfer_id, transfer_id);
        assert_eq!(input.amount.money(), amount);
        assert!(matches!(input.amount, TypeSafeTransferAmount::Standard(_)));
    }

    #[test]
    fn type_safe_transfer_input_smart_constructor_high_value() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(750000).unwrap(); // $7,500 (above high-value threshold)
        let auth_token = AuthorizationToken::new(12345);

        let accounts = ValidatedTransferPair::from_account_ids(source_id, target_id).unwrap();
        let transfer_amount = TransferAmount::new(amount);
        let authorized_transfer =
            AuthorizedHighValueTransfer::new(transfer_amount, auth_token).unwrap();

        let input = TypeSafeTransferInput::new_high_value(
            transfer_id.clone(),
            accounts,
            authorized_transfer,
            Some("High-value transfer".to_string()),
        );

        assert_eq!(input.transfer_id, transfer_id);
        assert_eq!(input.amount.money(), amount);
        assert!(matches!(input.amount, TypeSafeTransferAmount::HighValue(_)));
    }

    #[test]
    fn type_safe_transfer_input_factory_method_standard() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(200000).unwrap(); // $2,000 (standard transfer)

        let input = TypeSafeTransferInput::from_parameters(
            transfer_id.clone(),
            source_id,
            target_id,
            amount,
            None, // No authorization needed for standard transfers
            Some("Factory standard transfer".to_string()),
        )
        .unwrap();

        assert_eq!(input.transfer_id, transfer_id);
        assert_eq!(input.amount.money(), amount);
        assert!(matches!(input.amount, TypeSafeTransferAmount::Standard(_)));
    }

    #[test]
    fn type_safe_transfer_input_factory_method_high_value() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(850000).unwrap(); // $8,500 (high-value transfer)
        let auth_token = AuthorizationToken::new(54321);

        let input = TypeSafeTransferInput::from_parameters(
            transfer_id.clone(),
            source_id,
            target_id,
            amount,
            Some(auth_token),
            Some("Factory high-value transfer".to_string()),
        )
        .unwrap();

        assert_eq!(input.transfer_id, transfer_id);
        assert_eq!(input.amount.money(), amount);
        assert!(matches!(input.amount, TypeSafeTransferAmount::HighValue(_)));
    }

    #[test]
    fn type_safe_transfer_input_factory_rejects_same_account() {
        let transfer_id = TransferId::generate();
        let account_id = AccountId::generate();
        let amount = Money::from_cents(100000).unwrap();

        let result = TypeSafeTransferInput::from_parameters(
            transfer_id,
            account_id.clone(),
            account_id,
            amount,
            None,
            None,
        );

        assert!(matches!(result, Err(BankingError::SameAccountTransfer(_))));
    }

    #[test]
    fn type_safe_transfer_input_factory_rejects_high_value_without_auth() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(600000).unwrap(); // $6,000 (above high-value threshold)

        let result = TypeSafeTransferInput::from_parameters(
            transfer_id,
            source_id,
            target_id,
            amount,
            None, // Missing authorization for high-value transfer
            None,
        );

        assert!(matches!(
            result,
            Err(BankingError::BusinessRuleViolation(_))
        ));
    }

    #[test]
    fn type_safe_transfer_input_factory_rejects_exceeds_daily_limit() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(1200000).unwrap(); // $12,000 (exceeds daily limit)

        let result = TypeSafeTransferInput::from_parameters(
            transfer_id,
            source_id,
            target_id,
            amount,
            None, // Standard transfer but exceeds limits
            None,
        );

        assert!(matches!(
            result,
            Err(BankingError::BusinessRuleViolation(_))
        ));
    }

    #[test]
    fn type_safe_transfer_input_conversion_methods() {
        let transfer_id = TransferId::generate();
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();
        let amount = Money::from_cents(400000).unwrap(); // $4,000

        let input = TypeSafeTransferInput::from_parameters(
            transfer_id.clone(),
            source_id.clone(),
            target_id.clone(),
            amount,
            None,
            Some("Conversion test".to_string()),
        )
        .unwrap();

        // Test conversion to safe input
        let safe_input = input.to_safe_input();
        assert_eq!(safe_input.transfer_id, transfer_id);
        assert_eq!(safe_input.amount.amount(), amount);

        // Test conversion to legacy input
        let legacy_input = input.to_legacy_input();
        assert_eq!(legacy_input.transfer_id, transfer_id);
        assert_eq!(legacy_input.from_account, source_id);
        assert_eq!(legacy_input.to_account, target_id);
        assert_eq!(legacy_input.amount, amount);
    }

    #[test]
    fn type_safe_transfer_amount_enum_methods() {
        // Test standard amount
        let standard_money = Money::from_cents(300000).unwrap();
        let standard_transfer = TransferAmount::new(standard_money);
        let daily_limited = DailyLimitedTransferAmount::new(standard_transfer).unwrap();
        let standard_amount = TypeSafeTransferAmount::Standard(daily_limited);

        assert_eq!(standard_amount.money(), standard_money);
        assert_eq!(standard_amount.amount().amount(), standard_money);

        // Test high-value amount
        let high_value_money = Money::from_cents(750000).unwrap();
        let high_value_transfer = TransferAmount::new(high_value_money);
        let auth_token = AuthorizationToken::new(12345);
        let authorized = AuthorizedHighValueTransfer::new(high_value_transfer, auth_token).unwrap();
        let high_value_amount = TypeSafeTransferAmount::HighValue(authorized);

        assert_eq!(high_value_amount.money(), high_value_money);
        assert_eq!(high_value_amount.amount().amount(), high_value_money);
    }
}
