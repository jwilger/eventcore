//! Command execution performance benchmarks for `EventCore` library.

#![allow(missing_docs)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    command::{Command, CommandResult},
    executor::CommandExecutor,
    types::StreamId,
};
use eventcore_memory::InMemoryEventStore;
use std::collections::HashMap;
use std::hint::black_box;

/// Test command for benchmarking that simulates real business logic
#[derive(Clone)]
struct BenchmarkCommand {
    computation_cycles: usize,
}

impl BenchmarkCommand {
    const fn new(computation_cycles: usize) -> Self {
        Self { computation_cycles }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct BenchmarkInput {
    target_stream: StreamId,
    value: i64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct BenchmarkEvent {
    value: i64,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl<'a> TryFrom<&'a serde_json::Value> for BenchmarkEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<BenchmarkEvent> for serde_json::Value {
    fn from(event: BenchmarkEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

#[derive(Default, Clone)]
struct BenchmarkState {
    total: i64,
    count: u64,
}

#[async_trait::async_trait]
impl Command for BenchmarkCommand {
    type Input = BenchmarkInput;
    type State = BenchmarkState;
    type Event = BenchmarkEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.target_stream.clone()]
    }

    fn apply(
        &self,
        state: &mut Self::State,
        event: &eventcore::event_store::StoredEvent<Self::Event>,
    ) {
        state.total += event.payload.value;
        state.count += 1;
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        // Simulate computation work
        let mut result = 0i64;
        for i in 0..self.computation_cycles {
            #[allow(clippy::cast_possible_wrap)]
            let i_as_i64 = i as i64;
            result = result.wrapping_add(i_as_i64.wrapping_mul(state.total + input.value));
        }

        let event = BenchmarkEvent {
            value: input.value + (result % 1000), // Use computation result to prevent optimization
            timestamp: chrono::Utc::now(),
        };

        Ok(vec![(input.target_stream, event)])
    }
}

/// Benchmark single-stream command execution
fn bench_single_stream_commands(c: &mut Criterion) {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    let mut group = c.benchmark_group("single_stream_commands");
    group.throughput(Throughput::Elements(1));

    for computation_cycles in [0, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("execution", computation_cycles),
            &computation_cycles,
            |b, &cycles| {
                let command = BenchmarkCommand::new(cycles);
                let stream_id = StreamId::try_new("bench-stream").unwrap();

                b.to_async(FuturesExecutor).iter(|| async {
                    let input = BenchmarkInput {
                        target_stream: stream_id.clone(),
                        value: black_box(42),
                    };

                    black_box(executor.execute(&command, input).await.unwrap())
                });
            },
        );
    }
    group.finish();
}

/// Multi-stream benchmark command
#[derive(Clone)]
struct MultiStreamBenchmarkCommand;

impl MultiStreamBenchmarkCommand {
    const fn new(_stream_count: usize) -> Self {
        Self
    }
}

#[derive(Clone)]
struct MultiStreamInput {
    streams: Vec<StreamId>,
    value: i64,
}

#[derive(Default, Clone)]
struct MultiStreamState {
    stream_totals: HashMap<StreamId, i64>,
}

#[async_trait::async_trait]
impl Command for MultiStreamBenchmarkCommand {
    type Input = MultiStreamInput;
    type State = MultiStreamState;
    type Event = BenchmarkEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        input.streams.clone()
    }

    fn apply(
        &self,
        state: &mut Self::State,
        event: &eventcore::event_store::StoredEvent<Self::Event>,
    ) {
        // For benchmarking, we'll update all streams
        let keys: Vec<_> = state.stream_totals.keys().cloned().collect();
        for stream in keys {
            if let Some(total) = state.stream_totals.get_mut(&stream) {
                *total += event.payload.value;
            }
        }
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        let total: i64 = state.stream_totals.values().sum();

        let event = BenchmarkEvent {
            value: input.value + total,
            timestamp: chrono::Utc::now(),
        };

        // Write to all streams for maximum stress testing
        let results = input
            .streams
            .into_iter()
            .map(|stream| (stream, event.clone()))
            .collect();

        Ok(results)
    }
}

/// Benchmark multi-stream command execution
fn bench_multi_stream_commands(c: &mut Criterion) {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    let mut group = c.benchmark_group("multi_stream_commands");
    group.throughput(Throughput::Elements(1));

    for num_streams in [2, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("execution", num_streams),
            &num_streams,
            |b, &stream_count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    // Create a command that reads from multiple streams
                    let command = MultiStreamBenchmarkCommand::new(stream_count);
                    let streams: Vec<StreamId> = (0..stream_count)
                        .map(|i| StreamId::try_new(format!("stream-{i}")).unwrap())
                        .collect();

                    let input = MultiStreamInput {
                        streams: streams.clone(),
                        value: black_box(42),
                    };

                    black_box(executor.execute(&command, input).await.unwrap())
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single_stream_commands,
    bench_multi_stream_commands,
);
criterion_main!(benches);
