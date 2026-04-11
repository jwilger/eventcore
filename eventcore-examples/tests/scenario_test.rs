//! Integration tests demonstrating the GWT (Given-When-Then) test scenario API.

use eventcore::{Command, CommandError, CommandLogic, Event, NewEvents, StreamId};
use eventcore_testing::TestScenario;
use nutype::nutype;
use serde::{Deserialize, Serialize};

// =============================================================================
// Test Domain (minimal, just enough to exercise the scenario API)
// =============================================================================

#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, Into, Serialize, Deserialize)
)]
struct MoneyAmount(u16);

fn test_amount(cents: u16) -> MoneyAmount {
    MoneyAmount::try_new(cents).expect("valid amount")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum BankAccountEvent {
    MoneyDeposited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
    MoneyWithdrawn {
        account_id: StreamId,
        amount: MoneyAmount,
    },
}

impl Event for BankAccountEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            BankAccountEvent::MoneyDeposited { account_id, .. }
            | BankAccountEvent::MoneyWithdrawn { account_id, .. } => account_id,
        }
    }

    fn event_type_name() -> &'static str {
        "BankAccountEvent"
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct AccountBalance {
    cents: u16,
}

impl AccountBalance {
    fn deposit(mut self, amount: MoneyAmount) -> Self {
        self.cents = self.cents.saturating_add(amount.into());
        self
    }

    fn has_sufficient_funds(&self, amount: MoneyAmount) -> bool {
        self.cents >= amount.into()
    }
}

#[derive(Command)]
struct Deposit {
    #[stream]
    account_id: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Deposit {
    type Event = BankAccountEvent;
    type State = AccountBalance;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        match event {
            BankAccountEvent::MoneyDeposited { amount, .. } => state.deposit(*amount),
            _ => state,
        }
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![BankAccountEvent::MoneyDeposited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

#[derive(Command)]
struct Withdraw {
    #[stream]
    account_id: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for Withdraw {
    type Event = BankAccountEvent;
    type State = AccountBalance;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        match event {
            BankAccountEvent::MoneyDeposited { amount, .. } => state.deposit(*amount),
            _ => state,
        }
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if !state.has_sufficient_funds(self.amount) {
            return Err(CommandError::BusinessRuleViolation(
                "insufficient-funds".to_string(),
            ));
        }

        Ok(vec![BankAccountEvent::MoneyWithdrawn {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

// =============================================================================
// Scenario Tests
// =============================================================================

/// Given: An empty account
/// When: A deposit is made
/// Then: A MoneyDeposited event is emitted
#[tokio::test]
async fn deposit_into_empty_account() {
    let account_id = StreamId::try_new(uuid::Uuid::now_v7().to_string()).expect("valid stream id");
    let amount = test_amount(100);

    TestScenario::new()
        .when(Deposit {
            account_id: account_id.clone(),
            amount,
        })
        .await
        .succeeded()
        .then_events(vec![BankAccountEvent::MoneyDeposited {
            account_id,
            amount,
        }]);
}

/// Given: An account with a prior deposit
/// When: A withdrawal within the balance is made
/// Then: Both the original deposit and the new withdrawal events exist
#[tokio::test]
async fn withdraw_from_funded_account() {
    let account_id = StreamId::try_new(uuid::Uuid::now_v7().to_string()).expect("valid stream id");

    TestScenario::new()
        .given_events(
            account_id.clone(),
            vec![BankAccountEvent::MoneyDeposited {
                account_id: account_id.clone(),
                amount: test_amount(100),
            }],
        )
        .await
        .when(Withdraw {
            account_id: account_id.clone(),
            amount: test_amount(50),
        })
        .await
        .succeeded()
        .then_events(vec![
            BankAccountEvent::MoneyDeposited {
                account_id: account_id.clone(),
                amount: test_amount(100),
            },
            BankAccountEvent::MoneyWithdrawn {
                account_id,
                amount: test_amount(50),
            },
        ]);
}

/// Given: An account with insufficient funds
/// When: A withdrawal exceeding the balance is attempted
/// Then: A business rule violation is returned
/// And: Only the seed deposit exists (no withdrawal event)
#[tokio::test]
async fn withdraw_with_insufficient_funds() {
    let account_id = StreamId::try_new(uuid::Uuid::now_v7().to_string()).expect("valid stream id");

    TestScenario::new()
        .given_events(
            account_id.clone(),
            vec![BankAccountEvent::MoneyDeposited {
                account_id: account_id.clone(),
                amount: test_amount(50),
            }],
        )
        .await
        .when(Withdraw {
            account_id,
            amount: test_amount(100),
        })
        .await
        .failed_with_business_rule("insufficient-funds")
        .then_event_count(1);
}
