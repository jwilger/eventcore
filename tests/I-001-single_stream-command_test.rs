use eventcore::{
    CommandError, CommandLogic, Event, EventStore, EventStoreError, EventStreamReader,
    EventStreamSlice, InMemoryEventStore, NewEvents, StreamId, StreamWrites, execute,
};
use nutype::nutype;
use uuid::Uuid;

#[nutype(validate(greater = 0), derive(Debug, Clone, Copy, PartialEq, Eq))]
struct MoneyAmount(u16);

/// Test-specific domain events enum for BankAccount aggregate.
///
/// The Event trait implementation extracts the stream_id from each variant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestDomainEvents {
    MoneyDeposited {
        account_id: StreamId, // Aggregate identity - each event knows its stream
        amount: MoneyAmount,
    },
    MoneyWithdrawn {
        account_id: StreamId,
        amount: MoneyAmount,
    },
}

impl Event for TestDomainEvents {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestDomainEvents::MoneyDeposited { account_id, .. }
            | TestDomainEvents::MoneyWithdrawn { account_id, .. } => account_id,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AccountBalance {
    cents: u16,
}

impl AccountBalance {
    fn apply_event(mut self, event: &TestDomainEvents) -> Self {
        match event {
            TestDomainEvents::MoneyDeposited { amount, .. } => {
                self.cents = self.cents.saturating_add(amount.into_inner());
            }
            TestDomainEvents::MoneyWithdrawn { amount, .. } => {
                self.cents = self.cents.saturating_sub(amount.into_inner());
            }
        }
        self
    }
}

/// Deposit command for testing single-stream command execution.
struct Deposit {
    account_id: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Deposit {
    type Event = TestDomainEvents;
    type State = ();

    fn stream_id(&self) -> &StreamId {
        &self.account_id
    }

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(
        &self,
        _state: Self::State,
    ) -> Result<NewEvents<Self::Event>, eventcore::CommandError> {
        Ok(vec![TestDomainEvents::MoneyDeposited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

/// Withdraw command validates state before emitting events.
struct Withdraw {
    account_id: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Withdraw {
    type Event = TestDomainEvents;
    type State = AccountBalance;

    fn stream_id(&self) -> &StreamId {
        &self.account_id
    }

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.apply_event(event)
    }

    fn handle(
        &self,
        state: Self::State,
    ) -> Result<NewEvents<Self::Event>, eventcore::CommandError> {
        let requested = self.amount.into_inner();
        if state.cents < requested {
            return Err(CommandError::BusinessRuleViolation(format!(
                "insufficient funds for account {}: balance={}, attempted_withdrawal={}",
                self.account_id.as_ref(),
                state.cents,
                requested
            )));
        }

        let event = TestDomainEvents::MoneyWithdrawn {
            account_id: self.account_id.clone(),
            amount: self.amount,
        };

        Ok(vec![event].into())
    }
}

/// Integration test for I-001: Single-Stream Command End-to-End
///
/// This test exercises a complete single-stream command execution from the
/// library consumer (application developer) perspective. It tests the BankAccount
/// domain example with a Deposit command.
#[tokio::test]
async fn main_success() {
    // Given: Developer creates in-memory event store
    let store = InMemoryEventStore::new();

    // And: Developer creates a stream ID for a bank account
    let account_id = StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id");

    // And: Developer creates a Deposit command
    let amount = MoneyAmount::try_new(100).expect("valid amount");
    let command = Deposit {
        account_id: account_id.clone(),
        amount,
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
        TestDomainEvents::MoneyWithdrawn { .. } => {
            panic!("deposit scenario should not produce withdrawal events");
        }
    }
}

/// Scenario 2: Developer handles business rule violations with proper errors.
#[tokio::test]
async fn insufficient_funds_returns_business_rule_violation() {
    // Given: Developer creates in-memory event store and account id
    let store = InMemoryEventStore::new();
    let account_id = StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id");

    // And: Developer records an initial deposit of 50 to establish balance
    let initial_amount = MoneyAmount::try_new(50).expect("valid amount");
    let seed_deposit = Deposit {
        account_id: account_id.clone(),
        amount: initial_amount,
    };
    execute(&store, seed_deposit)
        .await
        .expect("initial deposit to succeed");

    // And: Developer prepares a withdraw command that exceeds current balance
    let withdrawal_amount = MoneyAmount::try_new(100).expect("valid amount");
    let withdraw = Withdraw {
        account_id: account_id.clone(),
        amount: withdrawal_amount,
    };

    // When: Developer executes the withdraw command
    let error = match execute(&store, withdraw).await {
        Ok(_) => panic!("expected business rule violation but command succeeded"),
        Err(error) => error,
    };

    // Then: CommandError::BusinessRuleViolation is returned with actionable context
    let message = match error {
        CommandError::BusinessRuleViolation(message) => message,
        _ => panic!("expected business rule violation error"),
    };
    assert!(
        message.contains(account_id.as_ref()),
        "error should include account id"
    );
    assert!(
        message.contains("balance=50"),
        "error should include current balance"
    );
    assert!(
        message.contains("attempted_withdrawal=100"),
        "error should include attempted withdrawal amount"
    );

    // And: No additional events were appended (only the original deposit remains)
    let events = store
        .read_stream::<TestDomainEvents>(account_id)
        .await
        .expect("reading stream to succeed");
    assert_eq!(events.len(), 1, "failure should not append new events");
}

/// Scenario 3: Developer handles version conflict manually.
///
/// This test demonstrates EventCore's optimistic concurrency control when two
/// commands attempt to modify the same stream concurrently. Both commands read
/// the stream at version 0, but the first to write advances the version to 1.
/// When the second command attempts to write expecting version 1, it receives
/// a ConcurrencyError that the developer must handle manually.
#[tokio::test]
async fn concurrent_deposits_detect_version_conflict() {
    use std::sync::Arc;
    use tokio::sync::{Barrier, Mutex};

    // Test infrastructure: ControlledEventStore wrapper for deterministic concurrency testing
    struct ControlledEventStore {
        inner: InMemoryEventStore,
        // Barrier ensures both commands read at version 0 before either writes
        read_barrier: Arc<Barrier>,
        // Mutex controls which command writes first
        write_lock: Arc<Mutex<()>>,
    }

    impl ControlledEventStore {
        fn new() -> Self {
            Self {
                inner: InMemoryEventStore::new(),
                read_barrier: Arc::new(Barrier::new(2)), // 2 commands must sync
                write_lock: Arc::new(Mutex::new(())),
            }
        }
    }

    impl EventStore for ControlledEventStore {
        async fn read_stream<E: Event>(
            &self,
            stream_id: StreamId,
        ) -> Result<EventStreamReader<E>, EventStoreError> {
            // Delegate read to inner store
            let result = self.inner.read_stream(stream_id).await;

            // Synchronization point: both commands read before either writes
            self.read_barrier.wait().await;

            result
        }

        async fn append_events(
            &self,
            writes: StreamWrites,
        ) -> Result<EventStreamSlice, EventStoreError> {
            // Acquire write lock to ensure sequential writes
            let _guard = self.write_lock.lock().await;

            // Delegate append to inner store (which will check versions)
            self.inner.append_events(writes).await
        }
    }

    // Given: Developer creates controlled event store for deterministic concurrency
    let store = Arc::new(ControlledEventStore::new());

    // And: Developer creates a stream ID for a bank account
    let account_id = StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id");

    // And: Developer prepares two concurrent deposit commands on same account
    let amount = MoneyAmount::try_new(100).expect("valid amount");
    let command1 = Deposit {
        account_id: account_id.clone(),
        amount,
    };
    let command2 = Deposit {
        account_id: account_id.clone(),
        amount,
    };

    // When: Developer spawns two tasks executing deposits concurrently
    let store1 = store.clone();
    let store2 = store.clone();

    let task1 = tokio::spawn(async move { execute(&*store1, command1).await });

    let task2 = tokio::spawn(async move { execute(&*store2, command2).await });

    // And: Developer awaits both command executions
    let result1 = task1.await.expect("task1 should not panic");
    let result2 = task2.await.expect("task2 should not panic");

    // Then: Exactly one command succeeds and one gets ConcurrencyError
    let success_count = [&result1, &result2].iter().filter(|r| r.is_ok()).count();

    let concurrency_error_count = [&result1, &result2]
        .iter()
        .filter(|r| matches!(r, Err(CommandError::ConcurrencyError)))
        .count();

    assert_eq!(
        success_count, 1,
        "exactly one command should succeed, got results: {:?}, {:?}",
        result1, result2
    );
    assert_eq!(
        concurrency_error_count, 1,
        "exactly one command should fail with ConcurrencyError, got results: {:?}, {:?}",
        result1, result2
    );
}
