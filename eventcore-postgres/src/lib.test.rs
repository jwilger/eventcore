use super::{glob_to_anchored_regex, regex_escape};

#[test]
fn star_translates_to_dot_star_anchored() {
    // '-' is not a regex metacharacter outside a class, so it stays literal.
    assert_eq!(glob_to_anchored_regex("account-*"), "^account-.*$");
}

#[test]
fn question_mark_translates_to_dot() {
    assert_eq!(glob_to_anchored_regex("account-?"), "^account-.$");
}

#[test]
fn character_class_is_preserved() {
    assert_eq!(
        glob_to_anchored_regex("account-[0-9]*"),
        "^account-[0-9].*$"
    );
}

#[test]
fn negated_character_class_uses_caret() {
    assert_eq!(glob_to_anchored_regex("account-[!0-9]"), "^account-[^0-9]$");
}

#[test]
fn literal_regex_metacharacters_are_escaped() {
    // A literal '.' in the glob must not become a regex wildcard, and other
    // regex metacharacters must be escaped to prevent injection.
    assert_eq!(glob_to_anchored_regex("a.c+(d)"), "^a\\.c\\+\\(d\\)$");
}

#[test]
fn regex_escape_escapes_all_metacharacters() {
    assert_eq!(
        regex_escape(".^$*+?()[]{}|\\"),
        "\\.\\^\\$\\*\\+\\?\\(\\)\\[\\]\\{\\}\\|\\\\"
    );
}
