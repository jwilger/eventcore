//! Memory allocation profiling benchmarks for `EventCore` library.

#![allow(missing_docs)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    Command, CommandExecutor, CommandResult, Event, EventId, EventMetadata, EventStore,
    EventToWrite, ExpectedVersion, ReadStreams, StoredEvent, StreamEvents, StreamId, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use std::collections::HashMap;
use std::hint::black_box;

/// Memory-heavy event for allocation testing
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct LargeEvent {
    id: String,
    data: Vec<u8>,
    metadata: HashMap<String, String>,
    nested_data: Vec<Vec<String>>,
}

impl LargeEvent {
    fn new(size_kb: usize) -> Self {
        let data_size = size_kb * 1024;
        let string_count = size_kb * 10; // Approximate

        Self {
            id: EventId::new().to_string(),
            data: vec![0u8; data_size],
            metadata: (0..string_count)
                .map(|i| (format!("key_{i}"), format!("value_{i}")))
                .collect(),
            nested_data: (0..string_count)
                .map(|i| (0..10).map(|j| format!("nested_{i}_{j}")).collect())
                .collect(),
        }
    }
}

impl<'a> TryFrom<&'a serde_json::Value> for LargeEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<LargeEvent> for serde_json::Value {
    fn from(event: LargeEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Command that creates memory-intensive events
#[derive(Clone)]
struct MemoryIntensiveCommand {
    event_size_kb: usize,
    event_count: usize,
}

impl MemoryIntensiveCommand {
    const fn new(event_size_kb: usize, event_count: usize) -> Self {
        Self {
            event_size_kb,
            event_count,
        }
    }
}

#[derive(Clone)]
struct MemoryIntensiveInput {
    target_stream: StreamId,
}

#[derive(Default, Clone)]
struct MemoryIntensiveState {
    total_events: usize,
    total_size_bytes: usize,
}

#[async_trait::async_trait]
impl Command for MemoryIntensiveCommand {
    type Input = MemoryIntensiveInput;
    type State = MemoryIntensiveState;
    type Event = LargeEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.target_stream.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.total_events += 1;
        state.total_size_bytes += event.payload.data.len()
            + event
                .payload
                .metadata
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
            + event
                .payload
                .nested_data
                .iter()
                .map(|v| v.iter().map(String::len).sum::<usize>())
                .sum::<usize>();
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let events: Result<Vec<_>, _> = (0..self.event_count)
            .map(|_| {
                StreamWrite::new(
                    &read_streams,
                    input.target_stream.clone(),
                    LargeEvent::new(self.event_size_kb),
                )
            })
            .collect();

        events
    }
}

/// Benchmark memory allocation patterns during event creation
fn bench_event_creation_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_creation_allocations");

    for size_kb in [1, 10, 100] {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("create_large_event", size_kb),
            &size_kb,
            |b, &size| {
                b.iter(|| {
                    let event = LargeEvent::new(size);
                    black_box(event)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark memory allocation during event serialization
fn bench_event_serialization_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_serialization_allocations");

    for size_kb in [1, 10, 100] {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("serialize_large_event", size_kb),
            &size_kb,
            |b, &size| {
                let stream_id = StreamId::try_new("test-stream").unwrap();
                let event = Event::new(stream_id, LargeEvent::new(size), EventMetadata::new());

                b.iter(|| {
                    let serialized = serde_json::to_vec(&event).unwrap();
                    black_box(serialized)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark memory allocation during command execution
fn bench_command_execution_allocations(c: &mut Criterion) {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    let mut group = c.benchmark_group("command_execution_allocations");

    for (event_size, event_count) in [(1, 10), (10, 5), (100, 1)] {
        group.throughput(Throughput::Bytes((event_size * event_count * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new(
                "execute_memory_command",
                format!("{event_size}kb_x{event_count}"),
            ),
            &(event_size, event_count),
            |b, &(size, count)| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let command = MemoryIntensiveCommand::new(size, count);
                    let stream_id =
                        StreamId::try_new(format!("mem-stream-{}", EventId::new())).unwrap();

                    let input = MemoryIntensiveInput {
                        target_stream: stream_id,
                    };

                    let result = executor.execute(&command, input, eventcore::ExecutionOptions::default()).await.unwrap();
                    black_box(result)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark memory allocation during event store operations
fn bench_event_store_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_store_allocations");

    for batch_size in [10, 50, 100] {
        group.throughput(Throughput::Elements(batch_size));

        group.bench_with_input(
            BenchmarkId::new("store_large_events", batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::new();
                    let stream_id =
                        StreamId::try_new(format!("alloc-stream-{}", EventId::new())).unwrap();

                    let events: Vec<EventToWrite<LargeEvent>> = (0..size)
                        .map(|_| {
                            EventToWrite::with_metadata(
                                EventId::new(),
                                LargeEvent::new(5), // 5KB events
                                EventMetadata::new(),
                            )
                        })
                        .collect();

                    let stream_events =
                        StreamEvents::new(stream_id.clone(), ExpectedVersion::New, events);

                    let result = event_store
                        .write_events_multi(vec![stream_events])
                        .await
                        .unwrap();

                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_event_creation_allocations,
    bench_event_serialization_allocations,
    bench_command_execution_allocations,
    bench_event_store_allocations,
);
criterion_main!(benches);
