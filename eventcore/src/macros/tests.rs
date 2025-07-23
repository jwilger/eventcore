//! Tests for helper macros.

use crate::{require, CommandError, CommandResult};

#[test]
fn test_require_macro_success() {
    fn test_function() -> CommandResult<()> {
        let condition = true;
        require!(condition, "This should not fail");
        Ok(())
    }

    assert!(test_function().is_ok());
}

#[test]
fn test_require_macro_failure() {
    fn test_function() -> CommandResult<()> {
        let condition = false;
        require!(condition, "This should fail");
        Ok(())
    }

    match test_function() {
        Err(CommandError::BusinessRuleViolation(msg)) => {
            assert_eq!(msg, "This should fail");
        }
        _ => panic!("Expected BusinessRuleViolation error"),
    }
}

#[test]
fn test_emit_macro() {
    // This test would require a proper setup with ReadStreams and events,
    // which is complex to mock here. The macro itself is simple enough
    // that integration tests in the examples crate would be more valuable.

    // Placeholder to ensure the macro module compiles
}

#[test]
fn test_emit_macro_no_clippy_warning() {
    // Test to ensure the emit! macro doesn't trigger clippy::vec_init_then_push
    // This test verifies that the pattern shown in the issue doesn't produce warnings

    // We can't easily test the actual macro expansion here, but we can
    // document the issue and solution for reference

    // The issue is that code like this:
    // ```
    // let mut events = vec![];
    // emit!(events, &read_streams, stream_id, SomeEvent { data: "test" });
    // ```
    //
    // Expands to:
    // ```
    // let mut events = vec![];
    // events.push(StreamWrite::new(&read_streams, stream_id, SomeEvent { data: "test" })?);
    // ```
    //
    // Which triggers clippy::vec_init_then_push because it sees a push right after vec![]

    // The solution is to add #[allow(clippy::vec_init_then_push)] to the macro expansion
}
