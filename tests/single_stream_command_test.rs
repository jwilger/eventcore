use eventcore::{CommandLogic, Event, InMemoryEventStore, NewEvents, execute};
use nutype::nutype;

#[nutype(sanitize(trim), validate(len_char_max = 240))]
struct AccountId(String);

#[nutype(validate(greater = 0))]
struct DepositAmount(u16);

/// Minimal stub for Deposit command used in testing.
///
/// This is a test-specific command that will be used to verify the
/// single-stream command execution flow. The actual implementation
/// will require implementing CommandLogic trait.
struct Deposit;

impl CommandLogic for Deposit {
    type State = ();

    fn apply(&self, state: Self::State, _event: Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents, eventcore::CommandError> {
        unimplemented!()
    }
}

/// Integration test for I-001: Single-Stream Command End-to-End
///
/// This test exercises a complete single-stream command execution from the
/// library consumer (application developer) perspective. It tests the BankAccount
/// domain example with a Deposit command.
///
/// Expected scenario: Developer executes Deposit(account_id: "account-123", amount: 100)
/// and command succeeds.
#[tokio::test]
async fn test_deposit_command_succeeds() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates a Deposit command
    let command = Deposit;

    // When: Developer executes the command
    let result = execute(&store, command).await;

    // Then: Command succeeds
    assert!(result.is_ok(), "Deposit command should succeed");
}
