//! Integration tests for multi-stream atomic operations.
//!
//! This test demonstrates:
//! - Commands with multiple `#[stream]` attributes for multi-stream atomicity
//! - Atomic writes across multiple event streams
//! - Final state consistency after concurrent operations
//!
//! The core guarantee tested: When a command touches multiple streams, either
//! ALL events are written or NONE are.

use eventcore::{
    Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy, StreamId, execute,
    run_projection,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::EventCollector;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// =============================================================================
// Domain Types
// =============================================================================

/// A validated monetary amount in cents.
#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)
)]
struct MoneyAmount(u16);

/// Domain events for multi-stream bank account transfers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TransferEvent {
    Debited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
    Credited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
}

impl Event for TransferEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            TransferEvent::Debited { account_id, .. }
            | TransferEvent::Credited { account_id, .. } => account_id,
        }
    }
}

// =============================================================================
// Commands
// =============================================================================

/// Seed a single account with an initial balance.
#[derive(Command)]
struct SeedDeposit {
    #[stream]
    account_id: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for SeedDeposit {
    type Event = TransferEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![TransferEvent::Credited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

/// Transfer money between two accounts atomically.
///
/// Uses multiple `#[stream]` attributes to declare that this command
/// touches both the source and destination streams. EventCore guarantees
/// that either both the debit AND credit are written, or neither is.
#[derive(Command)]
struct TransferMoney {
    #[stream]
    from: StreamId,
    #[stream]
    to: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for TransferMoney {
    type Event = TransferEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![
            TransferEvent::Debited {
                account_id: self.from.clone(),
                amount: self.amount,
            },
            TransferEvent::Credited {
                account_id: self.to.clone(),
                amount: self.amount,
            },
        ]
        .into())
    }
}

// =============================================================================
// Test Helpers
// =============================================================================

fn test_account_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

fn test_amount(cents: u16) -> MoneyAmount {
    MoneyAmount::try_new(cents).expect("valid amount")
}

async fn seed_account_balance(
    store: &InMemoryEventStore,
    account_id: &StreamId,
    amount: MoneyAmount,
) {
    let command = SeedDeposit {
        account_id: account_id.clone(),
        amount,
    };

    execute(store, command, RetryPolicy::new())
        .await
        .expect("initial balance seed to succeed");
}

/// Compute balance from a list of events.
fn compute_balance(events: &[TransferEvent]) -> i32 {
    events.iter().fold(0i32, |current, event| match event {
        TransferEvent::Credited { amount, .. } => current + i32::from(amount.into_inner()),
        TransferEvent::Debited { amount, .. } => current - i32::from(amount.into_inner()),
    })
}

// =============================================================================
// Integration Tests
// =============================================================================

/// Scenario 1: Multi-stream transfer succeeds when funds are sufficient
///
/// Given: Two accounts with initial balances (source: 100, destination: 50)
/// When: A TransferMoney command debits 30 from source and credits to destination
/// Then: The command succeeds on first attempt
/// And: Both streams contain the expected events (debit in source, credit in destination)
/// And: Total money in the system is conserved
#[tokio::test]
async fn transfer_money_succeeds_when_funds_are_sufficient() {
    // Given: In-memory store with two seeded account streams
    let store = InMemoryEventStore::new();
    let from_account = test_account_id();
    let to_account = test_account_id();
    let from_initial_balance = test_amount(100);
    let to_initial_balance = test_amount(50);

    seed_account_balance(&store, &from_account, from_initial_balance).await;
    seed_account_balance(&store, &to_account, to_initial_balance).await;

    // When: Developer executes a multi-stream TransferMoney command
    let transfer_amount = test_amount(30);
    let command = TransferMoney {
        from: from_account.clone(),
        to: to_account.clone(),
        amount: transfer_amount,
    };

    let result = execute(&store, command, RetryPolicy::new()).await;

    // Then: Command succeeds on first attempt
    let response = result.expect("transfer command should succeed");
    assert_eq!(response.attempts(), 1, "should succeed on first attempt");

    // And: Collect all events via projection
    let storage: Arc<Mutex<Vec<TransferEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store)
        .await
        .expect("projection to complete");

    let events = storage.lock().unwrap();

    // And: Source stream has initial credit + transfer debit
    let source_events: Vec<&TransferEvent> = events
        .iter()
        .filter(|e| e.stream_id() == &from_account)
        .collect();
    assert_eq!(source_events.len(), 2, "source should have 2 events");
    assert!(
        matches!(
            source_events[0],
            TransferEvent::Credited { amount, .. } if *amount == from_initial_balance
        ),
        "first source event should be initial credit"
    );
    assert!(
        matches!(
            source_events[1],
            TransferEvent::Debited { amount, .. } if *amount == transfer_amount
        ),
        "second source event should be transfer debit"
    );

    // And: Destination stream has initial credit + transfer credit
    let dest_events: Vec<&TransferEvent> = events
        .iter()
        .filter(|e| e.stream_id() == &to_account)
        .collect();
    assert_eq!(dest_events.len(), 2, "destination should have 2 events");
    assert!(
        matches!(
            dest_events[0],
            TransferEvent::Credited { amount, .. } if *amount == to_initial_balance
        ),
        "first dest event should be initial credit"
    );
    assert!(
        matches!(
            dest_events[1],
            TransferEvent::Credited { amount, .. } if *amount == transfer_amount
        ),
        "second dest event should be transfer credit"
    );

    // And: Total money in the system is conserved (150 cents total)
    let total_balance = compute_balance(&events);
    assert_eq!(total_balance, 150, "total money should be conserved");
}

/// Scenario 2: Concurrent transfers produce consistent final state
///
/// Given: Two accounts with initial balances (source: 100, destination: 50)
/// When: Two concurrent transfer commands are executed (30 and 40 cents)
/// Then: Both transfers succeed (potentially with retries)
/// And: Final balances are correct (source: 30, destination: 120)
/// And: Total money in the system is conserved
/// And: Both debits and both credits are present in the event log
#[tokio::test]
async fn concurrent_transfers_produce_consistent_final_state() {
    // Given: Two accounts with initial balances
    let store = Arc::new(InMemoryEventStore::new());
    let from_account = test_account_id();
    let to_account = test_account_id();
    let from_initial_balance = test_amount(100);
    let to_initial_balance = test_amount(50);

    seed_account_balance(store.as_ref(), &from_account, from_initial_balance).await;
    seed_account_balance(store.as_ref(), &to_account, to_initial_balance).await;

    // When: Execute two concurrent transfers that will race for the same streams
    let first_transfer_amount = test_amount(30);
    let second_transfer_amount = test_amount(40);

    let first_command = TransferMoney {
        from: from_account.clone(),
        to: to_account.clone(),
        amount: first_transfer_amount,
    };

    let second_command = TransferMoney {
        from: from_account.clone(),
        to: to_account.clone(),
        amount: second_transfer_amount,
    };

    let store_for_first = Arc::clone(&store);
    let store_for_second = Arc::clone(&store);

    // Execute both transfers concurrently
    let (first_result, second_result) = tokio::join!(
        async move { execute(store_for_first.as_ref(), first_command, RetryPolicy::new()).await },
        async move {
            execute(
                store_for_second.as_ref(),
                second_command,
                RetryPolicy::new(),
            )
            .await
        }
    );

    // Then: Both transfers succeed
    let first_response = first_result.expect("first transfer should succeed");
    let second_response = second_result.expect("second transfer should succeed");

    // And: At least one attempt was made for each
    assert!(
        first_response.attempts() >= 1,
        "first transfer should have at least 1 attempt"
    );
    assert!(
        second_response.attempts() >= 1,
        "second transfer should have at least 1 attempt"
    );

    // And: Collect all events via projection
    let storage: Arc<Mutex<Vec<TransferEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, store.as_ref())
        .await
        .expect("projection to complete");

    let events = storage.lock().unwrap();

    // And: Source stream has correct events (initial credit + 2 debits)
    let source_events: Vec<&TransferEvent> = events
        .iter()
        .filter(|e| e.stream_id() == &from_account)
        .collect();
    assert_eq!(source_events.len(), 3, "source should have 3 events");

    let source_debit_amounts: Vec<u16> = source_events
        .iter()
        .filter_map(|e| match e {
            TransferEvent::Debited { amount, .. } => Some(amount.into_inner()),
            _ => None,
        })
        .collect();
    let mut sorted_debits = source_debit_amounts.clone();
    sorted_debits.sort();
    assert_eq!(
        sorted_debits,
        vec![30, 40],
        "source should have debits for both transfers"
    );

    // And: Destination stream has correct events (initial credit + 2 credits)
    let dest_events: Vec<&TransferEvent> = events
        .iter()
        .filter(|e| e.stream_id() == &to_account)
        .collect();
    assert_eq!(dest_events.len(), 3, "destination should have 3 events");

    let dest_credit_amounts: Vec<u16> = dest_events
        .iter()
        .filter_map(|e| match e {
            TransferEvent::Credited { amount, .. } => Some(amount.into_inner()),
            _ => None,
        })
        .collect();
    // First credit is the initial deposit (50), next two are transfers
    assert!(
        dest_credit_amounts.contains(&50),
        "destination should have initial credit"
    );
    assert!(
        dest_credit_amounts.contains(&30),
        "destination should have first transfer credit"
    );
    assert!(
        dest_credit_amounts.contains(&40),
        "destination should have second transfer credit"
    );

    // And: Final balances are correct
    let source_balance =
        compute_balance(&source_events.iter().copied().cloned().collect::<Vec<_>>());
    let dest_balance = compute_balance(&dest_events.iter().copied().cloned().collect::<Vec<_>>());
    assert_eq!(source_balance, 30, "source balance should be 30 cents");
    assert_eq!(dest_balance, 120, "destination balance should be 120 cents");

    // And: Total money in the system is conserved
    let total_balance = compute_balance(&events);
    assert_eq!(total_balance, 150, "total money should be conserved");
}
