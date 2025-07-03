//! Benchmarks comparing EventCore's multi-stream approach vs traditional single-stream event sourcing.
//!
//! This benchmark demonstrates the performance characteristics and benefits of multi-stream
//! event sourcing compared to traditional aggregate-per-stream patterns.

#![allow(missing_docs)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::or_fun_call)]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use eventcore::{
    Command, CommandExecutor, CommandResult, EventId, EventMetadata, EventStore, EventToWrite,
    ExpectedVersion, ReadStreams, StoredEvent, StreamEvents, StreamId, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hint::black_box;
use uuid::Uuid;

// ============================================================================
// Shared Domain Types
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(String);

impl TransferId {
    pub fn generate() -> Self {
        Self(format!(
            "txn-{}",
            Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money(i64); // cents

impl Money {
    pub fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    pub fn subtract(&self, other: &Self) -> Option<Self> {
        if self.0 >= other.0 {
            Some(Self(self.0 - other.0))
        } else {
            None
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self(self.0 + other.0)
    }
}

// ============================================================================
// Traditional Single-Stream Event Sourcing Implementation
// ============================================================================

/// Traditional events - each aggregate has its own event types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraditionalAccountEvent {
    Opened {
        balance: Money,
    },
    Debited {
        amount: Money,
        transfer_id: TransferId,
    },
    Credited {
        amount: Money,
        transfer_id: TransferId,
    },
}

impl<'a> TryFrom<&'a serde_json::Value> for TraditionalAccountEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<TraditionalAccountEvent> for serde_json::Value {
    fn from(event: TraditionalAccountEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Traditional approach: separate commands for debit and credit
#[derive(Clone)]
pub struct TraditionalDebitCommand;

#[derive(Clone, Serialize, Deserialize)]
pub struct DebitInput {
    pub account_id: AccountId,
    pub amount: Money,
    pub transfer_id: TransferId,
}

#[derive(Clone)]
pub struct AccountState {
    pub balance: Money,
    pub exists: bool,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            balance: Money::from_cents(0),
            exists: false,
        }
    }
}

#[async_trait::async_trait]
impl Command for TraditionalDebitCommand {
    type Input = DebitInput;
    type State = AccountState;
    type Event = TraditionalAccountEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("account-{}", input.account_id.0)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TraditionalAccountEvent::Opened { balance } => {
                state.balance = *balance;
                state.exists = true;
            }
            TraditionalAccountEvent::Debited { amount, .. } => {
                state.balance = state
                    .balance
                    .subtract(amount)
                    .unwrap_or(Money::from_cents(0));
            }
            TraditionalAccountEvent::Credited { amount, .. } => {
                state.balance = state.balance.add(amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.exists {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Account does not exist".to_string(),
            ));
        }

        if state.balance.subtract(&input.amount).is_none() {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Insufficient funds".to_string(),
            ));
        }

        let event = TraditionalAccountEvent::Debited {
            amount: input.amount,
            transfer_id: input.transfer_id,
        };

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("account-{}", input.account_id.0)).unwrap(),
            event,
        )?])
    }
}

#[derive(Clone)]
pub struct TraditionalCreditCommand;

#[derive(Clone, Serialize, Deserialize)]
pub struct CreditInput {
    pub account_id: AccountId,
    pub amount: Money,
    pub transfer_id: TransferId,
}

#[async_trait::async_trait]
impl Command for TraditionalCreditCommand {
    type Input = CreditInput;
    type State = AccountState;
    type Event = TraditionalAccountEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![StreamId::try_new(format!("account-{}", input.account_id.0)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TraditionalAccountEvent::Opened { balance } => {
                state.balance = *balance;
                state.exists = true;
            }
            TraditionalAccountEvent::Debited { amount, .. } => {
                state.balance = state
                    .balance
                    .subtract(amount)
                    .unwrap_or(Money::from_cents(0));
            }
            TraditionalAccountEvent::Credited { amount, .. } => {
                state.balance = state.balance.add(amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.exists {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Account does not exist".to_string(),
            ));
        }

        let event = TraditionalAccountEvent::Credited {
            amount: input.amount,
            transfer_id: input.transfer_id,
        };

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("account-{}", input.account_id.0)).unwrap(),
            event,
        )?])
    }
}

// ============================================================================
// EventCore Multi-Stream Implementation
// ============================================================================

/// Multi-stream event - single event type for transfers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiStreamEvent {
    AccountOpened {
        account_id: AccountId,
        balance: Money,
    },
    MoneyTransferred {
        transfer_id: TransferId,
        from_account: AccountId,
        to_account: AccountId,
        amount: Money,
    },
}

impl<'a> TryFrom<&'a serde_json::Value> for MultiStreamEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<MultiStreamEvent> for serde_json::Value {
    fn from(event: MultiStreamEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Multi-stream approach: single command for entire transfer
#[derive(Clone)]
pub struct MultiStreamTransferCommand;

#[derive(Clone, Serialize, Deserialize)]
pub struct TransferInput {
    pub transfer_id: TransferId,
    pub from_account: AccountId,
    pub to_account: AccountId,
    pub amount: Money,
}

#[derive(Default, Clone)]
pub struct MultiStreamState {
    pub balances: HashMap<AccountId, Money>,
}

#[async_trait::async_trait]
impl Command for MultiStreamTransferCommand {
    type Input = TransferInput;
    type State = MultiStreamState;
    type Event = MultiStreamEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("account-{}", input.from_account.0)).unwrap(),
            StreamId::try_new(format!("account-{}", input.to_account.0)).unwrap(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            MultiStreamEvent::AccountOpened {
                account_id,
                balance,
            } => {
                state.balances.insert(account_id.clone(), *balance);
            }
            MultiStreamEvent::MoneyTransferred {
                from_account,
                to_account,
                amount,
                ..
            } => {
                if let Some(from_balance) = state.balances.get_mut(from_account) {
                    *from_balance = from_balance.subtract(amount).unwrap();
                }
                if let Some(to_balance) = state.balances.get_mut(to_account) {
                    *to_balance = to_balance.add(amount);
                }
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate both accounts exist
        let from_balance = state.balances.get(&input.from_account).ok_or_else(|| {
            eventcore::CommandError::BusinessRuleViolation(
                "Source account does not exist".to_string(),
            )
        })?;

        if !state.balances.contains_key(&input.to_account) {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Target account does not exist".to_string(),
            ));
        }

        // Validate sufficient funds
        if from_balance.subtract(&input.amount).is_none() {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Insufficient funds".to_string(),
            ));
        }

        let event = MultiStreamEvent::MoneyTransferred {
            transfer_id: input.transfer_id,
            from_account: input.from_account.clone(),
            to_account: input.to_account.clone(),
            amount: input.amount,
        };

        // Write atomically to both account streams
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.from_account.0)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.to_account.0)).unwrap(),
                event,
            )?,
        ])
    }
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Setup test accounts with initial balances
async fn setup_accounts(
    executor: &CommandExecutor<InMemoryEventStore<serde_json::Value>>,
    account_ids: &[AccountId],
    initial_balance: Money,
) {
    for account_id in account_ids {
        // Traditional approach
        let stream_id = StreamId::try_new(format!("account-{}", account_id.0)).unwrap();
        let event = EventToWrite::with_metadata(
            EventId::new(),
            serde_json::to_value(TraditionalAccountEvent::Opened {
                balance: initial_balance,
            })
            .unwrap(),
            EventMetadata::new(),
        );
        let stream_events = StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
        executor
            .event_store()
            .write_events_multi(vec![stream_events])
            .await
            .unwrap();

        // Multi-stream approach (same data, different event type)
        let event = EventToWrite::with_metadata(
            EventId::new(),
            serde_json::to_value(MultiStreamEvent::AccountOpened {
                account_id: account_id.clone(),
                balance: initial_balance,
            })
            .unwrap(),
            EventMetadata::new(),
        );
        let stream_events = StreamEvents::new(
            StreamId::try_new(format!("account-{}", account_id.0)).unwrap(),
            ExpectedVersion::Any,
            vec![event],
        );
        executor
            .event_store()
            .write_events_multi(vec![stream_events])
            .await
            .unwrap();
    }
}

/// Benchmark traditional two-command transfer pattern
fn bench_traditional_transfer(c: &mut Criterion) {
    let mut group = c.benchmark_group("traditional_vs_multistream");
    group.throughput(Throughput::Elements(1));

    group.bench_function("traditional_two_phase_transfer", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            runtime.block_on(async {
                let event_store = InMemoryEventStore::<serde_json::Value>::new();
                let executor = CommandExecutor::new(event_store);

                // Setup accounts
                let from_account = AccountId::new("traditional-from");
                let to_account = AccountId::new("traditional-to");
                setup_accounts(
                    &executor,
                    &[from_account.clone(), to_account.clone()],
                    Money::from_cents(10000),
                )
                .await;

                let transfer_id = TransferId::generate();
                let amount = Money::from_cents(1000);

                // Traditional approach: two separate commands
                let debit_cmd = TraditionalDebitCommand;
                let debit_input = DebitInput {
                    account_id: from_account,
                    amount,
                    transfer_id: transfer_id.clone(),
                };

                let credit_cmd = TraditionalCreditCommand;
                let credit_input = CreditInput {
                    account_id: to_account,
                    amount,
                    transfer_id,
                };

                // Execute both commands (simulating eventual consistency)
                let debit_result = executor
                    .execute(
                        &debit_cmd,
                        debit_input,
                        eventcore::ExecutionOptions::default(),
                    )
                    .await
                    .unwrap();

                let credit_result = executor
                    .execute(
                        &credit_cmd,
                        credit_input,
                        eventcore::ExecutionOptions::default(),
                    )
                    .await
                    .unwrap();

                black_box((debit_result, credit_result))
            })
        });
    });

    group.bench_function("multistream_atomic_transfer", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            runtime.block_on(async {
                let event_store = InMemoryEventStore::<serde_json::Value>::new();
                let executor = CommandExecutor::new(event_store);

                // Setup accounts
                let from_account = AccountId::new("multistream-from");
                let to_account = AccountId::new("multistream-to");
                setup_accounts(
                    &executor,
                    &[from_account.clone(), to_account.clone()],
                    Money::from_cents(10000),
                )
                .await;

                // Multi-stream approach: single atomic command
                let transfer_cmd = MultiStreamTransferCommand;
                let transfer_input = TransferInput {
                    transfer_id: TransferId::generate(),
                    from_account,
                    to_account,
                    amount: Money::from_cents(1000),
                };

                black_box(
                    executor
                        .execute(
                            &transfer_cmd,
                            transfer_input,
                            eventcore::ExecutionOptions::default(),
                        )
                        .await
                        .unwrap(),
                )
            })
        });
    });

    group.finish();
}

/// Benchmark performance with multiple concurrent accounts
fn bench_concurrent_account_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_operations");

    for num_accounts in [10, 50, 100] {
        group.throughput(Throughput::Elements(num_accounts as u64));

        group.bench_with_input(
            BenchmarkId::new("traditional_concurrent", num_accounts),
            &num_accounts,
            |b, &account_count| {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    runtime.block_on(async {
                        let event_store = InMemoryEventStore::<serde_json::Value>::new();
                        let executor = CommandExecutor::new(event_store);

                        // Setup many accounts
                        let accounts: Vec<AccountId> = (0..account_count)
                            .map(|i| AccountId::new(&format!("trad-acc-{i}")))
                            .collect();
                        setup_accounts(&executor, &accounts, Money::from_cents(10000)).await;

                        // Perform transfers between random pairs (traditional approach)
                        let debit_cmd = TraditionalDebitCommand;
                        let credit_cmd = TraditionalCreditCommand;

                        let from_idx = 0;
                        let to_idx = account_count - 1;
                        let transfer_id = TransferId::generate();

                        let debit_input = DebitInput {
                            account_id: accounts[from_idx].clone(),
                            amount: Money::from_cents(100),
                            transfer_id: transfer_id.clone(),
                        };

                        let credit_input = CreditInput {
                            account_id: accounts[to_idx].clone(),
                            amount: Money::from_cents(100),
                            transfer_id,
                        };

                        let debit = executor
                            .execute(
                                &debit_cmd,
                                debit_input,
                                eventcore::ExecutionOptions::default(),
                            )
                            .await
                            .unwrap();

                        let credit = executor
                            .execute(
                                &credit_cmd,
                                credit_input,
                                eventcore::ExecutionOptions::default(),
                            )
                            .await
                            .unwrap();

                        black_box((debit, credit))
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("multistream_concurrent", num_accounts),
            &num_accounts,
            |b, &account_count| {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    runtime.block_on(async {
                        let event_store = InMemoryEventStore::<serde_json::Value>::new();
                        let executor = CommandExecutor::new(event_store);

                        // Setup many accounts
                        let accounts: Vec<AccountId> = (0..account_count)
                            .map(|i| AccountId::new(&format!("multi-acc-{i}")))
                            .collect();
                        setup_accounts(&executor, &accounts, Money::from_cents(10000)).await;

                        // Single atomic transfer
                        let transfer_cmd = MultiStreamTransferCommand;
                        let transfer_input = TransferInput {
                            transfer_id: TransferId::generate(),
                            from_account: accounts[0].clone(),
                            to_account: accounts[account_count - 1].clone(),
                            amount: Money::from_cents(100),
                        };

                        black_box(
                            executor
                                .execute(
                                    &transfer_cmd,
                                    transfer_input,
                                    eventcore::ExecutionOptions::default(),
                                )
                                .await
                                .unwrap(),
                        )
                    })
                });
            },
        );
    }

    group.finish();
}

/// Multi-recipient transfer command for EventCore
#[derive(Clone)]
pub struct MultiStreamSplitPaymentCommand;

#[derive(Clone, Serialize, Deserialize)]
pub struct SplitPaymentInput {
    pub transfer_id: TransferId,
    pub source_account: AccountId,
    pub recipients: Vec<(AccountId, Money)>,
}

#[async_trait::async_trait]
impl Command for MultiStreamSplitPaymentCommand {
    type Input = SplitPaymentInput;
    type State = MultiStreamState;
    type Event = MultiStreamEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        let mut streams =
            vec![StreamId::try_new(format!("account-{}", input.source_account.0)).unwrap()];
        for (recipient, _) in &input.recipients {
            streams.push(StreamId::try_new(format!("account-{}", recipient.0)).unwrap());
        }
        streams
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            MultiStreamEvent::AccountOpened {
                account_id,
                balance,
            } => {
                state.balances.insert(account_id.clone(), *balance);
            }
            MultiStreamEvent::MoneyTransferred {
                from_account,
                to_account,
                amount,
                ..
            } => {
                if let Some(from_balance) = state.balances.get_mut(from_account) {
                    *from_balance = from_balance.subtract(amount).unwrap();
                }
                if let Some(to_balance) = state.balances.get_mut(to_account) {
                    *to_balance = to_balance.add(amount);
                }
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Calculate total amount
        let total_amount = input
            .recipients
            .iter()
            .map(|(_, amount)| amount.0)
            .sum::<i64>();
        let total_amount = Money::from_cents(total_amount);

        // Validate source account has sufficient funds
        let source_balance = state.balances.get(&input.source_account).ok_or_else(|| {
            eventcore::CommandError::BusinessRuleViolation(
                "Source account does not exist".to_string(),
            )
        })?;

        if source_balance.subtract(&total_amount).is_none() {
            return Err(eventcore::CommandError::BusinessRuleViolation(
                "Insufficient funds for split payment".to_string(),
            ));
        }

        // Validate all recipients exist
        for (recipient, _) in &input.recipients {
            if !state.balances.contains_key(recipient) {
                return Err(eventcore::CommandError::BusinessRuleViolation(format!(
                    "Recipient account {} does not exist",
                    recipient.0
                )));
            }
        }

        // Create events for all transfers atomically
        let mut events = Vec::new();
        for (recipient, amount) in input.recipients {
            let event = MultiStreamEvent::MoneyTransferred {
                transfer_id: input.transfer_id.clone(),
                from_account: input.source_account.clone(),
                to_account: recipient.clone(),
                amount,
            };

            // Write to source stream
            events.push(StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.source_account.0)).unwrap(),
                event.clone(),
            )?);

            // Write to recipient stream
            events.push(StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", recipient.0)).unwrap(),
                event,
            )?);
        }

        Ok(events)
    }
}

/// Benchmark complex workflows (e.g., multi-party transfers)
fn bench_complex_workflows(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_workflows");
    group.throughput(Throughput::Elements(1));

    // Scenario: Split payment among multiple recipients
    for num_recipients in [3, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("traditional_split_payment", num_recipients),
            &num_recipients,
            |b, &recipient_count| {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    runtime.block_on(async {
                        let event_store = InMemoryEventStore::<serde_json::Value>::new();
                        let executor = CommandExecutor::new(event_store);

                        // Setup source and recipient accounts
                        let source = AccountId::new("trad-split-source");
                        let recipients: Vec<AccountId> = (0..recipient_count)
                            .map(|i| AccountId::new(&format!("trad-split-recipient-{i}")))
                            .collect();

                        let mut all_accounts = vec![source.clone()];
                        all_accounts.extend(recipients.clone());
                        setup_accounts(&executor, &all_accounts, Money::from_cents(100_000)).await;

                        // Traditional: N+1 commands (1 debit + N credits)
                        let debit_cmd = TraditionalDebitCommand;
                        let credit_cmd = TraditionalCreditCommand;
                        let per_recipient = Money::from_cents(1000);
                        let total_amount =
                            Money::from_cents(per_recipient.0 * i64::from(recipient_count));

                        // First debit the source
                        let debit_input = DebitInput {
                            account_id: source,
                            amount: total_amount,
                            transfer_id: TransferId::generate(),
                        };

                        executor
                            .execute(
                                &debit_cmd,
                                debit_input,
                                eventcore::ExecutionOptions::default(),
                            )
                            .await
                            .unwrap();

                        // Then credit each recipient
                        for recipient in recipients {
                            let credit_input = CreditInput {
                                account_id: recipient,
                                amount: per_recipient,
                                transfer_id: TransferId::generate(),
                            };

                            executor
                                .execute(
                                    &credit_cmd,
                                    credit_input,
                                    eventcore::ExecutionOptions::default(),
                                )
                                .await
                                .unwrap();
                        }

                        black_box(())
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("multistream_split_payment", num_recipients),
            &num_recipients,
            |b, &recipient_count| {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    runtime.block_on(async {
                        let event_store = InMemoryEventStore::<serde_json::Value>::new();
                        let executor = CommandExecutor::new(event_store);

                        // Setup source and recipient accounts
                        let source = AccountId::new("multi-split-source");
                        let recipients: Vec<(AccountId, Money)> = (0..recipient_count)
                            .map(|i| {
                                (
                                    AccountId::new(&format!("multi-split-recipient-{i}")),
                                    Money::from_cents(1000),
                                )
                            })
                            .collect();

                        let mut all_accounts = vec![source.clone()];
                        all_accounts.extend(recipients.iter().map(|(acc, _)| acc.clone()));
                        setup_accounts(&executor, &all_accounts, Money::from_cents(100_000)).await;

                        // Multi-stream: single atomic command
                        let split_cmd = MultiStreamSplitPaymentCommand;
                        let split_input = SplitPaymentInput {
                            transfer_id: TransferId::generate(),
                            source_account: source,
                            recipients,
                        };

                        black_box(
                            executor
                                .execute(
                                    &split_cmd,
                                    split_input,
                                    eventcore::ExecutionOptions::default(),
                                )
                                .await
                                .unwrap(),
                        )
                    })
                });
            },
        );
    }

    group.finish();
}

/// Benchmark saga pattern vs multi-stream for complex workflows
fn bench_saga_vs_multistream(c: &mut Criterion) {
    let mut group = c.benchmark_group("saga_pattern_comparison");
    group.throughput(Throughput::Elements(1));

    // Scenario: Order fulfillment affecting inventory and shipping
    group.bench_function("traditional_saga_order_fulfillment", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            runtime.block_on(async {
                let event_store = InMemoryEventStore::<serde_json::Value>::new();
                let executor = CommandExecutor::new(event_store);

                // Traditional approach would require:
                // 1. Reserve inventory (command 1)
                // 2. Process payment (command 2)
                // 3. Create shipping (command 3)
                // 4. Update order status (command 4)
                // With saga coordination between them

                // Simulate the overhead of multiple commands
                let account = AccountId::new("saga-test-account");
                setup_accounts(&executor, &[account.clone()], Money::from_cents(10000)).await;

                // Execute multiple commands to simulate saga
                for _ in 0..4 {
                    let debit_cmd = TraditionalDebitCommand;
                    let debit_input = DebitInput {
                        account_id: account.clone(),
                        amount: Money::from_cents(100),
                        transfer_id: TransferId::generate(),
                    };

                    executor
                        .execute(
                            &debit_cmd,
                            debit_input,
                            eventcore::ExecutionOptions::default(),
                        )
                        .await
                        .unwrap();
                }

                black_box(())
            })
        });
    });

    group.bench_function("multistream_order_fulfillment", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            runtime.block_on(async {
                let event_store = InMemoryEventStore::<serde_json::Value>::new();
                let executor = CommandExecutor::new(event_store);

                // Multi-stream approach: single atomic command
                let from_account = AccountId::new("multi-order-customer");
                let to_account = AccountId::new("multi-order-merchant");
                setup_accounts(
                    &executor,
                    &[from_account.clone(), to_account.clone()],
                    Money::from_cents(10000),
                )
                .await;

                let transfer_cmd = MultiStreamTransferCommand;
                let transfer_input = TransferInput {
                    transfer_id: TransferId::generate(),
                    from_account,
                    to_account,
                    amount: Money::from_cents(400), // Same total as 4 saga steps
                };

                black_box(
                    executor
                        .execute(
                            &transfer_cmd,
                            transfer_input,
                            eventcore::ExecutionOptions::default(),
                        )
                        .await
                        .unwrap(),
                )
            })
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_traditional_transfer,
    bench_concurrent_account_operations,
    bench_complex_workflows,
    bench_saga_vs_multistream,
);
criterion_main!(benches);
