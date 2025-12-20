//! Shared validation predicates for domain types.
//!
//! This module contains validation functions used by nutype-based domain types
//! across the eventcore crate.

/// Validation predicate: reject glob metacharacters.
///
/// Per ADR-017, domain types like StreamId reserve glob metacharacters
/// (*, ?, [, ]) to enable future pattern matching without ambiguity or
/// escaping complexity.
pub(crate) fn no_glob_metacharacters(s: &str) -> bool {
    !s.contains(['*', '?', '[', ']'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_without_metacharacters_pass_validation() {
        assert!(no_glob_metacharacters("account-123"));
        assert!(no_glob_metacharacters("tenant/account/456"));
        assert!(no_glob_metacharacters("order-2024-12-10-001"));
    }

    #[test]
    fn strings_with_metacharacters_fail_validation() {
        assert!(!no_glob_metacharacters("account-*"));
        assert!(!no_glob_metacharacters("account-?"));
        assert!(!no_glob_metacharacters("account-["));
        assert!(!no_glob_metacharacters("account-]"));
    }
}
