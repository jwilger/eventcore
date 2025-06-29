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
    }
}
