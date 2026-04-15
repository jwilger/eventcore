use eventcore::{
    Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy, StreamId, execute,
};
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// Domain Types
// =============================================================================

#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, Into, Serialize, Deserialize)
)]
pub struct MoneyAmount(u16);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BankAccountEvent {
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

// =============================================================================
// State
// =============================================================================

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccountBalance {
    cents: u16,
}

impl AccountBalance {
    fn apply(self, event: &BankAccountEvent) -> Self {
        match event {
            BankAccountEvent::MoneyDeposited { amount, .. } => Self {
                cents: self.cents.saturating_add((*amount).into()),
            },
            BankAccountEvent::MoneyWithdrawn { amount, .. } => Self {
                cents: self.cents.saturating_sub((*amount).into()),
            },
        }
    }
}

// =============================================================================
// Commands
// =============================================================================

/// Single-stream deposit: no state reconstruction needed.
#[derive(Clone, Command)]
pub struct Deposit {
    #[stream]
    pub account_id: StreamId,
    pub amount: MoneyAmount,
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

/// Single-stream withdrawal: requires state reconstruction for balance check.
#[derive(Clone, Command)]
pub struct Withdraw {
    #[stream]
    pub account_id: StreamId,
    pub amount: MoneyAmount,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum WithdrawError {
    #[error("insufficient-funds")]
    InsufficientFunds,
}

impl CommandLogic for Withdraw {
    type Event = BankAccountEvent;
    type State = AccountBalance;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.apply(event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if state.cents < self.amount.into() {
            return Err(CommandError::BusinessRuleViolation(Box::new(
                WithdrawError::InsufficientFunds,
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
// Transfer Types (Multi-Stream)
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferEvent {
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

    fn event_type_name() -> &'static str {
        "TransferEvent"
    }
}

/// Multi-stream atomic transfer across two accounts.
#[derive(Clone, Command)]
pub struct TransferMoney {
    #[stream]
    pub from: StreamId,
    #[stream]
    pub to: StreamId,
    pub amount: MoneyAmount,
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
// Helpers
// =============================================================================

pub fn new_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

pub fn test_amount(cents: u16) -> MoneyAmount {
    MoneyAmount::try_new(cents).expect("valid amount")
}

/// Seed a stream with N deposit events using execute().
pub async fn seed_stream<S: eventcore_types::EventStore + Sync>(
    store: &S,
    account_id: &StreamId,
    count: usize,
) {
    let amount = test_amount(100);
    for _ in 0..count {
        let cmd = Deposit {
            account_id: account_id.clone(),
            amount,
        };
        let _response = execute(store, cmd, RetryPolicy::new())
            .await
            .expect("seed deposit should succeed");
    }
}
