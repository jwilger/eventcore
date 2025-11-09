use std::{
    convert::TryFrom,
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use eventcore::{
    CommandLogic, CommandStreams, Event, EventStore, EventStoreError, EventStreamReader,
    EventStreamSlice, InMemoryEventStore, NewEvents, RetryPolicy, StreamId, StreamVersion,
    StreamWrites, execute,
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
    Audit {
        account_id: StreamId,
    },
}

impl Event for TestDomainEvents {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestDomainEvents::Debited { account_id, .. }
            | TestDomainEvents::Credited { account_id, .. }
            | TestDomainEvents::Audit { account_id } => account_id,
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
    attempts: Option<NonZeroU32>,
    from_account: AccountSnapshot,
    to_account: AccountSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamSnapshot {
    source_events: Vec<TestDomainEvents>,
    destination_events: Vec<TestDomainEvents>,
}

struct SnapshottingStore {
    inner: Arc<InMemoryEventStore>,
    source_stream: StreamId,
    destination_stream: StreamId,
    snapshots: Arc<tokio::sync::Mutex<Vec<StreamSnapshot>>>,
}

impl SnapshottingStore {
    fn new(
        inner: Arc<InMemoryEventStore>,
        source_stream: StreamId,
        destination_stream: StreamId,
    ) -> Self {
        Self {
            inner,
            source_stream,
            destination_stream,
            snapshots: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    fn snapshots(&self) -> Arc<tokio::sync::Mutex<Vec<StreamSnapshot>>> {
        Arc::clone(&self.snapshots)
    }

    async fn record_snapshot(&self) {
        let source_events = self
            .inner
            .read_stream::<TestDomainEvents>(self.source_stream.clone())
            .await
            .expect("snapshotting store should read source stream after write");
        let destination_events = self
            .inner
            .read_stream::<TestDomainEvents>(self.destination_stream.clone())
            .await
            .expect("snapshotting store should read destination stream after write");

        let snapshot = StreamSnapshot {
            source_events: source_events.into_iter().collect(),
            destination_events: destination_events.into_iter().collect(),
        };

        let mut snapshots = self.snapshots.lock().await;
        snapshots.push(snapshot);
    }
}

impl EventStore for SnapshottingStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let result = self.inner.append_events(writes).await;
        if result.is_ok() {
            self.record_snapshot().await;
        }
        result
    }
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
        TestDomainEvents::Audit { .. } => current,
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

struct ConflictInjectingStore {
    inner: InMemoryEventStore,
    conflict_stream: StreamId,
    conflict_injected: Mutex<bool>,
}

impl ConflictInjectingStore {
    fn new(inner: InMemoryEventStore, conflict_stream: StreamId) -> Self {
        Self {
            inner,
            conflict_stream,
            conflict_injected: Mutex::new(false),
        }
    }
}

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

impl EventStore for ConflictInjectingStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<eventcore::EventStreamReader<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let should_inject = {
            let mut flag = self
                .conflict_injected
                .lock()
                .expect("conflict injector mutex must not be poisoned");

            if !*flag {
                *flag = true;
                true
            } else {
                false
            }
        };

        if should_inject {
            let current_events = self
                .inner
                .read_stream::<TestDomainEvents>(self.conflict_stream.clone())
                .await
                .expect("conflict injector should read target stream prior to injection");

            let expected_version = StreamVersion::new(current_events.len());
            let audit_event = TestDomainEvents::Audit {
                account_id: self.conflict_stream.clone(),
            };
            let writes_to_inject = StreamWrites::new().append(audit_event, expected_version);

            self.inner
                .append_events(writes_to_inject)
                .await
                .expect("conflict injector should append audit event");

            return Err(EventStoreError::VersionConflict);
        }

        self.inner.append_events(writes).await
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
    let attempts = result
        .as_ref()
        .ok()
        .and_then(|response| NonZeroU32::new(response.attempts()));
    let actual = TransferAcceptanceResult {
        succeeded: result.is_ok(),
        attempts,
        from_account: account_snapshot(&from_account, from_events.into_iter().collect()),
        to_account: account_snapshot(&to_account, to_events.into_iter().collect()),
    };

    let expected = TransferAcceptanceResult {
        succeeded: true,
        attempts: Some(NonZeroU32::new(1).unwrap()),
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
        "multi-stream transfer should succeed when funds are sufficient"
    );
}

/// Scenario 2: Transfer retried after conflict injected on destination stream.
#[tokio::test]
async fn transfer_retries_after_destination_conflict() {
    // Given: In-memory store with seeded accounts before wrapping in conflict injector.
    let base_store = InMemoryEventStore::new();
    let from_account = test_account_id();
    let to_account = test_account_id();
    let from_initial_balance = test_amount(100);
    let to_initial_balance = test_amount(50);

    seed_account_balance(&base_store, &from_account, from_initial_balance).await;
    seed_account_balance(&base_store, &to_account, to_initial_balance).await;

    let conflict_store = ConflictInjectingStore::new(base_store, to_account.clone());

    // When: Transfer is executed against conflict injecting store.
    let transfer_amount = test_amount(30);
    let command = TransferMoney {
        from: from_account.clone(),
        to: to_account.clone(),
        amount: transfer_amount,
    };

    let result = execute(&conflict_store, command, RetryPolicy::new()).await;

    // Then: Source reflects debit, destination reflects injected audit between deposit and credit.
    let from_events = conflict_store
        .read_stream::<TestDomainEvents>(from_account.clone())
        .await
        .expect("reading source account stream succeeds after retry");
    let to_events = conflict_store
        .read_stream::<TestDomainEvents>(to_account.clone())
        .await
        .expect("reading destination account stream succeeds after retry");

    let attempts = result
        .as_ref()
        .ok()
        .and_then(|response| NonZeroU32::new(response.attempts()));
    let actual = TransferAcceptanceResult {
        succeeded: result.is_ok(),
        attempts,
        from_account: account_snapshot(&from_account, from_events.into_iter().collect()),
        to_account: account_snapshot(&to_account, to_events.into_iter().collect()),
    };

    let expected = TransferAcceptanceResult {
        succeeded: true,
        attempts: Some(NonZeroU32::new(2).unwrap()),
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
                TestDomainEvents::Audit {
                    account_id: to_account.clone(),
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
        "retry logic should succeed after destination stream version conflict"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct PartialVisibilityEvidence {
    first_transfer_ok: bool,
    first_attempts_at_least_one: bool,
    second_transfer_ok: bool,
    second_attempts_at_least_one: bool,
    final_source_version: usize,
    final_destination_version: usize,
    final_source_balance: MoneyAmount,
    final_destination_balance: MoneyAmount,
    final_source_debits: Vec<u16>,
    final_destination_transfer_credits: Vec<u16>,
    event_counts_always_matched: bool,
    debit_credit_counts_balanced: bool,
}

/// Scenario 3: Concurrent transfers never expose partially applied changes across streams.
///
/// This test verifies the atomicity guarantee of multi-stream writes by running two
/// concurrent transfer commands while a background poller continuously reads both streams.
/// The test ensures that observers never see a debit without its corresponding credit
/// (or vice versa) - proving that multi-stream writes are truly atomic.
///
/// Test strategy:
/// 1. Seed two accounts with initial balances
/// 2. Start a background poller that continuously snapshots both streams
/// 3. Execute two concurrent transfer commands (racing for the same streams)
/// 4. Verify that every snapshot shows balanced debit/credit counts
/// 5. Verify final state shows both transfers completed successfully
///
/// The key invariant: At any point in time, the number of debits (after initial seed)
/// must equal the number of credits (after initial seed) across both streams. If this
/// invariant ever breaks, it means we observed a partial write.
#[tokio::test]
async fn concurrent_transfers_never_expose_partial_state() {
    // Given: Two accounts with initial balances
    let base_store = Arc::new(InMemoryEventStore::new());
    let from_account = test_account_id();
    let to_account = test_account_id();
    let from_initial_balance = test_amount(100);
    let to_initial_balance = test_amount(50);

    seed_account_balance(base_store.as_ref(), &from_account, from_initial_balance).await;
    seed_account_balance(base_store.as_ref(), &to_account, to_initial_balance).await;

    // And: A snapshotting store wrapper that records stream state after each write
    let snapshotting_store = Arc::new(SnapshottingStore::new(
        Arc::clone(&base_store),
        from_account.clone(),
        to_account.clone(),
    ));
    let snapshots = snapshotting_store.snapshots();

    // And: A background poller that continuously reads both streams to detect partial writes
    let stop_flag = Arc::new(AtomicBool::new(false));
    let poller_store = Arc::clone(&snapshotting_store);
    let poller_snapshots = Arc::clone(&snapshots);
    let poller_from_stream = from_account.clone();
    let poller_to_stream = to_account.clone();
    let poller_stop_flag = Arc::clone(&stop_flag);

    let poller_handle = tokio::spawn(async move {
        loop {
            if poller_stop_flag.load(Ordering::SeqCst) {
                break;
            }

            // Read both streams and record snapshot
            let source_events = poller_store
                .read_stream::<TestDomainEvents>(poller_from_stream.clone())
                .await
                .expect("poller should read source stream");
            let destination_events = poller_store
                .read_stream::<TestDomainEvents>(poller_to_stream.clone())
                .await
                .expect("poller should read destination stream");

            let snapshot = StreamSnapshot {
                source_events: source_events.into_iter().collect(),
                destination_events: destination_events.into_iter().collect(),
            };

            let mut guard = poller_snapshots.lock().await;
            guard.push(snapshot);

            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Give poller time to start before executing transfers
    tokio::time::sleep(Duration::from_millis(5)).await;

    // When: Execute two concurrent transfers that will race for the same streams
    // These transfers will conflict with each other, forcing retries and creating
    // opportunities for partial writes to be observed if atomicity is broken
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

    // Clone store references for concurrent execution
    let store_for_first = Arc::clone(&snapshotting_store);
    let store_for_second = Arc::clone(&snapshotting_store);

    // Execute both transfers concurrently - they will race and one will retry
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

    // Stop the background poller now that transfers are complete
    stop_flag.store(true, Ordering::SeqCst);
    poller_handle
        .await
        .expect("poller task should complete without panicking");

    // Then: Read final state of both streams to verify transfers completed
    let final_source_reader = snapshotting_store
        .read_stream::<TestDomainEvents>(from_account.clone())
        .await
        .expect("reading final source stream succeeds");
    let final_destination_reader = snapshotting_store
        .read_stream::<TestDomainEvents>(to_account.clone())
        .await
        .expect("reading final destination stream succeeds");

    let final_source_snapshot =
        account_snapshot(&from_account, final_source_reader.into_iter().collect());
    let final_destination_snapshot =
        account_snapshot(&to_account, final_destination_reader.into_iter().collect());

    // Retrieve all snapshots captured by the poller and snapshotting store
    let recorded_snapshots = {
        let guard = snapshots.lock().await;
        guard.clone()
    };

    // Verify atomicity invariant #1: Event counts always matched across streams
    // If we ever saw N events in source but M events in destination (N != M),
    // it means we observed a partial write
    let event_counts_always_matched = recorded_snapshots
        .iter()
        .all(|snapshot| snapshot.source_events.len() == snapshot.destination_events.len());

    // Verify atomicity invariant #2: Debit/credit counts always balanced
    // After skipping the initial seed deposits, the number of debits in the source
    // stream must always equal the number of credits in the destination stream.
    // If this ever breaks, we observed a debit without its corresponding credit.
    let debit_credit_counts_balanced = recorded_snapshots.iter().all(|snapshot| {
        let debited_after_initial = snapshot
            .source_events
            .iter()
            .enumerate()
            .skip(1) // Skip initial seed deposit
            .filter(|(_, event)| matches!(event, TestDomainEvents::Debited { .. }))
            .count();

        let credited_after_initial = snapshot
            .destination_events
            .iter()
            .enumerate()
            .skip(1) // Skip initial seed deposit
            .filter(|(_, event)| matches!(event, TestDomainEvents::Credited { .. }))
            .count();

        debited_after_initial == credited_after_initial
    });

    // Extract and sort debit amounts from source stream for final verification
    let mut final_source_debits: Vec<u16> = final_source_snapshot
        .events
        .iter()
        .filter_map(|event| match event {
            TestDomainEvents::Debited { amount, .. } => Some(amount.into_inner()),
            _ => None,
        })
        .collect();
    final_source_debits.sort_unstable();

    // Extract and sort credit amounts from destination stream (excluding initial seed)
    let mut final_destination_transfer_credits: Vec<u16> = final_destination_snapshot
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            TestDomainEvents::Credited { amount, .. } if index > 0 => Some(amount.into_inner()),
            _ => None,
        })
        .collect();
    final_destination_transfer_credits.sort_unstable();

    // Collect all evidence into a single struct for assertion
    let actual_analysis = PartialVisibilityEvidence {
        first_transfer_ok: first_result.is_ok(),
        first_attempts_at_least_one: first_result
            .as_ref()
            .ok()
            .map(|response| response.attempts() >= 1)
            .unwrap_or(false),
        second_transfer_ok: second_result.is_ok(),
        second_attempts_at_least_one: second_result
            .as_ref()
            .ok()
            .map(|response| response.attempts() >= 1)
            .unwrap_or(false),
        final_source_version: final_source_snapshot.version,
        final_destination_version: final_destination_snapshot.version,
        final_source_balance: final_source_snapshot.balance,
        final_destination_balance: final_destination_snapshot.balance,
        final_source_debits,
        final_destination_transfer_credits,
        event_counts_always_matched,
        debit_credit_counts_balanced,
    };

    let expected_analysis = PartialVisibilityEvidence {
        first_transfer_ok: true,
        first_attempts_at_least_one: true,
        second_transfer_ok: true,
        second_attempts_at_least_one: true,
        final_source_version: 3,
        final_destination_version: 3,
        final_source_balance: test_amount(30),
        final_destination_balance: test_amount(120),
        final_source_debits: vec![30, 40],
        final_destination_transfer_credits: vec![30, 40],
        event_counts_always_matched: true,
        debit_credit_counts_balanced: true,
    };

    assert_eq!(
        actual_analysis, expected_analysis,
        "concurrent transfers must never reveal partially applied state across streams"
    );
}
