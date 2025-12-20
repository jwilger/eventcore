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
    use proptest::prelude::*;

    /// Property: Any string without glob metacharacters passes validation.
    ///
    /// This generates arbitrary strings that explicitly exclude the four
    /// glob metacharacters (*, ?, [, ]) and verifies they all pass.
    #[test]
    fn strings_without_metacharacters_pass_validation() {
        proptest!(|(s in "[^*?\\[\\]]*")| {
            prop_assert!(
                no_glob_metacharacters(&s),
                "String without metacharacters should pass: {:?}",
                s
            );
        });
    }

    /// Property: Any string containing at least one glob metacharacter fails validation.
    ///
    /// This uses a strategy that places a metacharacter at an arbitrary position
    /// (including first and last) within a string of safe characters.
    #[test]
    fn strings_with_metacharacters_fail_validation() {
        let safe_chars = "[^*?\\[\\]]*";
        let metachar = prop_oneof![Just('*'), Just('?'), Just('['), Just(']')];

        let strategy = (safe_chars, metachar, safe_chars)
            .prop_map(|(prefix, mc, suffix)| format!("{}{}{}", prefix, mc, suffix));

        proptest!(|(s in strategy)| {
            prop_assert!(
                !no_glob_metacharacters(&s),
                "String with metacharacter should fail: {:?}",
                s
            );
        });
    }
}
