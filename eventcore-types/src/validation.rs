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

/// Validation predicate: accept only strings that compile as a glob pattern.
///
/// Per ADR-0047, `StreamPattern` carries a POSIX glob pattern used for
/// subscription filtering. Parsing the pattern at construction time
/// (parse-don't-validate) guarantees that an invalid pattern (e.g. an
/// unclosed character class `account-[`) can never be constructed, so
/// matching code never has to recover from a compile error.
pub(crate) fn is_valid_glob_pattern(s: &str) -> bool {
    glob::Pattern::new(s).is_ok()
}

#[cfg(test)]
#[path = "validation.test.rs"]
mod tests;
