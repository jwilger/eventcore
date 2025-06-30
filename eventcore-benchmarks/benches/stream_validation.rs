//! Stream validation performance benchmarks for `EventCore` library.
//!
//! This benchmark specifically measures the performance of stream validation
//! in `StreamWrite::new`, comparing O(n) vector search vs O(1) hash set lookup.

#![allow(missing_docs)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    Command, CommandExecutor, CommandResult, ReadStreams, StoredEvent, StreamId, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use std::hint::black_box;

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct TestEvent {
    value: i64,
    stream_index: usize,
}

impl<'a> TryFrom<&'a serde_json::Value> for TestEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<TestEvent> for serde_json::Value {
    fn from(event: TestEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Test command that exercises stream validation with many streams
#[derive(Clone)]
struct StreamValidationCommand;

#[derive(Clone)]
struct ValidationInput {
    streams: Vec<StreamId>,
    value: i64,
}

#[derive(Default, Clone)]
struct ValidationState;

#[async_trait::async_trait]
impl Command for StreamValidationCommand {
    type Input = ValidationInput;
    type State = ValidationState;
    type Event = TestEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        input.streams.clone()
    }

    fn apply(&self, _state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
        // No-op for benchmarking
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // This is the hot path we're benchmarking - creating many StreamWrite instances
        // which will exercise the O(1) vs O(n) validation logic
        let results: Result<Vec<_>, _> = input
            .streams
            .into_iter()
            .enumerate()
            .map(|(index, stream)| {
                StreamWrite::new(
                    &read_streams,
                    stream,
                    TestEvent {
                        value: input.value,
                        stream_index: index,
                    },
                )
            })
            .collect();

        results
    }
}

/// Benchmark stream validation performance with different numbers of streams
fn bench_stream_validation(c: &mut Criterion) {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    let mut group = c.benchmark_group("stream_validation");

    // Test with different numbers of streams to show O(n) vs O(1) difference
    for stream_count in [10, 50, 100, 200, 500] {
        #[allow(clippy::cast_sign_loss)]
        let throughput = stream_count as u64;
        group.throughput(Throughput::Elements(throughput));

        group.bench_with_input(
            BenchmarkId::new("multi_stream_writes", stream_count),
            &stream_count,
            |b, &count| {
                let command = StreamValidationCommand;
                let streams: Vec<StreamId> = (0..count)
                    .map(|i| StreamId::try_new(format!("stream-{i}")).unwrap())
                    .collect();

                b.to_async(FuturesExecutor).iter(|| async {
                    let input = ValidationInput {
                        streams: streams.clone(),
                        value: black_box(42),
                    };

                    black_box(
                        executor
                            .execute(&command, input, eventcore::ExecutionOptions::default())
                            .await
                            .unwrap(),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_stream_validation);
criterion_main!(benches);
