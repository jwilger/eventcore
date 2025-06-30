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
//! Property tests support multiple configuration modes:
//!
//! ### CI Mode (Fast)
//! Runs fewer cases with aggressive timeouts for CI environments:
//! ```bash
//! PROPTEST_MODE=ci cargo test --test properties
//! ```
//!
//! ### Comprehensive Mode (Thorough)
//! Runs extensive test cases for thorough validation:
//! ```bash
//! PROPTEST_MODE=comprehensive cargo test --test properties
//! ```
//!
//! ### Custom Configuration
//! Override specific settings:
//! ```bash
//! PROPTEST_CASES=5000 PROPTEST_TIMEOUT=30000 cargo test --test properties
//! ```
//!
//! ## Shrinking Strategy
//!
//! The tests use enhanced shrinking strategies to provide minimal counterexamples:
//! - Domain-aware shrinking for event sourcing types
//! - Simplified failure reproduction with minimal test cases
//! - Custom shrinking for complex multi-stream scenarios

pub mod event_immutability;
pub mod version_monotonicity;
pub mod event_ordering;
pub mod command_idempotency;
pub mod concurrency_consistency;
pub mod advanced_concurrency;

use proptest::test_runner::Config;

/// Test execution modes with different performance characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    /// Fast mode for CI environments - fewer cases, shorter timeouts
    Ci,
    /// Standard mode for development - balanced speed and coverage
    Standard,
    /// Comprehensive mode for thorough testing - maximum cases and time
    Comprehensive,
}

impl TestMode {
    /// Get the test mode from environment variables.
    ///
    /// Checks `PROPTEST_MODE` environment variable:
    /// - "ci" -> CI mode (fast)
    /// - "comprehensive" -> Comprehensive mode (thorough)  
    /// - anything else -> Standard mode (default)
    pub fn from_env() -> Self {
        match std::env::var("PROPTEST_MODE").as_deref() {
            Ok("ci") => Self::Ci,
            Ok("comprehensive") => Self::Comprehensive,
            _ => Self::Standard,
        }
    }

    /// Get the number of test cases for this mode.
    pub fn cases(self) -> u32 {
        match self {
            Self::Ci => 100,           // Fast CI runs
            Self::Standard => 1000,    // Balanced for development
            Self::Comprehensive => 10000, // Thorough testing
        }
    }

    /// Get the timeout per test case in milliseconds.
    pub fn timeout_ms(self) -> u32 {
        match self {
            Self::Ci => 5000,      // 5 seconds - aggressive for CI
            Self::Standard => 10000, // 10 seconds - reasonable default
            Self::Comprehensive => 30000, // 30 seconds - thorough testing
        }
    }

    /// Get the maximum shrinking iterations for this mode.
    pub fn max_shrink_iters(self) -> u32 {
        match self {
            Self::Ci => 512,       // Limited shrinking for speed
            Self::Standard => 1024,  // Standard shrinking
            Self::Comprehensive => 4096, // Extensive shrinking for minimal examples
        }
    }
}

/// Enhanced configuration for property tests with improved shrinking.
///
/// This configuration provides:
/// - Mode-based settings (CI, Standard, Comprehensive)
/// - Enhanced shrinking strategies for better minimal counterexamples
/// - Environment variable overrides for flexibility
/// - Detailed failure reporting
pub fn enhanced_proptest_config() -> Config {
    let mode = TestMode::from_env();
    
    // Allow environment variable overrides
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| mode.cases());
        
    let timeout = std::env::var("PROPTEST_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| mode.timeout_ms());
        
    let max_shrink_iters = std::env::var("PROPTEST_MAX_SHRINK_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| mode.max_shrink_iters());

    Config {
        cases,
        timeout,
        max_shrink_iters,
        source_file: Some("properties"),
        verbose: std::env::var("PROPTEST_VERBOSE").is_ok(),
        // Enable fork for better failure isolation in comprehensive mode
        fork: mode == TestMode::Comprehensive,
        ..Config::default()
    }
}

/// Default configuration for property tests (for backward compatibility).
///
/// Sets reasonable defaults for test execution:
/// - 1000 test cases (can be overridden with PROPTEST_CASES env var)
/// - 10 second timeout per test case
/// - Source file and line number reporting for failures
///
/// **Note**: Consider using `enhanced_proptest_config()` for better shrinking strategies.
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

/// Macro for defining property tests with enhanced configuration.
///
/// This macro sets up enhanced proptest configuration with improved shrinking
/// strategies and configurable test modes.
///
/// # Example
/// ```rust,ignore
/// enhanced_property_test! {
///     test_some_invariant(input in arb_input()) {
///         // Test logic here
///         prop_assert!(some_condition);
///     }
/// }
/// ```
#[macro_export]
macro_rules! enhanced_property_test {
    (
        $(#[$meta:meta])*
        $test_name:ident($($param:ident in $strategy:expr),* $(,)?) $body:block
    ) => {
        $(#[$meta])*
        #[test]
        fn $test_name() {
            use proptest::prelude::*;
            use $crate::properties::enhanced_proptest_config;

            let config = enhanced_proptest_config();
            proptest! {
                #![proptest_config(config)]
                #[test]
                fn inner($($param in $strategy),*) $body
            }
        }
    };
}

/// Macro for defining property tests with the default configuration (legacy).
///
/// This macro sets up the standard proptest configuration and provides
/// consistent test naming and structure.
///
/// **Note**: Consider using `enhanced_property_test!` for better shrinking strategies.
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