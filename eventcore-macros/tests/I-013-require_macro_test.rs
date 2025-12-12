use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, NewEvents, StreamDeclarations, StreamId,
    require,
};
use eventcore_macros::Command;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AccountEvent {
    stream_id: StreamId,
}

impl Event for AccountEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> &'static str {
        "AccountEvent"
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AccountState {
    available_funds: u64,
}

fn account_stream() -> StreamId {
    StreamId::try_new("accounts::primary".to_string()).expect("static stream id should be valid")
}

#[derive(Command)]
struct LiteralWithdrawCommand {
    #[stream]
    account_id: StreamId,
    amount: u64,
}

impl CommandLogic for LiteralWithdrawCommand {
    type Event = AccountEvent;
    type State = AccountState;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        require!(state.available_funds >= self.amount, "insufficient funds");

        Ok(NewEvents::default())
    }
}

#[derive(Command)]
struct FormattedWithdrawCommand {
    #[stream]
    account_id: StreamId,
    amount: u64,
}

impl CommandLogic for FormattedWithdrawCommand {
    type Event = AccountEvent;
    type State = AccountState;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        require!(
            state.available_funds >= self.amount,
            "Insufficient: have {}, need {}",
            state.available_funds,
            self.amount
        );

        Ok(NewEvents::default())
    }
}

struct ManualWithdrawCommand {
    account_id: StreamId,
    amount: u64,
}

impl CommandStreams for ManualWithdrawCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::single(self.account_id.clone())
    }
}

impl CommandLogic for ManualWithdrawCommand {
    type Event = AccountEvent;
    type State = AccountState;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if state.available_funds < self.amount {
            return Err(CommandError::BusinessRuleViolation(
                "insufficient funds".to_string(),
            ));
        }

        Ok(NewEvents::default())
    }
}

#[test]
fn developer_validates_simple_condition_with_command() {
    // Given: developer reconstructs an account state with enough funds to cover the withdrawal.
    let account_id = account_stream();
    let command = LiteralWithdrawCommand {
        account_id,
        amount: 25,
    };
    let state = AccountState {
        available_funds: 25,
    };

    // When: the literal `require!`-based command handles the request using the reconstructed state.
    let result = command.handle(state);

    // Then: command execution succeeds because the guard condition passes.
    assert!(
        result.is_ok(),
        "require! should allow execution when funds cover the withdrawal"
    );
}

#[test]
fn developer_formats_error_messages_inside_command_logic() {
    // Given: developer reconstructs state showing fewer funds than the requested withdrawal.
    let command = FormattedWithdrawCommand {
        account_id: account_stream(),
        amount: 75,
    };
    let state = AccountState {
        available_funds: 25,
    };

    // When: the formatted `require!` guard runs during command handling.
    let result = command.handle(state);

    // Then: the macro returns a business rule violation that preserves the formatted message.
    assert!(
        matches!(
            result,
            Err(CommandError::BusinessRuleViolation(message))
                if message == "Insufficient: have 25, need 75"
        ),
        "require! should propagate formatted error messages for developers"
    );
}

#[test]
fn developer_migrates_manual_validation_to_require_without_behavior_changes() {
    // Given: developer maintains both manual and macro-backed commands referencing the same stream.
    let withdrawal_amount = 50u64;
    let manual_command = ManualWithdrawCommand {
        account_id: account_stream(),
        amount: withdrawal_amount,
    };
    let literal_command = LiteralWithdrawCommand {
        account_id: account_stream(),
        amount: withdrawal_amount,
    };

    // When: both command styles handle insufficient and sufficient balance states for regression.
    let insufficient_state = AccountState {
        available_funds: 25,
    };
    let sufficient_state = AccountState {
        available_funds: 75,
    };
    let manual_fail = manual_command.handle(insufficient_state);
    let macro_fail = literal_command.handle(insufficient_state);
    let manual_success = manual_command.handle(sufficient_state);
    let macro_success = literal_command.handle(sufficient_state);

    // Then: migration keeps error messaging and success behavior identical.
    let failure_behavior_identical = match (manual_fail, macro_fail) {
        (
            Err(CommandError::BusinessRuleViolation(manual_message)),
            Err(CommandError::BusinessRuleViolation(macro_message)),
        ) => manual_message == "insufficient funds" && macro_message == "insufficient funds",
        _ => false,
    };
    let success_behavior_identical = manual_success.is_ok() && macro_success.is_ok();

    assert!(
        failure_behavior_identical && success_behavior_identical,
        "require! migration should not change error text or successful validation outcomes",
    );
}
