//! Performance benchmarks for `PostgreSQL` event store implementation

#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use eventcore::{
    Command, CommandExecutor, CommandResult, EventStore, ExecutionOptions, ReadOptions,
    ReadStreams, StoredEvent, StreamId, StreamWrite,
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};
use tokio::runtime::Runtime;

// Test container setup
const POSTGRES_VERSION: &str = "16-alpine";
const POSTGRES_USER: &str = "postgres";
const POSTGRES_PASSWORD: &str = "postgres";
const POSTGRES_DB: &str = "eventcore_bench";

// Test events and commands (same as in integration tests)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum TestEvent {
    CounterIncremented { amount: u32 },
    CounterDecremented { amount: u32 },
    CounterReset,
}

// Implement required trait for CommandExecutor compatibility
impl<'a> TryFrom<&'a Self> for TestEvent {
    type Error = &'static str;

    fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// Note: No conversion implementations needed!
// The PostgreSQL adapter now handles serialization/deserialization automatically

#[derive(Debug, Default, Clone)]
struct CounterState {
    value: u32,
}

#[derive(Debug, Clone)]
struct IncrementCounterCommand;

#[derive(Debug, Clone)]
struct IncrementCounterInput {
    stream_id: StreamId,
    amount: u32,
}

#[async_trait]
impl Command for IncrementCounterCommand {
    type Input = IncrementCounterInput;
    type State = CounterState;
    type Event = TestEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        match &stored_event.payload {
            TestEvent::CounterIncremented { amount } => state.value += amount,
            TestEvent::CounterDecremented { amount } => {
                state.value = state.value.saturating_sub(*amount);
            }
            TestEvent::CounterReset => state.value = 0,
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        Ok(vec![StreamWrite::new(
            &read_streams,
            input.stream_id,
            TestEvent::CounterIncremented {
                amount: input.amount,
            },
        )?])
    }
}

#[derive(Debug, Clone)]
struct TransferBetweenCountersCommand;

#[derive(Debug, Clone)]
struct TransferBetweenCountersInput {
    from_stream: StreamId,
    to_stream: StreamId,
    amount: u32,
}

#[async_trait]
impl Command for TransferBetweenCountersCommand {
    type Input = TransferBetweenCountersInput;
    type State = (CounterState, CounterState);
    type Event = TestEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.from_stream.clone(), input.to_stream.clone()]
    }

    fn apply(&self, state: &mut Self::State, stored_event: &StoredEvent<Self::Event>) {
        match &stored_event.payload {
            TestEvent::CounterDecremented { amount } => {
                state.0.value = state.0.value.saturating_sub(*amount);
            }
            TestEvent::CounterIncremented { amount } => state.1.value += amount,
            TestEvent::CounterReset => {
                state.0.value = 0;
                state.1.value = 0;
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                input.from_stream,
                TestEvent::CounterDecremented {
                    amount: input.amount,
                },
            )?,
            StreamWrite::new(
                &read_streams,
                input.to_stream,
                TestEvent::CounterIncremented {
                    amount: input.amount,
                },
            )?,
        ])
    }
}

// Benchmark setup
struct BenchmarkContext {
    runtime: Runtime,
    executor: CommandExecutor<PostgresEventStore<TestEvent>>,
    _container: Box<ContainerAsync<GenericImage>>,
}

impl BenchmarkContext {
    fn new() -> Self {
        let runtime = Runtime::new().unwrap();

        let (container, executor) = runtime.block_on(async {
            let postgres_image = GenericImage::new("postgres", POSTGRES_VERSION).with_wait_for(
                WaitFor::message_on_stderr("database system is ready to accept connections"),
            );

            let container = postgres_image
                .with_env_var("POSTGRES_USER", POSTGRES_USER)
                .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
                .with_env_var("POSTGRES_DB", POSTGRES_DB)
                .start()
                .await
                .unwrap();
            let port = container.get_host_port_ipv4(5432).await.unwrap();

            let config = PostgresConfig::new(format!(
                "postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@localhost:{port}/{POSTGRES_DB}"
            ));

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let event_store: PostgresEventStore<TestEvent> =
                PostgresEventStore::new(config).await.unwrap();
            event_store.initialize().await.unwrap();

            let executor = CommandExecutor::new(event_store);

            (Box::new(container), executor)
        });

        Self {
            runtime,
            executor,
            _container: container,
        }
    }
}

fn bench_single_stream_commands(c: &mut Criterion) {
    let context = BenchmarkContext::new();
    let mut group = c.benchmark_group("single_stream_commands");

    for batch_size in &[1, 10, 100] {
        group.bench_with_input(
            BenchmarkId::new("sequential_writes", batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter_batched(
                    || {
                        // Setup: create a unique stream for each iteration
                        let stream_id = StreamId::try_new(format!(
                            "bench-{}",
                            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
                        ))
                        .unwrap();
                        let commands: Vec<_> = (0..batch_size)
                            .map(|i| IncrementCounterInput {
                                stream_id: stream_id.clone(),
                                amount: u32::try_from(i).unwrap() + 1,
                            })
                            .collect();
                        (stream_id, commands)
                    },
                    |(_stream_id, commands)| {
                        // Benchmark: execute commands sequentially
                        let executor_clone = context.executor.clone();
                        context.runtime.block_on(async move {
                            let command = IncrementCounterCommand;
                            for input in commands {
                                executor_clone
                                    .execute(&command, input, ExecutionOptions::default())
                                    .await
                                    .unwrap();
                            }
                        });
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_multi_stream_commands(c: &mut Criterion) {
    let context = BenchmarkContext::new();
    let mut group = c.benchmark_group("multi_stream_commands");

    // Setup initial streams with balance
    let executor_clone = context.executor.clone();
    context.runtime.block_on(async move {
        let command = IncrementCounterCommand;
        for i in 0..10 {
            let stream_id = StreamId::try_new(format!("transfer-source-{i}")).unwrap();
            executor_clone
                .execute(
                    &command,
                    IncrementCounterInput {
                        stream_id,
                        amount: 1000,
                    },
                    ExecutionOptions::default(),
                )
                .await
                .unwrap();
        }
    });

    group.bench_function("transfer_between_streams", |b| {
        let mut counter = 0;
        b.iter_batched(
            || {
                // Setup: prepare transfer command
                let from_idx = counter % 10;
                let to_idx = (counter + 1) % 10;
                counter += 1;

                TransferBetweenCountersInput {
                    from_stream: StreamId::try_new(format!("transfer-source-{from_idx}")).unwrap(),
                    to_stream: StreamId::try_new(format!("transfer-dest-{to_idx}")).unwrap(),
                    amount: 1,
                }
            },
            |input| {
                // Benchmark: execute transfer
                let executor_clone = context.executor.clone();
                context.runtime.block_on(async move {
                    let command = TransferBetweenCountersCommand;
                    executor_clone
                        .execute(&command, input, ExecutionOptions::default())
                        .await
                        .unwrap();
                });
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_concurrent_operations(c: &mut Criterion) {
    let context = BenchmarkContext::new();
    let mut group = c.benchmark_group("concurrent_operations");

    for num_concurrent in &[2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_writes_different_streams", num_concurrent),
            num_concurrent,
            |b, &num_concurrent| {
                let mut counter = 0;
                b.iter_batched(
                    || {
                        // Setup: create commands for different streams
                        let commands: Vec<_> = (0..num_concurrent)
                            .map(|i| {
                                let stream_id =
                                    StreamId::try_new(format!("concurrent-{counter}-{i}")).unwrap();
                                counter += 1;
                                IncrementCounterInput {
                                    stream_id,
                                    amount: 1,
                                }
                            })
                            .collect();
                        commands
                    },
                    |commands| {
                        // Benchmark: execute commands concurrently
                        let executor_clone = context.executor.clone();
                        context.runtime.block_on(async move {
                            let futures: Vec<_> = commands
                                .into_iter()
                                .map(|input| {
                                    let executor = executor_clone.clone();
                                    let command = IncrementCounterCommand;
                                    async move {
                                        executor
                                            .execute(&command, input, ExecutionOptions::default())
                                            .await
                                    }
                                })
                                .collect();

                            futures::future::join_all(futures).await;
                        });
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_event_store_operations(c: &mut Criterion) {
    let context = BenchmarkContext::new();
    let mut group = c.benchmark_group("event_store_operations");

    // Prepare streams with different event counts
    let executor_clone = context.executor.clone();
    context.runtime.block_on(async move {
        let command = IncrementCounterCommand;
        for size in [10, 100, 1000] {
            let stream_id = StreamId::try_new(format!("read-bench-{size}")).unwrap();
            for i in 0..size {
                executor_clone
                    .execute(
                        &command,
                        IncrementCounterInput {
                            stream_id: stream_id.clone(),
                            amount: u32::try_from(i).unwrap() + 1,
                        },
                        ExecutionOptions::default(),
                    )
                    .await
                    .unwrap();
            }
        }
    });

    for size in &[10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("read_stream", size), size, |b, &size| {
            let stream_id = StreamId::try_new(format!("read-bench-{size}")).unwrap();
            b.iter(|| {
                let stream_id_clone = stream_id.clone();
                let executor_clone = context.executor.clone();
                context.runtime.block_on(async move {
                    let event_store = executor_clone.event_store();
                    let read_options = ReadOptions::default();
                    event_store
                        .read_streams(&[stream_id_clone], &read_options)
                        .await
                        .unwrap();
                });
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_single_stream_commands,
    bench_multi_stream_commands,
    bench_concurrent_operations,
    bench_event_store_operations
);
criterion_main!(benches);
