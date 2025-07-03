//! Comprehensive tests for banking domain types
//!
//! This module contains property-based tests and validation tests to ensure
//! that our domain types properly enforce business rules.

#[cfg(test)]
mod property_tests {
    use crate::banking::types::*;
    use proptest::prelude::*;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // Property test generators
    prop_compose! {
        /// Generate valid money amounts in cents
        fn valid_cents()(cents in 0u64..=100_000_000_000_000u64) -> u64 {
            cents
        }
    }

    prop_compose! {
        /// Generate valid decimal amounts for Money
        fn valid_decimal_amount()(
            dollars in 0u64..=999_999_999_999u64,
            cents in 0u8..=99u8
        ) -> Decimal {
            Decimal::from_str(&format!("{dollars}.{cents:02}")).unwrap()
        }
    }

    prop_compose! {
        /// Generate valid account ID strings
        fn valid_account_id_string()(s in r"ACC-[A-Z0-9]{8,32}") -> String {
            s
        }
    }

    prop_compose! {
        /// Generate valid transfer ID strings
        fn valid_transfer_id_string()(s in r"TXF-[A-Z0-9]{8,32}") -> String {
            s
        }
    }

    prop_compose! {
        /// Generate valid customer names
        fn valid_customer_name()(s in r"[A-Za-z][A-Za-z\s'-]{1,98}[A-Za-z]") -> String {
            s.trim().to_string()
        }
    }

    proptest! {
        #[test]
        fn money_from_valid_cents_always_succeeds(cents in valid_cents()) {
            if cents <= (MAX_MONEY_AMOUNT * rust_decimal_macros::dec!(100)).to_u64().unwrap() {
                let money = Money::from_cents(cents).unwrap();
                prop_assert_eq!(money.to_cents(), cents);
            }
        }

        #[test]
        fn money_from_valid_decimal_always_succeeds(amount in valid_decimal_amount()) {
            let money = Money::new(amount).unwrap();
            prop_assert_eq!(money.amount(), amount);
        }

        #[test]
        fn money_serialization_roundtrip(cents in valid_cents()) {
            if cents <= (MAX_MONEY_AMOUNT * rust_decimal_macros::dec!(100)).to_u64().unwrap() {
                let original = Money::from_cents(cents).unwrap();
                let json = serde_json::to_string(&original).unwrap();
                let deserialized: Money = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(original, deserialized);
            }
        }

        #[test]
        fn money_addition_is_commutative(
            cents1 in 0u64..=50_000_000_000_000u64,
            cents2 in 0u64..=50_000_000_000_000u64
        ) {
            let a = Money::from_cents(cents1).unwrap();
            let b = Money::from_cents(cents2).unwrap();

            let sum1 = a.add(&b);
            let sum2 = b.add(&a);

            match (sum1, sum2) {
                (Ok(s1), Ok(s2)) => prop_assert_eq!(s1, s2),
                (Err(_), Err(_)) => {}, // Both failed, which is consistent
                _ => prop_assert!(false, "Addition should be commutative"),
            }
        }

        #[test]
        fn money_subtraction_preserves_non_negative_invariant(
            cents1 in valid_cents(),
            cents2 in valid_cents()
        ) {
            if let (Ok(a), Ok(b)) = (Money::from_cents(cents1), Money::from_cents(cents2)) {
                match a.subtract(&b) {
                    Ok(result) => {
                        prop_assert!(result.amount() >= rust_decimal_macros::dec!(0));
                    }
                    Err(MoneyError::NegativeAmount(_)) => {
                        prop_assert!(b.amount() > a.amount());
                    }
                    Err(e) => prop_assert!(false, "Unexpected error: {:?}", e),
                }
            }
        }

        #[test]
        fn account_id_from_valid_string_succeeds(s in valid_account_id_string()) {
            let account_id = AccountId::try_new(s.clone());
            prop_assert!(account_id.is_ok());
            let account_id_val = account_id.unwrap();
            prop_assert_eq!(account_id_val.as_ref(), &s);
        }

        #[test]
        fn transfer_id_from_valid_string_succeeds(s in valid_transfer_id_string()) {
            let transfer_id = TransferId::try_new(s.clone());
            prop_assert!(transfer_id.is_ok());
            let transfer_id_val = transfer_id.unwrap();
            prop_assert_eq!(transfer_id_val.as_ref(), &s);
        }

        #[test]
        fn customer_name_from_valid_string_succeeds(s in valid_customer_name()) {
            if s.len() >= 2 && s.len() <= 100 {
                let name = CustomerName::try_new(s);
                prop_assert!(name.is_ok());
            }
        }

        #[test]
        fn generated_account_ids_are_unique(
            _x in 0u8..10u8 // Just to run the test multiple times
        ) {
            let id1 = AccountId::generate();
            let id2 = AccountId::generate();
            prop_assert_ne!(id1, id2);
        }

        #[test]
        fn generated_transfer_ids_are_unique(
            _x in 0u8..10u8 // Just to run the test multiple times
        ) {
            let id1 = TransferId::generate();
            let id2 = TransferId::generate();
            prop_assert_ne!(id1, id2);
        }
    }
}

#[cfg(test)]
mod validation_tests {
    use crate::banking::types::*;
    use rust_decimal_macros::dec;

    #[test]
    fn money_rejects_negative_amounts() {
        assert!(Money::new(dec!(-0.01)).is_err());
        assert!(Money::new(dec!(-100.00)).is_err());
        assert!(Money::new(dec!(-999999.99)).is_err());
    }

    #[test]
    fn money_rejects_more_than_two_decimals() {
        assert!(Money::new(dec!(10.001)).is_err());
        assert!(Money::new(dec!(10.999)).is_err());
        assert!(Money::new(dec!(0.12345)).is_err());
    }

    #[test]
    fn money_rejects_amounts_exceeding_maximum() {
        assert!(Money::new(MAX_MONEY_AMOUNT).is_ok());
        assert!(Money::new(MAX_MONEY_AMOUNT + dec!(0.01)).is_err());
        assert!(Money::new(MAX_MONEY_AMOUNT + dec!(1000000)).is_err());
    }

    #[test]
    fn money_from_string_parsing() {
        // Valid cases
        assert!("100".parse::<Money>().is_ok());
        assert!("100.00".parse::<Money>().is_ok());
        assert!("100.50".parse::<Money>().is_ok());
        assert!("$100.50".parse::<Money>().is_ok());
        assert!(" $100.50 ".parse::<Money>().is_ok());

        // Invalid cases
        assert!("-100".parse::<Money>().is_err());
        assert!("100.001".parse::<Money>().is_err());
        assert!("abc".parse::<Money>().is_err());
        assert!("".parse::<Money>().is_err());
    }

    #[test]
    fn account_id_validation() {
        // Valid cases
        assert!(AccountId::try_new("ACC-12345678".to_string()).is_ok());
        assert!(AccountId::try_new("ACC-ABCDEF123".to_string()).is_ok());

        // Invalid cases - empty
        assert!(AccountId::try_new(String::new()).is_err());
        assert!(AccountId::try_new("   ".to_string()).is_err());

        // Invalid cases - wrong format
        assert!(AccountId::try_new("acc-12345".to_string()).is_err()); // lowercase
        assert!(AccountId::try_new("ACC_12345".to_string()).is_err()); // underscore
        assert!(AccountId::try_new("12345".to_string()).is_err()); // no prefix
        assert!(AccountId::try_new("ACC-".to_string()).is_err()); // no suffix

        // Invalid cases - too long
        let long_id = format!("ACC-{}", "A".repeat(100));
        assert!(AccountId::try_new(long_id).is_err());
    }

    #[test]
    fn transfer_id_validation() {
        // Valid cases
        assert!(TransferId::try_new("TXF-12345678".to_string()).is_ok());
        assert!(TransferId::try_new("TXF-ABCDEF123".to_string()).is_ok());

        // Invalid cases - empty
        assert!(TransferId::try_new(String::new()).is_err());
        assert!(TransferId::try_new("   ".to_string()).is_err());

        // Invalid cases - wrong format
        assert!(TransferId::try_new("txf-12345".to_string()).is_err()); // lowercase
        assert!(TransferId::try_new("TXF_12345".to_string()).is_err()); // underscore
        assert!(TransferId::try_new("12345".to_string()).is_err()); // no prefix
        assert!(TransferId::try_new("TXF-".to_string()).is_err()); // no suffix
    }

    #[test]
    fn customer_name_validation() {
        // Valid cases
        assert!(CustomerName::try_new("John Doe".to_string()).is_ok());
        assert!(CustomerName::try_new("Mary O'Brien".to_string()).is_ok());
        assert!(CustomerName::try_new("Jean-Claude Van Damme".to_string()).is_ok());
        assert!(CustomerName::try_new("Li Wei".to_string()).is_ok());

        // Invalid cases - too short
        assert!(CustomerName::try_new("J".to_string()).is_err());
        assert!(CustomerName::try_new(String::new()).is_err());

        // Invalid cases - too long
        let long_name = "A".repeat(101);
        assert!(CustomerName::try_new(long_name).is_err());

        // Trimming works
        assert!(CustomerName::try_new("  John Doe  ".to_string()).is_ok());
        let name = CustomerName::try_new("  John Doe  ".to_string()).unwrap();
        assert_eq!(name.as_ref(), "John Doe");
    }

    #[test]
    fn money_arithmetic_operations() {
        let ten = Money::from_cents(1000).unwrap();
        let five = Money::from_cents(500).unwrap();
        let fifteen = Money::from_cents(1500).unwrap();

        // Addition
        assert_eq!(ten.add(&five).unwrap(), fifteen);
        assert_eq!(five.add(&ten).unwrap(), fifteen);

        // Subtraction
        assert_eq!(fifteen.subtract(&five).unwrap(), ten);
        assert_eq!(ten.subtract(&five).unwrap(), five);

        // Subtraction that would be negative
        assert!(five.subtract(&ten).is_err());
    }

    #[test]
    fn money_display_formatting() {
        assert_eq!(Money::from_cents(0).unwrap().to_string(), "$0");
        assert_eq!(Money::from_cents(100).unwrap().to_string(), "$1");
        assert_eq!(Money::from_cents(1050).unwrap().to_string(), "$10.50");
        assert_eq!(Money::from_cents(99999).unwrap().to_string(), "$999.99");
    }
}

#[cfg(test)]
mod command_validation_tests {
    use crate::banking::{
        commands::{BankingError, OpenAccount, TransferMoneyInput},
        types::{AccountHolder, AccountId, CustomerName, Money, TransferId},
    };

    #[test]
    fn open_account_command_accepts_valid_data() {
        let command = OpenAccount::new(
            AccountId::generate(),
            AccountHolder {
                name: CustomerName::try_new("Test User".to_string()).unwrap(),
                email: "test@example.com".to_string(),
            },
            Money::from_cents(10000).unwrap(),
        );

        // If construction succeeds, the command is valid
        assert!(command.initial_deposit == Money::from_cents(10000).unwrap());
    }

    #[test]
    fn transfer_money_input_rejects_same_account() {
        let account = AccountId::generate();
        let result = TransferMoneyInput::new(
            TransferId::generate(),
            account.clone(),
            account.clone(),
            Money::from_cents(1000).unwrap(),
            None,
        );

        match result {
            Err(BankingError::SameAccountTransfer(id)) => {
                assert_eq!(id, account);
            }
            _ => panic!("Expected SameAccountTransfer error"),
        }
    }

    #[test]
    fn transfer_money_input_accepts_valid_transfer() {
        let from = AccountId::generate();
        let to = AccountId::generate();

        let input = TransferMoneyInput::new(
            TransferId::generate(),
            from.clone(),
            to.clone(),
            Money::from_cents(5000).unwrap(),
            Some("Test transfer".to_string()),
        )
        .unwrap();

        assert_eq!(input.from_account, from);
        assert_eq!(input.to_account, to);
        assert_eq!(input.amount, Money::from_cents(5000).unwrap());
        assert_eq!(input.description, Some("Test transfer".to_string()));
    }
}
