use std::convert::TryFrom;

use eventcore::{
    CommandLogic, CommandStreams, Event, EventStore, InMemoryEventStore, NewEvents, RetryPolicy,
    StreamId, execute,
};
use nutype::nutype;
use uuid::Uuid;

fn test_account_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

fn test_amount(cents: u16) -> MoneyAmount {
    MoneyAmount::try_new(cents).expect("valid amount")
}

#[nutype(validate(greater = 0), derive(Debug, Clone, Copy, PartialEq, Eq))]
struct MoneyAmount(u16);

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestDomainEvents {
    Debited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
    Credited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
}

impl Event for TestDomainEvents {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestDomainEvents::Debited { account_id, .. }
            | TestDomainEvents::Credited { account_id, .. } => account_id,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AccountSnapshot {
    stream_id: StreamId,
    version: usize,
    balance: MoneyAmount,
    events: Vec<TestDomainEvents>,
}

#[derive(Debug, PartialEq, Eq)]
struct TransferAcceptanceResult {
    succeeded: bool,
    from_account: AccountSnapshot,
    to_account: AccountSnapshot,
}

fn account_snapshot(stream_id: &StreamId, events: Vec<TestDomainEvents>) -> AccountSnapshot {
    AccountSnapshot {
        stream_id: stream_id.clone(),
        version: events.len(),
        balance: compute_balance(&events),
        events,
    }
}

fn compute_balance(events: &[TestDomainEvents]) -> MoneyAmount {
    let balance = events.iter().fold(0i32, |current, event| match event {
        TestDomainEvents::Credited { amount, .. } => current + i32::from(amount.into_inner()),
        TestDomainEvents::Debited { amount, .. } => current - i32::from(amount.into_inner()),
    });

    let non_negative_balance =
        u16::try_from(balance).expect("balance should never be negative in test scenario");
    MoneyAmount::try_new(non_negative_balance)
        .expect("balance should remain positive in test scenario")
}

struct SeedDeposit {
    account_id: StreamId,
    amount: MoneyAmount,
}

#[allow(dead_code)]
struct TransferMoney {
    from: StreamId,
    to: StreamId,
    amount: MoneyAmount,
}

impl CommandLogic for SeedDeposit {
    type Event = TestDomainEvents;
    type State = ();

    fn streams(&self) -> CommandStreams {
        CommandStreams::single(self.account_id.clone())
    }

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(
        &self,
        _state: Self::State,
    ) -> Result<NewEvents<Self::Event>, eventcore::CommandError> {
        Ok(vec![TestDomainEvents::Credited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

impl CommandLogic for TransferMoney {
    type Event = TestDomainEvents;
    type State = ();

    fn streams(&self) -> CommandStreams {
        CommandStreams::try_from_streams(vec![self.from.clone(), self.to.clone()])
            .expect("transfer command must declare unique streams")
    }

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(
        &self,
        _state: Self::State,
    ) -> Result<NewEvents<Self::Event>, eventcore::CommandError> {
        Ok(vec![
            TestDomainEvents::Debited {
                account_id: self.from.clone(),
                amount: self.amount,
            },
            TestDomainEvents::Credited {
                account_id: self.to.clone(),
                amount: self.amount,
            },
        ]
        .into())
    }
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

/// Scenario 1 (Happy Path): Multi-stream transfer succeeds when each account has sufficient funds.
#[tokio::test]
async fn transfer_money_succeeds_when_funds_are_sufficient() {
    // Given: In-memory store with two seeded account streams.
    let store = InMemoryEventStore::new();
    let from_account = test_account_id();
    let to_account = test_account_id();
    let from_initial_balance = test_amount(100);
    let to_initial_balance = test_amount(50);

    seed_account_balance(&store, &from_account, from_initial_balance).await;
    seed_account_balance(&store, &to_account, to_initial_balance).await;

    // When: Developer executes a multi-stream TransferMoney command.
    let transfer_amount = test_amount(30);
    let command = TransferMoney {
        from: from_account.clone(),
        to: to_account.clone(),
        amount: transfer_amount,
    };

    let result = execute(&store, command, RetryPolicy::new()).await;

    // And: Developer inspects both streams to verify debit/credit behavior and versions.
    let from_events = store
        .read_stream::<TestDomainEvents>(from_account.clone())
        .await
        .expect("reading source account stream succeeds");
    let to_events = store
        .read_stream::<TestDomainEvents>(to_account.clone())
        .await
        .expect("reading destination account stream succeeds");

    // Single assertion: struct comparison keeps one assert while inspecting both accounts.
    let actual = TransferAcceptanceResult {
        succeeded: result.is_ok(),
        from_account: account_snapshot(&from_account, from_events.into_iter().collect()),
        to_account: account_snapshot(&to_account, to_events.into_iter().collect()),
    };

    let expected = TransferAcceptanceResult {
        succeeded: true,
        from_account: account_snapshot(
            &from_account,
            vec![
                TestDomainEvents::Credited {
                    account_id: from_account.clone(),
                    amount: from_initial_balance,
                },
                TestDomainEvents::Debited {
                    account_id: from_account.clone(),
                    amount: transfer_amount,
                },
            ],
        ),
        to_account: account_snapshot(
            &to_account,
            vec![
                TestDomainEvents::Credited {
                    account_id: to_account.clone(),
                    amount: to_initial_balance,
                },
                TestDomainEvents::Credited {
                    account_id: to_account.clone(),
                    amount: transfer_amount,
                },
            ],
        ),
    };

    assert_eq!(
        actual, expected,
        "multi-stream transfer should debit source, credit destination, and advance streams"
    );
}
