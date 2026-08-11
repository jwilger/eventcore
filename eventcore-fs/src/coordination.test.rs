use super::*;

#[test]
fn sanitize_is_injective_and_filesystem_safe() {
    let one = sanitize("accounts::balance");
    let two = sanitize("accounts::balances");
    assert_ne!(one, two);
    assert!(one.chars().all(|c| c.is_ascii_hexdigit()));
}
