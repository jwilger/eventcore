use eventcore::{
    CommandLogic, Event, EventStore, InMemoryEventStore, NewEvents, StreamId, execute,
};
use nutype::nutype;

#[nutype(sanitize(trim), validate(len_char_max = 240))]
struct AccountId(String);

#[nutype(validate(greater = 0))]
struct DepositAmount(u16);

/// Test-specific event type representing a money deposit.
///
/// This is a domain event from the consumer's perspective (BankAccount domain).
/// The library (EventCore) doesn't know about MoneyDeposited - it only knows
/// about the generic Event type. This demonstrates how consumers define their
/// own event types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct MoneyDeposited;

/// Minimal stub for Deposit command used in testing.
///
/// This is a test-specific command that will be used to verify the
/// single-stream command execution flow. The actual implementation
/// will require implementing CommandLogic trait.
struct Deposit;

impl CommandLogic<MoneyDeposited> for Deposit {
    type State = ();

    fn apply(&self, state: Self::State, _event: Event<MoneyDeposited>) -> Self::State {
        state
    }

    fn handle(
        &self,
        _state: Self::State,
    ) -> Result<NewEvents<MoneyDeposited>, eventcore::CommandError> {
        Ok(NewEvents::default())
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

/// Integration test for I-001: Verify events are actually stored
///
/// This test verifies the core event sourcing behavior: commands must persist
/// events to the store and those events must be retrievable for state reconstruction.
///
/// Expected scenario: After executing a Deposit command, developer can read events
/// from the account stream and verify at least one event was stored.
#[tokio::test]
async fn test_deposit_command_stores_events() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates a stream ID for a bank account
    let account_id = StreamId::try_new("account-123".to_string()).expect("a valid stream id");

    // And: Developer creates a Deposit command
    let command = Deposit;

    // When: Developer executes the command
    execute(&store, command)
        .await
        .expect("command execution to succeed");

    // And: Developer reads events from the account stream
    let events = store
        .read_stream::<MoneyDeposited>(account_id)
        .await
        .expect("reading a stream to succeed");

    // Then: At least one event was stored
    assert!(
        events.len() > 0,
        "Deposit command should store at least one event"
    );
}

/// Integration test for I-001: Verify actual event data is retrievable
///
/// This test verifies that we can access and validate the actual event data
/// stored by a command. This is essential for event sourcing: events must
/// contain the data needed to reconstruct state.
///
/// Expected scenario: After executing a Deposit command, developer can read
/// the stored event and access its data (event type, payload, metadata).
#[tokio::test]
async fn test_deposit_command_event_data_is_retrievable() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates a stream ID for a bank account
    let account_id = StreamId::try_new("account-123".to_string()).expect("a valid stream id");

    // And: Developer creates a Deposit command
    let command = Deposit;

    // When: Developer executes the command
    execute(&store, command)
        .await
        .expect("command execution to succeed");

    // And: Developer reads events from the account stream
    let events = store
        .read_stream::<MoneyDeposited>(account_id)
        .await
        .expect("reading a stream to succeed");

    // And: Developer accesses the first event
    let first_event = events.first().expect("at least one event to exist");

    // Then: Event has expected event type
    assert_eq!(
        first_event.payload, MoneyDeposited,
        "Deposit command should produce MoneyDeposited event"
    );
}
