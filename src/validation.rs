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
