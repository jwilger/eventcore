//! E-commerce domain types with type-safe validation
//!
//! This module defines all domain types for the e-commerce order example,
//! following type-driven development principles to make illegal states unrepresentable.

use nutype::nutype;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use uuid::Uuid;

/// Error types for e-commerce domain operations
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EcommerceError {
    /// Invalid product identifier format
    #[error("Invalid product ID: {0}")]
    InvalidProductId(String),
    /// Invalid order identifier format
    #[error("Invalid order ID: {0}")]
    InvalidOrderId(String),
    /// Invalid money amount
    #[error("Invalid money amount: {0}")]
    InvalidMoney(String),
    /// Invalid quantity value
    #[error("Invalid quantity: {0}")]
    InvalidQuantity(String),
    /// Product name validation error
    #[error("Invalid product name: {0}")]
    InvalidProductName(String),
    /// Customer email validation error
    #[error("Invalid customer email: {0}")]
    InvalidCustomerEmail(String),
    /// SKU validation error
    #[error("Invalid SKU: {0}")]
    InvalidSku(String),
}

// From trait implementations for nutype errors
impl From<ProductIdError> for EcommerceError {
    fn from(err: ProductIdError) -> Self {
        Self::InvalidProductId(err.to_string())
    }
}

impl From<OrderIdError> for EcommerceError {
    fn from(err: OrderIdError) -> Self {
        Self::InvalidOrderId(err.to_string())
    }
}

impl From<SkuError> for EcommerceError {
    fn from(err: SkuError) -> Self {
        Self::InvalidSku(err.to_string())
    }
}

impl From<ProductNameError> for EcommerceError {
    fn from(err: ProductNameError) -> Self {
        Self::InvalidProductName(err.to_string())
    }
}

impl From<CustomerEmailError> for EcommerceError {
    fn from(err: CustomerEmailError) -> Self {
        Self::InvalidCustomerEmail(err.to_string())
    }
}

/// Order identifier with validation
///
/// Format: ORD-{UPPERCASE_ALPHANUMERIC}
/// Example: ORD-A1B2C3D4
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50, regex = r"^ORD-[A-Z0-9]+$"),
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
pub struct OrderId(String);

impl OrderId {
    /// Generate a new order ID with a random UUID suffix
    pub fn generate() -> Self {
        let uuid = Uuid::now_v7().simple().to_string().to_uppercase();
        Self::try_new(format!("ORD-{}", &uuid[..8])).expect("Generated OrderId should be valid")
    }
}

/// Product identifier with validation
///
/// Format: PRD-{UPPERCASE_ALPHANUMERIC}
/// Example: PRD-LAPTOP01
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50, regex = r"^PRD-[A-Z0-9]+$"),
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
pub struct ProductId(String);

impl ProductId {
    /// Generate a new product ID with a random UUID suffix
    pub fn generate() -> Self {
        let uuid = Uuid::now_v7().simple().to_string().to_uppercase();
        Self::try_new(format!("PRD-{}", &uuid[..8])).expect("Generated ProductId should be valid")
    }
}

/// Stock keeping unit (SKU) with validation
///
/// Format: Alphanumeric with hyphens, max 20 characters
/// Example: LAPTOP-15-INCH-1TB
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 20, regex = r"^[A-Z0-9-]+$"),
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
pub struct Sku(String);

/// Product name with validation
///
/// Non-empty string with reasonable length limits
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 100),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Display,
        AsRef,
        Deref,
        Serialize,
        Deserialize,
        TryFrom
    )
)]
pub struct ProductName(String);

/// Customer email address with validation
///
/// Basic email format validation
#[nutype(
    sanitize(trim),
    validate(
        not_empty,
        len_char_max = 255,
        regex = r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"
    ),
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
pub struct CustomerEmail(String);

/// Product quantity with validation
///
/// Must be positive, maximum 1000 per order item
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Quantity(u32);

impl Quantity {
    /// Maximum quantity per order item
    pub const MAX_QUANTITY: u32 = 1000;

    /// Create a new quantity
    pub fn new(value: u32) -> Result<Self, EcommerceError> {
        if value == 0 {
            return Err(EcommerceError::InvalidQuantity(
                "Quantity must be greater than 0".to_string(),
            ));
        }
        if value > Self::MAX_QUANTITY {
            return Err(EcommerceError::InvalidQuantity(format!(
                "Quantity {} exceeds maximum {}",
                value,
                Self::MAX_QUANTITY
            )));
        }
        Ok(Self(value))
    }

    /// Create a quantity for inventory purposes (allows zero)
    pub fn for_inventory(value: u32) -> Result<Self, EcommerceError> {
        if value > Self::MAX_QUANTITY {
            return Err(EcommerceError::InvalidQuantity(format!(
                "Quantity {} exceeds maximum {}",
                value,
                Self::MAX_QUANTITY
            )));
        }
        Ok(Self(value))
    }

    /// Get the underlying value
    pub fn value(&self) -> u32 {
        self.0
    }

    /// Add quantities, checking for overflow
    pub fn checked_add(self, other: Self) -> Result<Self, EcommerceError> {
        let new_value = self
            .0
            .checked_add(other.0)
            .ok_or_else(|| EcommerceError::InvalidQuantity("Quantity overflow".to_string()))?;
        Self::new(new_value)
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Money amount with validation
///
/// Uses Decimal for precise financial calculations
/// Must be non-negative with max 2 decimal places
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Money(Decimal);

impl Money {
    /// Maximum money amount (100 million)
    pub const MAX_AMOUNT: Decimal = Decimal::from_parts(100_000_000, 0, 0, false, 0);

    /// Create money from cents (avoids floating point issues)
    pub fn from_cents(cents: u64) -> Result<Self, EcommerceError> {
        let decimal = Decimal::new(cents as i64, 2);
        Self::new(decimal)
    }

    /// Create money from decimal amount
    pub fn new(amount: Decimal) -> Result<Self, EcommerceError> {
        if amount.is_sign_negative() {
            return Err(EcommerceError::InvalidMoney(format!(
                "Money amount cannot be negative: {}",
                amount
            )));
        }
        if amount.scale() > 2 {
            return Err(EcommerceError::InvalidMoney(format!(
                "Money amount cannot have more than 2 decimal places: {}",
                amount
            )));
        }
        if amount > Self::MAX_AMOUNT {
            return Err(EcommerceError::InvalidMoney(format!(
                "Money amount {} exceeds maximum {}",
                amount,
                Self::MAX_AMOUNT
            )));
        }
        Ok(Self(amount))
    }

    /// Get the underlying decimal value
    pub fn amount(&self) -> Decimal {
        self.0
    }

    /// Convert to cents for storage
    pub fn to_cents(&self) -> u64 {
        (self.0 * Decimal::from(100)).to_u64().unwrap_or(0)
    }

    /// Add money amounts
    pub fn checked_add(self, other: Self) -> Result<Self, EcommerceError> {
        let new_amount = self.0 + other.0;
        Self::new(new_amount)
    }

    /// Subtract money amounts
    pub fn subtract(self, other: Self) -> Result<Self, EcommerceError> {
        if other.0 > self.0 {
            return Err(EcommerceError::InvalidMoney(
                "Cannot subtract larger amount from smaller amount".to_string(),
            ));
        }
        let new_amount = self.0 - other.0;
        Self::new(new_amount)
    }

    /// Multiply by quantity
    pub fn multiply_by_quantity(self, quantity: Quantity) -> Result<Self, EcommerceError> {
        let new_amount = self.0 * Decimal::from(quantity.value());
        Self::new(new_amount)
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${:.2}", self.0)
    }
}

impl Default for Money {
    fn default() -> Self {
        Self(Decimal::new(0, 0))
    }
}

impl std::str::FromStr for Money {
    type Err = EcommerceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let amount_str = trimmed
            .strip_prefix('$')
            .map_or(trimmed, |stripped| stripped);

        let decimal = amount_str.parse::<Decimal>().map_err(|e| {
            EcommerceError::InvalidMoney(format!("Failed to parse money amount '{}': {}", s, e))
        })?;

        Self::new(decimal)
    }
}

/// Product information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// Unique product identifier
    pub id: ProductId,
    /// Product SKU
    pub sku: Sku,
    /// Product name
    pub name: ProductName,
    /// Product price
    pub price: Money,
    /// Product description
    pub description: Option<String>,
}

impl Product {
    /// Create a new product
    pub fn new(
        id: ProductId,
        sku: Sku,
        name: ProductName,
        price: Money,
        description: Option<String>,
    ) -> Self {
        Self {
            id,
            sku,
            name,
            price,
            description,
        }
    }
}

/// Order item representing a product and quantity in an order
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    /// Product being ordered
    pub product_id: ProductId,
    /// Quantity of the product
    pub quantity: Quantity,
    /// Unit price at time of order
    pub unit_price: Money,
}

impl OrderItem {
    /// Create a new order item
    pub fn new(product_id: ProductId, quantity: Quantity, unit_price: Money) -> Self {
        Self {
            product_id,
            quantity,
            unit_price,
        }
    }

    /// Calculate total price for this item
    pub fn total_price(&self) -> Result<Money, EcommerceError> {
        self.unit_price.multiply_by_quantity(self.quantity)
    }
}

/// Customer information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Customer {
    /// Customer email address
    pub email: CustomerEmail,
    /// Customer full name
    pub name: String,
    /// Shipping address
    pub shipping_address: Option<String>,
}

impl Customer {
    /// Create a new customer
    pub fn new(email: CustomerEmail, name: String, shipping_address: Option<String>) -> Self {
        Self {
            email,
            name,
            shipping_address,
        }
    }
}

/// Order status enumeration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Order is being created, items can be added/removed
    Draft,
    /// Order has been placed and is being processed
    Placed,
    /// Order has been shipped
    Shipped,
    /// Order has been delivered
    Delivered,
    /// Order has been cancelled
    Cancelled,
}

impl Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "Draft"),
            Self::Placed => write!(f, "Placed"),
            Self::Shipped => write!(f, "Shipped"),
            Self::Delivered => write!(f, "Delivered"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_order_id_generation() {
        let id = OrderId::generate();
        assert!(id.as_ref().starts_with("ORD-"));
        assert!(id.as_ref().len() <= 50);
    }

    #[test]
    fn test_order_id_validation() {
        assert!(OrderId::try_new("ORD-ABC123".to_string()).is_ok());
        assert!(OrderId::try_new("ORD-".to_string()).is_err());
        assert!(OrderId::try_new("abc-123".to_string()).is_err());
        assert!(OrderId::try_new("ORD-abc".to_string()).is_err()); // lowercase not allowed
    }

    #[test]
    fn test_product_id_generation() {
        let id = ProductId::generate();
        assert!(id.as_ref().starts_with("PRD-"));
        assert!(id.as_ref().len() <= 50);
    }

    #[test]
    fn test_product_id_validation() {
        assert!(ProductId::try_new("PRD-LAPTOP01".to_string()).is_ok());
        assert!(ProductId::try_new("PRD-".to_string()).is_err());
        assert!(ProductId::try_new("prd-laptop".to_string()).is_err());
    }

    #[test]
    fn test_sku_validation() {
        assert!(Sku::try_new("LAPTOP-15-INCH".to_string()).is_ok());
        assert!(Sku::try_new("SKU123".to_string()).is_ok());
        assert!(Sku::try_new("".to_string()).is_err());
        assert!(Sku::try_new("sku-123".to_string()).is_err()); // lowercase not allowed
        assert!(Sku::try_new("a".repeat(21)).is_err()); // too long
    }

    #[test]
    fn test_quantity_validation() {
        assert!(Quantity::new(1).is_ok());
        assert!(Quantity::new(1000).is_ok());
        assert!(Quantity::new(0).is_err());
        assert!(Quantity::new(1001).is_err());
    }

    #[test]
    fn test_quantity_operations() {
        let q1 = Quantity::new(5).unwrap();
        let q2 = Quantity::new(3).unwrap();
        let sum = q1.checked_add(q2).unwrap();
        assert_eq!(sum.value(), 8);

        let max_q = Quantity::new(1000).unwrap();
        assert!(max_q.checked_add(Quantity::new(1).unwrap()).is_err());
    }

    #[test]
    fn test_money_validation() {
        assert!(Money::from_cents(100).is_ok()); // $1.00
        assert!(Money::new(Decimal::new(1050, 2)).is_ok()); // $10.50

        // Negative amount should fail
        assert!(Money::new(Decimal::new(-100, 2)).is_err());

        // Too many decimal places should fail
        assert!(Money::new(Decimal::new(1001, 3)).is_err());
    }

    #[test]
    fn test_money_operations() {
        let m1 = Money::from_cents(100).unwrap(); // $1.00
        let m2 = Money::from_cents(250).unwrap(); // $2.50

        let sum = m1.checked_add(m2).unwrap();
        assert_eq!(sum.to_cents(), 350); // $3.50

        let diff = m2.subtract(m1).unwrap();
        assert_eq!(diff.to_cents(), 150); // $1.50

        // Cannot subtract larger from smaller
        assert!(m1.subtract(m2).is_err());

        let qty = Quantity::new(3).unwrap();
        let total = m1.multiply_by_quantity(qty).unwrap();
        assert_eq!(total.to_cents(), 300); // $3.00
    }

    #[test]
    fn test_money_parsing() {
        assert_eq!("$10.50".parse::<Money>().unwrap().to_cents(), 1050);
        assert_eq!("25.99".parse::<Money>().unwrap().to_cents(), 2599);
        assert!("invalid".parse::<Money>().is_err());
        assert!("-5.00".parse::<Money>().is_err());
    }

    #[test]
    fn test_order_item_total_price() {
        let product_id = ProductId::try_new("PRD-LAPTOP01".to_string()).unwrap();
        let quantity = Quantity::new(2).unwrap();
        let unit_price = Money::from_cents(99999).unwrap(); // $999.99

        let item = OrderItem::new(product_id, quantity, unit_price);
        let total = item.total_price().unwrap();
        assert_eq!(total.to_cents(), 199_998); // $1999.98
    }

    #[test]
    fn test_customer_email_validation() {
        assert!(CustomerEmail::try_new("user@example.com".to_string()).is_ok());
        assert!(CustomerEmail::try_new("test.email+tag@domain.co.uk".to_string()).is_ok());
        assert!(CustomerEmail::try_new("invalid-email".to_string()).is_err());
        assert!(CustomerEmail::try_new("@domain.com".to_string()).is_err());
        assert!(CustomerEmail::try_new("user@".to_string()).is_err());
    }

    // Property-based tests
    proptest! {
        #[test]
        fn prop_money_from_cents_roundtrip(cents in 0u64..1_000_000) {
            let money = Money::from_cents(cents).unwrap();
            assert_eq!(money.to_cents(), cents);
        }

        #[test]
        fn prop_quantity_value_roundtrip(value in 1u32..=1000) {
            let quantity = Quantity::new(value).unwrap();
            assert_eq!(quantity.value(), value);
        }

        #[test]
        fn prop_money_addition_associative(
            a in 0u64..100_000,
            b in 0u64..100_000,
            c in 0u64..100_000
        ) {
            let ma = Money::from_cents(a).unwrap();
            let mb = Money::from_cents(b).unwrap();
            let mc = Money::from_cents(c).unwrap();

            if let (Ok(ab), Ok(bc)) = (ma.checked_add(mb), mb.checked_add(mc)) {
                if let (Ok(ab_c), Ok(a_bc)) = (ab.checked_add(mc), ma.checked_add(bc)) {
                    assert_eq!(ab_c, a_bc);
                }
            }
        }

        #[test]
        fn prop_quantity_addition_commutative(a in 1u32..=500, b in 1u32..=500) {
            let qa = Quantity::new(a).unwrap();
            let qb = Quantity::new(b).unwrap();

            if let (Ok(ab), Ok(ba)) = (qa.checked_add(qb), qb.checked_add(qa)) {
                assert_eq!(ab, ba);
            }
        }
    }
}
