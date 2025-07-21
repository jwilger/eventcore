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
    // This test verifies that the macros compile correctly when used from an external crate.
    // The WithdrawMoney struct and its CommandLogic implementation demonstrate that:
    // 1. The #[derive(Command)] macro works correctly
    // 2. The emit! macro can be used within command handlers
    // 3. The require! macro properly generates validation code

    // Test that require! macro expands correctly
    fn test_require() -> CommandResult<()> {
        let condition = false;
        require!(condition, "Test error message");
        Ok(())
    }

    // Create an instance to verify the struct compiles and can be instantiated
    let _cmd = WithdrawMoney {
        account_stream: StreamId::try_new("test-account".to_string()).unwrap(),
        amount: 100,
    };

    match test_require() {
        Err(CommandError::BusinessRuleViolation(msg)) => {
            assert_eq!(msg, "Test error message");
        }
        _ => panic!("Expected BusinessRuleViolation error"),
    }
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
