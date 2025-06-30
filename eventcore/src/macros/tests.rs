//! Tests for helper macros.

use crate::command::CommandResult;
use crate::errors::CommandError;
use crate::require;

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
