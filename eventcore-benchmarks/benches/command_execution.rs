use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use eventcore::{
    command::{Command, CommandResult},
    event::Event,
    event_store::EventStore,
    executor::CommandExecutor,
    metadata::EventMetadata,
    types::{EventId, EventVersion, StreamId},
};
use eventcore_memory::InMemoryEventStore;
use std::hint::black_box;
use std::{collections::HashMap, sync::Arc};
use tokio::runtime::Runtime;

/// Test command for benchmarking that simulates real business logic
#[derive(Clone)]
struct BenchmarkCommand {
    computation_cycles: usize,
}

impl BenchmarkCommand {
    fn new(computation_cycles: usize) -> Self {
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

    fn apply(&self, state: &mut Self::State, event: &Self::Event) {
        state.total += event.value;
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
            result = result.wrapping_add((i as i64).wrapping_mul(state.total + input.value));
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
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());
    let executor = CommandExecutor::new(event_store.clone());

    let mut group = c.benchmark_group("single_stream_commands");
    group.throughput(Throughput::Elements(1));

    for computation_cycles in [0, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("execution", computation_cycles),
            &computation_cycles,
            |b, &cycles| {
                let command = BenchmarkCommand::new(cycles);
                let stream_id = StreamId::new("bench-stream").unwrap();

                b.to_async(&rt).iter(|| async {
                    let input = BenchmarkInput {
                        target_stream: stream_id.clone(),
                        value: black_box(42),
                    };

                    let metadata = EventMetadata::new();

                    black_box(executor.execute(&command, input, metadata).await.unwrap())
                });
            },
        );
    }
    group.finish();
}

/// Multi-stream benchmark command
#[derive(Clone)]
struct MultiStreamBenchmarkCommand {
    stream_count: usize,
}

impl MultiStreamBenchmarkCommand {
    fn new(stream_count: usize) -> Self {
        Self { stream_count }
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

    fn apply(&self, state: &mut Self::State, event: &Self::Event) {
        // For benchmarking, we'll update all streams
        for stream in state.stream_totals.keys() {
            if let Some(total) = state.stream_totals.get_mut(stream) {
                *total += event.value;
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
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());
    let executor = CommandExecutor::new(event_store.clone());

    let mut group = c.benchmark_group("multi_stream_commands");
    group.throughput(Throughput::Elements(1));

    for num_streams in [2, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("execution", num_streams),
            &num_streams,
            |b, &stream_count| {
                b.to_async(&rt).iter(|| async {
                    // Create a command that reads from multiple streams
                    let command = MultiStreamBenchmarkCommand::new(stream_count);
                    let streams: Vec<StreamId> = (0..stream_count)
                        .map(|i| StreamId::new(&format!("stream-{}", i)).unwrap())
                        .collect();

                    let input = MultiStreamInput {
                        streams: streams.clone(),
                        value: black_box(42),
                    };

                    let metadata = EventMetadata::new();

                    black_box(executor.execute(&command, input, metadata).await.unwrap())
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
