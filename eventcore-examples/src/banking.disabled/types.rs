//! Type-safe domain types for the banking example
//!
//! This module demonstrates how to create domain types that encode business rules
//! in the type system, making illegal states unrepresentable.

use nutype::nutype;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur when working with Money
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// The amount is negative, which is not allowed
    #[error("Money amount cannot be negative: {0}")]
    NegativeAmount(Decimal),

    /// The amount has too many decimal places
    #[error("Money can only have up to 2 decimal places, got: {0}")]
    TooManyDecimalPlaces(Decimal),

    /// The amount exceeds the maximum allowed value
    #[error("Money amount {0} exceeds maximum allowed value of {1}")]
    ExceedsMaximum(Decimal, Decimal),

    /// Failed to parse money from string
    #[error("Failed to parse money from string: {0}")]
    ParseError(String),
}

/// Maximum amount of money that can be represented (1 trillion)
pub const MAX_MONEY_AMOUNT: Decimal = dec!(1_000_000_000_000.00);

/// Represents a monetary amount with proper validation
///
/// Money is always non-negative and has at most 2 decimal places.
/// This type ensures these invariants at construction time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Money(Decimal);

impl Money {
    /// Creates a new Money instance from a Decimal
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount is negative
    /// - The amount has more than 2 decimal places
    /// - The amount exceeds the maximum allowed value
    pub fn new(amount: Decimal) -> Result<Self, MoneyError> {
        // Check if negative
        if amount.is_sign_negative() {
            return Err(MoneyError::NegativeAmount(amount));
        }

        // Check decimal places
        if amount.scale() > 2 {
            return Err(MoneyError::TooManyDecimalPlaces(amount));
        }

        // Check maximum value
        if amount > MAX_MONEY_AMOUNT {
            return Err(MoneyError::ExceedsMaximum(amount, MAX_MONEY_AMOUNT));
        }

        Ok(Self(amount))
    }

    /// Creates Money from cents (e.g., 1234 = $12.34)
    pub fn from_cents(cents: u64) -> Result<Self, MoneyError> {
        let amount = Decimal::from(cents) / dec!(100);
        Self::new(amount)
    }

    /// Returns the amount as a Decimal
    pub fn amount(&self) -> Decimal {
        self.0
    }

    /// Returns the amount in cents
    pub fn to_cents(&self) -> u64 {
        (self.0 * dec!(100)).to_u64().unwrap_or(0)
    }

    /// Adds two Money values
    pub fn add(&self, other: &Self) -> Result<Self, MoneyError> {
        Self::new(self.0 + other.0)
    }

    /// Subtracts another Money value from this one
    pub fn subtract(&self, other: &Self) -> Result<Self, MoneyError> {
        if other.0 > self.0 {
            return Err(MoneyError::NegativeAmount(self.0 - other.0));
        }
        Self::new(self.0 - other.0)
    }

    /// Zero money value
    pub fn zero() -> Self {
        Self(dec!(0))
    }
}

impl Default for Money {
    fn default() -> Self {
        Self::zero()
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${}", self.0)
    }
}

impl FromStr for Money {
    type Err = MoneyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Trim whitespace first, then remove $ sign if present
        let s = s.trim();
        let s = s.trim_start_matches('$').trim();

        let amount = Decimal::from_str(s).map_err(|_| MoneyError::ParseError(s.to_string()))?;

        Self::new(amount)
    }
}

/// Account identifier with validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50, regex = r"^ACC-[A-Z0-9]+$"),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Display,
        AsRef,
        Deref,
        Serialize,
        Deserialize,
        TryFrom
    )
)]
pub struct AccountId(String);

impl AccountId {
    /// Generates a new unique `AccountId`
    pub fn generate() -> Self {
        let uuid = Uuid::now_v7();
        Self::try_new(format!("ACC-{}", uuid.simple().to_string().to_uppercase())).unwrap()
    }
}

/// Transfer identifier with validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50, regex = r"^TXF-[A-Z0-9]+$"),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Display,
        AsRef,
        Deref,
        Serialize,
        Deserialize,
        TryFrom
    )
)]
pub struct TransferId(String);

impl TransferId {
    /// Generates a new unique `TransferId`
    pub fn generate() -> Self {
        let uuid = Uuid::now_v7();
        Self::try_new(format!("TXF-{}", uuid.simple().to_string().to_uppercase())).unwrap()
    }
}

/// Customer name with validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_min = 2, len_char_max = 100),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Display,
        AsRef,
        Deref,
        Serialize,
        Deserialize
    )
)]
pub struct CustomerName(String);

/// Account holder information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountHolder {
    /// Name of the account holder
    pub name: CustomerName,
    /// Email address (could be a validated type in production)
    pub email: String,
}

/// Branded types for command inputs - prevent misuse of account IDs in different contexts
///
/// These types use the newtype pattern to create distinct types for different roles
/// of AccountId in command inputs, making it impossible to confuse source and target
/// accounts at compile time.
///
/// Source account in a transfer operation
///
/// This branded type ensures that an account ID intended as a source cannot be
/// accidentally used as a target, eliminating a class of runtime errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceAccount(AccountId);

impl SourceAccount {
    /// Creates a new source account wrapper
    pub fn new(account_id: AccountId) -> Self {
        Self(account_id)
    }

    /// Extracts the underlying AccountId
    pub fn into_account_id(self) -> AccountId {
        self.0
    }

    /// Gets a reference to the underlying AccountId
    pub fn account_id(&self) -> &AccountId {
        &self.0
    }
}

impl std::fmt::Display for SourceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source:{}", self.0)
    }
}

/// Target account in a transfer operation
///
/// This branded type ensures that an account ID intended as a target cannot be
/// accidentally used as a source, eliminating a class of runtime errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetAccount(AccountId);

impl TargetAccount {
    /// Creates a new target account wrapper
    pub fn new(account_id: AccountId) -> Self {
        Self(account_id)
    }

    /// Extracts the underlying AccountId
    pub fn into_account_id(self) -> AccountId {
        self.0
    }

    /// Gets a reference to the underlying AccountId
    pub fn account_id(&self) -> &AccountId {
        &self.0
    }
}

impl std::fmt::Display for TargetAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "target:{}", self.0)
    }
}

/// Phantom type marker for different money contexts
///
/// This enables compile-time guarantees about money usage in different business contexts.
pub mod money_context {
    /// Marker for money being deposited
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Deposit;
    /// Marker for money being withdrawn
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Withdrawal;
    /// Marker for money being transferred
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Transfer;
}

/// Context-aware money type that encodes business rules at compile time
///
/// This prevents mixing up different types of monetary operations and ensures
/// that money amounts are used in the correct business context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContextualMoney<Context> {
    amount: Money,
    #[serde(skip)]
    _context: std::marker::PhantomData<Context>,
}

impl<Context> ContextualMoney<Context> {
    /// Creates a new contextual money amount
    pub fn new(amount: Money) -> Self {
        Self {
            amount,
            _context: std::marker::PhantomData,
        }
    }

    /// Extracts the underlying Money value
    pub fn amount(&self) -> Money {
        self.amount
    }

    /// Converts to a different context (when business rules allow)
    pub fn into_context<NewContext>(self) -> ContextualMoney<NewContext> {
        ContextualMoney::new(self.amount)
    }
}

impl<Context> std::fmt::Display for ContextualMoney<Context> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.amount)
    }
}

/// Type aliases for commonly used contextual money types
///
/// Money amount specifically for deposits
pub type DepositAmount = ContextualMoney<money_context::Deposit>;

/// Money amount specifically for withdrawals  
pub type WithdrawalAmount = ContextualMoney<money_context::Withdrawal>;

/// Money amount specifically for transfers
pub type TransferAmount = ContextualMoney<money_context::Transfer>;

/// Compile-time validated transfer pairs
///
/// This type enforces at compile time that source and target accounts are different
/// by preventing the creation of transfers where both accounts have the same underlying ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedTransferPair {
    source: SourceAccount,
    target: TargetAccount,
}

impl ValidatedTransferPair {
    /// Creates a validated transfer pair, ensuring accounts are different
    ///
    /// This method performs the validation once at construction time, after which
    /// the type system guarantees the business rule holds.
    pub fn new(source: SourceAccount, target: TargetAccount) -> Result<Self, MoneyError> {
        if source.account_id() == target.account_id() {
            return Err(MoneyError::ParseError(
                "Source and target accounts cannot be the same".to_string(),
            ));
        }

        Ok(Self { source, target })
    }

    /// Creates a validated pair from raw account IDs
    pub fn from_account_ids(
        source_id: AccountId,
        target_id: AccountId,
    ) -> Result<Self, MoneyError> {
        Self::new(SourceAccount::new(source_id), TargetAccount::new(target_id))
    }

    /// Gets the source account
    pub fn source(&self) -> &SourceAccount {
        &self.source
    }

    /// Gets the target account  
    pub fn target(&self) -> &TargetAccount {
        &self.target
    }

    /// Destructures into source and target accounts
    pub fn into_accounts(self) -> (SourceAccount, TargetAccount) {
        (self.source, self.target)
    }
}

/// Business rule: Transfer amounts must be within daily limits
///
/// This type encodes daily transfer limits in the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DailyLimitedTransferAmount {
    amount: TransferAmount,
}

impl DailyLimitedTransferAmount {
    /// Daily transfer limit (configurable in production)
    pub const DAILY_LIMIT: Money = Money(dec!(10000.00)); // $10,000

    /// Creates a transfer amount that respects daily limits
    pub fn new(amount: TransferAmount) -> Result<Self, MoneyError> {
        if amount.amount() > Self::DAILY_LIMIT {
            return Err(MoneyError::ExceedsMaximum(
                amount.amount().amount(),
                Self::DAILY_LIMIT.amount(),
            ));
        }

        Ok(Self { amount })
    }

    /// Gets the underlying transfer amount
    pub fn amount(&self) -> TransferAmount {
        self.amount
    }
}

/// High-value transfer that requires additional authorization
///
/// This type represents transfers above a certain threshold that have been
/// pre-authorized through additional verification steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedHighValueTransfer {
    amount: TransferAmount,
    authorization_token: AuthorizationToken,
}

impl AuthorizedHighValueTransfer {
    /// Threshold for high-value transfers
    pub const HIGH_VALUE_THRESHOLD: Money = Money(dec!(5000.00)); // $5,000

    /// Creates an authorized high-value transfer
    pub fn new(
        amount: TransferAmount,
        authorization_token: AuthorizationToken,
    ) -> Result<Self, MoneyError> {
        if amount.amount() <= Self::HIGH_VALUE_THRESHOLD {
            return Err(MoneyError::ParseError(format!(
                "Amount {} is not high-value (threshold: {})",
                amount.amount(),
                Self::HIGH_VALUE_THRESHOLD
            )));
        }

        Ok(Self {
            amount,
            authorization_token,
        })
    }

    /// Gets the transfer amount
    pub fn amount(&self) -> TransferAmount {
        self.amount
    }

    /// Gets the authorization token
    pub fn authorization_token(&self) -> AuthorizationToken {
        self.authorization_token
    }
}

/// Authorization token for high-value transfers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationToken {
    token: u64,
}

impl AuthorizationToken {
    /// Creates a new authorization token (would be issued by authorization service)
    pub fn new(token: u64) -> Self {
        Self { token }
    }

    /// Gets the token value
    pub fn value(&self) -> u64 {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn money_from_valid_decimal() {
        let money = Money::new(dec!(100.50)).unwrap();
        assert_eq!(money.amount(), dec!(100.50));
        assert_eq!(money.to_cents(), 10050);
    }

    #[test]
    fn money_rejects_negative() {
        let result = Money::new(dec!(-10.00));
        assert!(matches!(result, Err(MoneyError::NegativeAmount(_))));
    }

    #[test]
    fn money_rejects_too_many_decimals() {
        let result = Money::new(dec!(10.001));
        assert!(matches!(result, Err(MoneyError::TooManyDecimalPlaces(_))));
    }

    #[test]
    fn money_rejects_exceeds_maximum() {
        let result = Money::new(MAX_MONEY_AMOUNT + dec!(1));
        assert!(matches!(result, Err(MoneyError::ExceedsMaximum(_, _))));
    }

    #[test]
    fn money_from_cents() {
        let money = Money::from_cents(1234).unwrap();
        assert_eq!(money.amount(), dec!(12.34));
        assert_eq!(money.to_cents(), 1234);
    }

    #[test]
    fn money_add() {
        let a = Money::new(dec!(10.50)).unwrap();
        let b = Money::new(dec!(5.25)).unwrap();
        let result = a.add(&b).unwrap();
        assert_eq!(result.amount(), dec!(15.75));
    }

    #[test]
    fn money_subtract_valid() {
        let a = Money::new(dec!(10.50)).unwrap();
        let b = Money::new(dec!(5.25)).unwrap();
        let result = a.subtract(&b).unwrap();
        assert_eq!(result.amount(), dec!(5.25));
    }

    #[test]
    fn money_subtract_would_be_negative() {
        let a = Money::new(dec!(5.00)).unwrap();
        let b = Money::new(dec!(10.00)).unwrap();
        let result = a.subtract(&b);
        assert!(matches!(result, Err(MoneyError::NegativeAmount(_))));
    }

    #[test]
    fn money_from_string() {
        assert_eq!(Money::from_str("100.50").unwrap().amount(), dec!(100.50));
        assert_eq!(Money::from_str("$100.50").unwrap().amount(), dec!(100.50));
        assert_eq!(Money::from_str(" $100.50 ").unwrap().amount(), dec!(100.50));
    }

    #[test]
    fn account_id_generate() {
        let id = AccountId::generate();
        assert!(id.starts_with("ACC-"));
        assert!(id.len() > 4);
    }

    #[test]
    fn transfer_id_generate() {
        let id = TransferId::generate();
        assert!(id.starts_with("TXF-"));
        assert!(id.len() > 4);
    }

    #[test]
    fn source_account_branded_type() {
        let account_id = AccountId::generate();
        let source = SourceAccount::new(account_id.clone());

        assert_eq!(source.account_id(), &account_id);
        assert_eq!(source.into_account_id(), account_id);
    }

    #[test]
    fn target_account_branded_type() {
        let account_id = AccountId::generate();
        let target = TargetAccount::new(account_id.clone());

        assert_eq!(target.account_id(), &account_id);
        assert_eq!(target.into_account_id(), account_id);
    }

    #[test]
    fn contextual_money_types() {
        let money = Money::from_cents(1000).unwrap();

        let deposit = DepositAmount::new(money);
        let withdrawal = WithdrawalAmount::new(money);
        let transfer = TransferAmount::new(money);

        assert_eq!(deposit.amount(), money);
        assert_eq!(withdrawal.amount(), money);
        assert_eq!(transfer.amount(), money);

        // Test context conversion
        let converted: TransferAmount = deposit.into_context();
        assert_eq!(converted.amount(), money);
    }

    #[test]
    fn branded_types_display() {
        let account_id = AccountId::generate();
        let source = SourceAccount::new(account_id.clone());
        let target = TargetAccount::new(account_id);

        let source_display = source.to_string();
        let target_display = target.to_string();

        assert!(source_display.starts_with("source:"));
        assert!(target_display.starts_with("target:"));
    }

    #[test]
    fn validated_transfer_pair_prevents_same_account() {
        let account_id = AccountId::generate();
        let source = SourceAccount::new(account_id.clone());
        let target = TargetAccount::new(account_id);

        let result = ValidatedTransferPair::new(source, target);
        assert!(result.is_err());
    }

    #[test]
    fn validated_transfer_pair_allows_different_accounts() {
        let source_id = AccountId::generate();
        let target_id = AccountId::generate();

        let pair =
            ValidatedTransferPair::from_account_ids(source_id.clone(), target_id.clone()).unwrap();

        assert_eq!(pair.source().account_id(), &source_id);
        assert_eq!(pair.target().account_id(), &target_id);

        let (source, target) = pair.into_accounts();
        assert_eq!(source.account_id(), &source_id);
        assert_eq!(target.account_id(), &target_id);
    }

    #[test]
    fn daily_limited_transfer_amount_within_limit() {
        let amount = TransferAmount::new(Money::from_cents(500000).unwrap()); // $5,000
        let limited = DailyLimitedTransferAmount::new(amount).unwrap();

        assert_eq!(limited.amount(), amount);
    }

    #[test]
    fn daily_limited_transfer_amount_exceeds_limit() {
        let amount = TransferAmount::new(Money::from_cents(1500000).unwrap()); // $15,000
        let result = DailyLimitedTransferAmount::new(amount);

        assert!(result.is_err());
    }

    #[test]
    fn authorized_high_value_transfer_valid() {
        let amount = TransferAmount::new(Money::from_cents(750000).unwrap()); // $7,500
        let token = AuthorizationToken::new(12345);

        let authorized = AuthorizedHighValueTransfer::new(amount, token).unwrap();

        assert_eq!(authorized.amount(), amount);
        assert_eq!(authorized.authorization_token().value(), 12345);
    }

    #[test]
    fn authorized_high_value_transfer_below_threshold() {
        let amount = TransferAmount::new(Money::from_cents(300000).unwrap()); // $3,000
        let token = AuthorizationToken::new(12345);

        let result = AuthorizedHighValueTransfer::new(amount, token);
        assert!(result.is_err());
    }

    proptest! {
        #[test]
        fn money_valid_amounts(cents in 0u64..100_000_000_000_000u64) {
            if cents <= MAX_MONEY_AMOUNT.to_u64().unwrap() * 100 {
                let money = Money::from_cents(cents).unwrap();
                assert_eq!(money.to_cents(), cents);
            }
        }

        #[test]
        fn money_roundtrip_serialization(cents in 0u64..10_000_000u64) {
            let money = Money::from_cents(cents).unwrap();
            let json = serde_json::to_string(&money).unwrap();
            let deserialized: Money = serde_json::from_str(&json).unwrap();
            assert_eq!(money, deserialized);
        }

        #[test]
        fn branded_types_preserve_account_id_invariants(suffix in "[A-Z0-9]{8,20}") {
            let account_id = format!("ACC-{}", suffix);
            if let Ok(account_id) = AccountId::try_new(account_id) {
                let source = SourceAccount::new(account_id.clone());
                let target = TargetAccount::new(account_id.clone());

                // Source and target preserve the underlying account ID
                prop_assert_eq!(source.account_id(), &account_id);
                prop_assert_eq!(target.account_id(), &account_id);

                // Roundtrip conversions work correctly
                let account_id_copy = account_id.clone();
                prop_assert_eq!(source.clone().into_account_id(), account_id);
                prop_assert_eq!(target.clone().into_account_id(), account_id_copy);

                // Display format includes branding
                prop_assert!(source.to_string().starts_with("source:"));
                prop_assert!(target.to_string().starts_with("target:"));
            }
        }

        #[test]
        fn contextual_money_preserves_amount_invariants(cents in 0u64..1_000_000u64) {
            let money = Money::from_cents(cents).unwrap();

            let deposit = DepositAmount::new(money);
            let withdrawal = WithdrawalAmount::new(money);
            let transfer = TransferAmount::new(money);

            // All contexts preserve the underlying money amount
            prop_assert_eq!(deposit.amount(), money);
            prop_assert_eq!(withdrawal.amount(), money);
            prop_assert_eq!(transfer.amount(), money);

            // Context conversions preserve amount
            let converted_transfer: TransferAmount = deposit.into_context();
            prop_assert_eq!(converted_transfer.amount(), money);

            let converted_deposit: DepositAmount = transfer.into_context();
            prop_assert_eq!(converted_deposit.amount(), money);
        }

        #[test]
        fn validated_transfer_pair_enforces_different_accounts(
            suffix1 in "[A-Z0-9]{8,20}",
            suffix2 in "[A-Z0-9]{8,20}"
        ) {
            let account1 = format!("ACC-{}", suffix1);
            let account2 = format!("ACC-{}", suffix2);
            if let (Ok(acc1), Ok(acc2)) = (AccountId::try_new(account1), AccountId::try_new(account2)) {
                let result = ValidatedTransferPair::from_account_ids(acc1.clone(), acc2.clone());

                if acc1 == acc2 {
                    // Same accounts should be rejected
                    prop_assert!(result.is_err());
                } else {
                    // Different accounts should be accepted
                    prop_assert!(result.is_ok());
                    if let Ok(pair) = result {
                        prop_assert_eq!(pair.source().account_id(), &acc1);
                        prop_assert_eq!(pair.target().account_id(), &acc2);

                        // Destructuring preserves accounts
                        let (source, target) = pair.into_accounts();
                        prop_assert_eq!(source.account_id(), &acc1);
                        prop_assert_eq!(target.account_id(), &acc2);
                    }
                }
            }
        }

        #[test]
        fn daily_limited_transfer_amount_enforces_limits(cents in 0u64..2_000_000u64) {
            let money = Money::from_cents(cents).unwrap();
            let transfer_amount = TransferAmount::new(money);
            let result = DailyLimitedTransferAmount::new(transfer_amount);

            if money <= DailyLimitedTransferAmount::DAILY_LIMIT {
                // Within limits should succeed
                prop_assert!(result.is_ok());
                if let Ok(limited) = result {
                    prop_assert_eq!(limited.amount().amount(), money);
                }
            } else {
                // Exceeding limits should fail
                prop_assert!(result.is_err());
            }
        }

        #[test]
        fn authorized_high_value_transfer_enforces_threshold(cents in 500_000u64..1_000_000u64, token in any::<u64>()) {
            let money = Money::from_cents(cents).unwrap();
            let transfer_amount = TransferAmount::new(money);
            let auth_token = AuthorizationToken::new(token);
            let result = AuthorizedHighValueTransfer::new(transfer_amount, auth_token);

            if money > AuthorizedHighValueTransfer::HIGH_VALUE_THRESHOLD {
                // Above threshold should succeed with authorization
                prop_assert!(result.is_ok());
                if let Ok(authorized) = result {
                    prop_assert_eq!(authorized.amount().amount(), money);
                    prop_assert_eq!(authorized.authorization_token().value(), token);
                }
            } else {
                // Below threshold should fail (not high-value)
                prop_assert!(result.is_err());
            }
        }

        #[test]
        fn type_safe_transfer_amount_enum_preserves_invariants(cents in 0u64..2_000_000u64, token in any::<u64>()) {
            let money = Money::from_cents(cents).unwrap();
            let transfer_amount = TransferAmount::new(money);

            if money <= DailyLimitedTransferAmount::DAILY_LIMIT {
                if let Ok(limited) = DailyLimitedTransferAmount::new(transfer_amount) {
                    let enum_amount = crate::banking::commands::TypeSafeTransferAmount::Standard(limited);
                    prop_assert_eq!(enum_amount.money(), money);
                    prop_assert_eq!(enum_amount.amount().amount(), money);
                }
            }

            if money > AuthorizedHighValueTransfer::HIGH_VALUE_THRESHOLD {
                let auth_token = AuthorizationToken::new(token);
                if let Ok(authorized) = AuthorizedHighValueTransfer::new(transfer_amount, auth_token) {
                    let enum_amount = crate::banking::commands::TypeSafeTransferAmount::HighValue(authorized);
                    prop_assert_eq!(enum_amount.money(), money);
                    prop_assert_eq!(enum_amount.amount().amount(), money);
                }
            }
        }
    }
}
