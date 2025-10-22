use eventcore::{
    CommandLogic, Event, EventStore, InMemoryEventStore, NewEvents, StreamId, execute,
};
use nutype::nutype;
use uuid::Uuid;

#[nutype(validate(greater = 0), derive(Debug, Clone, PartialEq, Eq))]
struct DepositAmount(u16);

/// Test-specific domain events enum for BankAccount aggregate.
///
/// The Event trait implementation extracts the stream_id from each variant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestDomainEvents {
    MoneyDeposited {
        account_id: StreamId, // Aggregate identity - each event knows its stream
        amount: DepositAmount,
    },
}

impl Event for TestDomainEvents {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestDomainEvents::MoneyDeposited { account_id, .. } => account_id,
        }
    }
}

/// Deposit command for testing single-stream command execution.
struct Deposit {
    account_id: StreamId,
    amount: DepositAmount,
}

impl CommandLogic for Deposit {
    type Event = TestDomainEvents;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(
        &self,
        _state: Self::State,
    ) -> Result<NewEvents<Self::Event>, eventcore::CommandError> {
        let event = TestDomainEvents::MoneyDeposited {
            account_id: self.account_id.clone(),
            amount: self.amount.clone(),
        };
        Ok(vec![event].into())
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

    // And: Developer creates a Deposit command with account and amount
    let account_id = StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id");
    let amount = DepositAmount::try_new(100).expect("valid amount");
    let command = Deposit { account_id, amount };

    // When: Developer executes the command
    let result = execute(&store, command).await;

    // Then: Command succeeds
    assert!(result.is_ok(), "Deposit command should succeed");
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
    let account_id = StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id");

    // And: Developer creates a Deposit command
    let amount = DepositAmount::try_new(100).expect("valid amount");
    let command = Deposit {
        account_id: account_id.clone(),
        amount: amount.clone(),
    };

    // When: Developer executes the command
    execute(&store, command)
        .await
        .expect("command execution to succeed");

    // And: Developer reads events from the account stream
    let events = store
        .read_stream::<TestDomainEvents>(account_id.clone())
        .await
        .expect("reading a stream to succeed");

    // And: Developer accesses the first event
    let first_event = events.first().expect("at least one event to exist");

    // Then: Event is MoneyDeposited with correct data
    match first_event {
        TestDomainEvents::MoneyDeposited {
            account_id: event_account_id,
            amount: event_amount,
        } => {
            assert_eq!(
                event_account_id, &account_id,
                "Event should be for correct account"
            );
            assert_eq!(event_amount, &amount, "Event should have correct amount");
        }
    }
}
