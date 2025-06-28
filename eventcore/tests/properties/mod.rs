//! Property-based test suite for eventcore library.
//!
//! This module contains property-based tests that verify fundamental invariants
//! of the event sourcing system. These tests use the `proptest` library to
//! generate many test cases and verify that key properties hold across all
//! possible inputs.
//!
//! ## Property Tests Included
//!
//! 1. **Event Immutability**: Once created, events cannot be modified
//! 2. **Version Monotonicity**: Stream versions always increase monotonically
//! 3. **Event Ordering**: Event ordering is deterministic and consistent
//! 4. **Command Idempotency**: Commands produce the same result when repeated
//! 5. **Concurrency Consistency**: Concurrent commands maintain system consistency
//!
//! ## Running Property Tests
//!
//! Property tests can be run with:
//! ```bash
//! cargo test --test properties
//! ```
//!
//! To run a specific property test module:
//! ```bash
//! cargo test --test properties event_immutability
//! ```
//!
//! ## Configuration
//!
//! Property tests are configured to run 1000 cases by default.
//! This can be overridden with the `PROPTEST_CASES` environment variable:
//! ```bash
//! PROPTEST_CASES=10000 cargo test --test properties
//! ```

pub mod event_immutability;
pub mod version_monotonicity;
pub mod event_ordering;
pub mod command_idempotency;
pub mod concurrency_consistency;

use proptest::test_runner::Config;

/// Default configuration for property tests.
///
/// Sets reasonable defaults for test execution:
/// - 1000 test cases (can be overridden with PROPTEST_CASES env var)
/// - 10 second timeout per test case
/// - Source file and line number reporting for failures
pub fn default_proptest_config() -> Config {
    Config {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000),
        timeout: 10000, // 10 seconds
        source_file: Some("properties"),
        ..Config::default()
    }
}

/// Macro for defining property tests with the default configuration.
///
/// This macro sets up the standard proptest configuration and provides
/// consistent test naming and structure.
///
/// # Example
/// ```rust,ignore
/// property_test! {
///     test_some_invariant(input in arb_input()) {
///         // Test logic here
///         prop_assert!(some_condition);
///     }
/// }
/// ```
#[macro_export]
macro_rules! property_test {
    (
        $(#[$meta:meta])*
        $test_name:ident($($param:ident in $strategy:expr),* $(,)?) $body:block
    ) => {
        $(#[$meta])*
        #[test]
        fn $test_name() {
            use proptest::prelude::*;
            use $crate::properties::default_proptest_config;

            let config = default_proptest_config();
            proptest! {
                #![proptest_config(config)]
                #[test]
                fn inner($($param in $strategy),*) $body
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_respects_env_var() {
        // Set environment variable temporarily
        std::env::set_var("PROPTEST_CASES", "5000");
        let config = default_proptest_config();
        assert_eq!(config.cases, 5000);
        
        // Clean up
        std::env::remove_var("PROPTEST_CASES");
    }

    #[test]
    fn test_default_config_fallback() {
        // Ensure no environment variable is set
        std::env::remove_var("PROPTEST_CASES");
        let config = default_proptest_config();
        assert_eq!(config.cases, 1000);
    }

    #[test]
    fn test_config_timeout_is_reasonable() {
        let config = default_proptest_config();
        assert_eq!(config.timeout, 10000); // 10 seconds
    }
}