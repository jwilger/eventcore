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

/// Property: Literal strings (no metacharacters) are always valid globs.
///
/// A pattern with no glob syntax is a literal match and must always
/// compile.
#[test]
fn literal_strings_are_valid_glob_patterns() {
    proptest!(|(s in "[^*?\\[\\]]+")| {
        prop_assert!(
            is_valid_glob_pattern(&s),
            "Literal string should be a valid glob pattern: {:?}",
            s
        );
    });
}

/// Property: Strings ending in an unclosed character class are invalid globs.
///
/// A `[` that is never closed by a `]` is a syntax error in the `glob`
/// crate, so such patterns must be rejected at construction time.
#[test]
fn unclosed_character_class_is_invalid_glob_pattern() {
    proptest!(|(prefix in "[a-z]+")| {
        let pattern = format!("{prefix}[");
        prop_assert!(
            !is_valid_glob_pattern(&pattern),
            "Unclosed character class should be an invalid glob pattern: {:?}",
            pattern
        );
    });
}

/// Property: Common glob wildcards compile successfully.
///
/// Patterns combining a literal prefix with `*`, `?`, or a `[0-9]`
/// character class are valid POSIX glob syntax and must be accepted.
#[test]
fn common_wildcard_patterns_are_valid() {
    let wildcard = prop_oneof![
        Just("*".to_string()),
        Just("?".to_string()),
        Just("[0-9]".to_string()),
        Just("[a-z]*".to_string()),
    ];

    proptest!(|(prefix in "[a-z]+", w in wildcard)| {
        let pattern = format!("{prefix}-{w}");
        prop_assert!(
            is_valid_glob_pattern(&pattern),
            "Common wildcard pattern should be valid: {:?}",
            pattern
        );
    });
}
