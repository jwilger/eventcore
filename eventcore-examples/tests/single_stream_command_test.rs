//! Integration test for single-stream command execution.
//!
//! This test demonstrates the core EventCore workflow:
//! - Defining commands with `#[derive(Command)]`
//! - Implementing `CommandLogic` for business rules
//! - Using `execute()` to run commands
//! - Using `EventCollector` with `run_projection()` for assertions

use eventcore::{
    Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy, execute, run_projection,
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
///
/// Using nutype ensures amounts are always positive, preventing invalid domain
/// states at the type level.
#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, Into, Serialize, Deserialize)
)]
struct MoneyAmount(u16);

/// Domain events for bank account aggregate.
///
/// Each variant carries its stream identity (account_id), enabling the Event
/// trait implementation to route events to the correct stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum BankAccountEvent {
    MoneyDeposited {
        account_id: eventcore::StreamId,
        amount: MoneyAmount,
    },
    MoneyWithdrawn {
        account_id: eventcore::StreamId,
        amount: MoneyAmount,
    },
}

impl Event for BankAccountEvent {
    fn stream_id(&self) -> &eventcore::StreamId {
        match self {
            BankAccountEvent::MoneyDeposited { account_id, .. }
            | BankAccountEvent::MoneyWithdrawn { account_id, .. } => account_id,
        }
    }
}

/// Reconstructed account state for command decision-making.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AccountBalance {
    cents: u16,
}

impl AccountBalance {
    fn deposit(mut self, amount: MoneyAmount) -> Self {
        self.cents = self.cents.saturating_add(amount.into());
        self
    }

    fn withdraw(mut self, amount: MoneyAmount) -> Self {
        self.cents = self.cents.saturating_sub(amount.into());
        self
    }

    fn has_sufficient_funds(&self, amount: MoneyAmount) -> bool {
        self.cents >= amount.into()
    }

    fn balance_cents(&self) -> u16 {
        self.cents
    }

    fn apply(self, event: &BankAccountEvent) -> Self {
        match event {
            BankAccountEvent::MoneyDeposited { amount, .. } => self.deposit(*amount),
            BankAccountEvent::MoneyWithdrawn { amount, .. } => self.withdraw(*amount),
        }
    }
}

// =============================================================================
// Commands
// =============================================================================

/// Deposit money into a bank account.
///
/// Uses `#[derive(Command)]` to generate the `CommandStreams` implementation,
/// eliminating boilerplate while keeping stream declarations explicit.
#[derive(Command)]
struct Deposit {
    #[stream]
    account_id: eventcore::StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Deposit {
    type Event = BankAccountEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![BankAccountEvent::MoneyDeposited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

/// Withdraw money from a bank account.
///
/// This command validates sufficient funds before emitting events,
/// demonstrating business rule enforcement.
#[derive(Command)]
struct Withdraw {
    #[stream]
    account_id: eventcore::StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Withdraw {
    type Event = BankAccountEvent;
    type State = AccountBalance;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.apply(event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if !state.has_sufficient_funds(self.amount) {
            let requested: u16 = self.amount.into();
            return Err(CommandError::BusinessRuleViolation(format!(
                "insufficient funds for account {}: balance={}, attempted_withdrawal={}",
                self.account_id.as_ref(),
                state.balance_cents(),
                requested
            )));
        }

        Ok(vec![BankAccountEvent::MoneyWithdrawn {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

// =============================================================================
// Test Helpers
// =============================================================================

fn test_account_id() -> eventcore::StreamId {
    eventcore::StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

fn test_amount(cents: u16) -> MoneyAmount {
    MoneyAmount::try_new(cents).expect("valid amount")
}

// =============================================================================
// Integration Tests
// =============================================================================

/// Scenario 1: Single-stream command emits events successfully
///
/// Given: A bank account stream
/// When: A deposit command is executed
/// Then: A MoneyDeposited event is emitted with correct data
#[tokio::test]
async fn deposit_command_emits_money_deposited_event() {
    // Given: An in-memory event store
    let store = InMemoryEventStore::new();

    // And: A bank account stream ID
    let account_id = test_account_id();

    // And: A deposit command
    let amount = test_amount(100);
    let command = Deposit {
        account_id: account_id.clone(),
        amount,
    };

    // When: The deposit command is executed
    execute(&store, command, RetryPolicy::new())
        .await
        .expect("command execution to succeed");

    // Then: Events can be collected via projection
    let storage: Arc<Mutex<Vec<BankAccountEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store)
        .await
        .expect("projection to complete");

    // And: Exactly one MoneyDeposited event was emitted
    let events = storage.lock().unwrap();
    assert_eq!(events.len(), 1, "expected exactly one event");

    // And: The event contains correct data
    match &events[0] {
        BankAccountEvent::MoneyDeposited {
            account_id: event_account_id,
            amount: event_amount,
        } => {
            assert_eq!(
                event_account_id, &account_id,
                "event should reference correct account"
            );
            assert_eq!(event_amount, &amount, "event should have correct amount");
        }
        _ => panic!("expected MoneyDeposited event"),
    }
}

/// Scenario 2: Business rule violations return proper errors
///
/// Given: A bank account with insufficient funds
/// When: A withdrawal exceeding the balance is attempted
/// Then: CommandError::BusinessRuleViolation is returned with actionable context
/// And: No events were appended on failure
#[tokio::test]
async fn insufficient_funds_returns_business_rule_violation() {
    // Given: An in-memory event store
    let store = InMemoryEventStore::new();

    // And: A bank account stream ID
    let account_id = test_account_id();

    // And: An initial deposit to establish balance
    let initial_amount = test_amount(50);
    let seed_deposit = Deposit {
        account_id: account_id.clone(),
        amount: initial_amount,
    };
    execute(&store, seed_deposit, RetryPolicy::new())
        .await
        .expect("initial deposit to succeed");

    // And: A withdrawal command that exceeds current balance
    let withdrawal_amount = test_amount(100);
    let withdraw = Withdraw {
        account_id: account_id.clone(),
        amount: withdrawal_amount,
    };

    // When: The withdrawal command is executed
    let error = match execute(&store, withdraw, RetryPolicy::new()).await {
        Ok(_) => panic!("expected business rule violation but command succeeded"),
        Err(error) => error,
    };

    // Then: CommandError::BusinessRuleViolation is returned
    let message = match error {
        CommandError::BusinessRuleViolation(message) => message,
        _ => panic!("expected BusinessRuleViolation error, got: {:?}", error),
    };

    // And: Error message contains actionable context
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

    // And: No additional events were appended (only the original deposit)
    let storage: Arc<Mutex<Vec<BankAccountEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store)
        .await
        .expect("projection to complete");

    let events = storage.lock().unwrap();
    assert_eq!(events.len(), 1, "failure should not append new events");
}
