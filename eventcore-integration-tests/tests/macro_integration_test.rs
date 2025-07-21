//! Integration test to verify emit! and require! macros work correctly
//! when used from external crates (not within eventcore itself).

use async_trait::async_trait;
use eventcore::{
    emit, require, CommandError, CommandLogic, CommandResult, ReadStreams, StreamId,
    StreamResolver, StreamWrite,
};
use eventcore_macros::Command;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TestState {
    balance: u64,
    is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TestEvent {
    MoneyWithdrawn { amount: u64 },
    AccountDeactivated,
}

#[derive(Command, Clone)]
#[allow(dead_code)]
struct WithdrawMoney {
    #[stream]
    account_stream: StreamId,
    amount: u64,
}

#[async_trait]
impl CommandLogic for WithdrawMoney {
    type State = TestState;
    type Event = TestEvent;

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            TestEvent::MoneyWithdrawn { amount } => {
                state.balance = state.balance.saturating_sub(*amount);
            }
            TestEvent::AccountDeactivated => {
                state.is_active = false;
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Test the require! macro - this demonstrates that the macro works
        // and properly returns CommandError::BusinessRuleViolation
        require!(state.is_active, "Account is not active");
        require!(
            state.balance >= self.amount,
            "Insufficient funds for withdrawal"
        );

        let mut events = Vec::new();

        // Test the emit! macro - this demonstrates that the macro works
        // and properly creates StreamWrite instances
        emit!(
            events,
            &read_streams,
            self.account_stream.clone(),
            TestEvent::MoneyWithdrawn {
                amount: self.amount
            }
        );

        // Test multiple emits
        if state.balance - self.amount == 0 {
            emit!(
                events,
                &read_streams,
                self.account_stream.clone(),
                TestEvent::AccountDeactivated
            );
        }

        Ok(events)
    }
}

#[test]
fn test_macros_compile() {
    // This test just verifies that the macros compile correctly
    // when used from an external crate. The actual functionality
    // is tested through the command logic implementation above.

    // Test that require! macro expands correctly
    fn test_require() -> CommandResult<()> {
        let condition = false;
        require!(condition, "Test error message");
        Ok(())
    }

    match test_require() {
        Err(CommandError::BusinessRuleViolation(msg)) => {
            assert_eq!(msg, "Test error message");
        }
        _ => panic!("Expected BusinessRuleViolation error"),
    }

    // The emit! macro is tested implicitly by the CommandLogic implementation
    // compiling successfully above.
}

#[test]
fn test_require_macro_with_complex_expressions() {
    // Test that require! works with more complex boolean expressions
    fn test_complex_require() -> CommandResult<()> {
        let x = 5;
        let y = 10;
        require!(x > y, "x must be greater than y");
        Ok(())
    }

    match test_complex_require() {
        Err(CommandError::BusinessRuleViolation(msg)) => {
            assert_eq!(msg, "x must be greater than y");
        }
        _ => panic!("Expected BusinessRuleViolation error"),
    }
}

#[test]
fn test_require_macro_success_case() {
    // Test that require! doesn't return error when condition is true
    fn test_successful_require() -> CommandResult<()> {
        let condition = true;
        require!(condition, "This should not fail");
        Ok(())
    }

    assert!(test_successful_require().is_ok());
}
