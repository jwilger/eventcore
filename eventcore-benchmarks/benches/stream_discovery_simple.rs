//! Simplified stream discovery profiling for EventCore library.
//!
//! This benchmark creates a focused profiling target for the stream discovery loop
//! without complex type issues. Uses the same patterns as existing working benchmarks.

#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    Command, CommandExecutor, CommandResult, EventStore, ReadStreams, StoredEvent, StreamId,
    StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use std::collections::HashMap;
use std::hint::black_box;

/// Simple event for stream discovery profiling
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct SimpleEvent {
    stream_index: usize,
    data: String,
}

impl<'a> TryFrom<&'a serde_json::Value> for SimpleEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<SimpleEvent> for serde_json::Value {
    fn from(event: SimpleEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Command that progressively discovers streams to stress the discovery loop
#[derive(Clone)]
struct StreamDiscoveryCommand {
    max_discovery_rounds: usize,
}

impl StreamDiscoveryCommand {
    const fn new(max_rounds: usize) -> Self {
        Self {
            max_discovery_rounds: max_rounds,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct DiscoveryInput {
    base_name: String,
    target_streams: usize,
}

#[derive(Default, Clone)]
struct DiscoveryState {
    discovered_streams: HashMap<usize, String>,
    round_count: usize,
}

#[async_trait::async_trait]
impl Command for StreamDiscoveryCommand {
    type Input = DiscoveryInput;
    type State = DiscoveryState;
    type Event = SimpleEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Start with just the base stream
        vec![StreamId::try_new(format!("{}-0", input.base_name)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Track discovered streams based on events
        state
            .discovered_streams
            .insert(event.payload.stream_index, event.payload.data.clone());
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        mut state: Self::State,
        input: Self::Input,
        stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        state.round_count += 1;

        // Progressive discovery: add one new stream per iteration
        let current_count = state.discovered_streams.len().max(1);

        // If we haven't reached target and haven't exceeded max rounds, discover more
        if current_count < input.target_streams && state.round_count <= self.max_discovery_rounds {
            let next_stream =
                StreamId::try_new(format!("{}-{}", input.base_name, current_count)).unwrap();

            // Request additional stream - this will trigger another discovery iteration
            stream_resolver.add_streams(vec![next_stream]);
            return Ok(vec![]);
        }

        // Final iteration: write summary event
        let event = SimpleEvent {
            stream_index: state.discovered_streams.len(),
            data: format!(
                "Completed discovery of {} streams in {} rounds",
                state.discovered_streams.len(),
                state.round_count
            ),
        };

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("{}-summary", input.base_name)).unwrap(),
            event,
        )?])
    }
}

/// Profile progressive stream discovery with different iteration counts
fn bench_stream_discovery_iterations(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_iterations");

    // Test different numbers of discovery iterations to stress the loop
    for iteration_count in [2_usize, 5, 8, 12] {
        group.throughput(Throughput::Elements(iteration_count as u64));

        group.bench_with_input(
            BenchmarkId::new("progressive_discovery", iteration_count),
            &iteration_count,
            |b, &count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup: pre-populate the streams that will be discovered
                    for i in 0..count {
                        let stream_id = StreamId::try_new(format!("discovery-{}", i)).unwrap();
                        let event = eventcore::EventToWrite::with_metadata(
                            eventcore::EventId::new(),
                            serde_json::to_value(SimpleEvent {
                                stream_index: i,
                                data: format!("Initial data for stream {}", i),
                            })
                            .unwrap(),
                            eventcore::EventMetadata::new(),
                        );
                        let stream_events = eventcore::StreamEvents::new(
                            stream_id,
                            eventcore::ExpectedVersion::New,
                            vec![event],
                        );

                        use eventcore::EventStore;
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Also create the summary stream
                    let summary_stream =
                        StreamId::try_new("discovery-summary".to_string()).unwrap();
                    let summary_event = eventcore::EventToWrite::with_metadata(
                        eventcore::EventId::new(),
                        serde_json::to_value(SimpleEvent {
                            stream_index: 999,
                            data: "Summary placeholder".to_string(),
                        })
                        .unwrap(),
                        eventcore::EventMetadata::new(),
                    );
                    let summary_stream_events = eventcore::StreamEvents::new(
                        summary_stream,
                        eventcore::ExpectedVersion::New,
                        vec![summary_event],
                    );
                    executor
                        .event_store()
                        .write_events_multi(vec![summary_stream_events])
                        .await
                        .unwrap();

                    // Benchmark: Execute command that will progressively discover streams
                    let command = StreamDiscoveryCommand::new(count + 2); // Allow extra rounds
                    let input = DiscoveryInput {
                        base_name: "discovery".to_string(),
                        target_streams: count,
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

/// Profile stream discovery with varying max iteration limits
fn bench_stream_discovery_with_limits(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_limits");

    // Test how iteration limits affect performance
    for max_limit in [3_usize, 5, 10, 15] {
        group.throughput(Throughput::Elements(max_limit as u64));

        group.bench_with_input(
            BenchmarkId::new("limited_discovery", max_limit),
            &max_limit,
            |b, &limit| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup streams
                    for i in 0..limit {
                        let stream_id = StreamId::try_new(format!("limited-{}", i)).unwrap();
                        let event = eventcore::EventToWrite::with_metadata(
                            eventcore::EventId::new(),
                            serde_json::to_value(SimpleEvent {
                                stream_index: i,
                                data: format!("Limited stream {}", i),
                            })
                            .unwrap(),
                            eventcore::EventMetadata::new(),
                        );
                        let stream_events = eventcore::StreamEvents::new(
                            stream_id,
                            eventcore::ExpectedVersion::New,
                            vec![event],
                        );

                        use eventcore::EventStore;
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    let summary_stream = StreamId::try_new("limited-summary".to_string()).unwrap();
                    let summary_event = eventcore::EventToWrite::with_metadata(
                        eventcore::EventId::new(),
                        serde_json::to_value(SimpleEvent {
                            stream_index: 999,
                            data: "Summary".to_string(),
                        })
                        .unwrap(),
                        eventcore::EventMetadata::new(),
                    );
                    let summary_stream_events = eventcore::StreamEvents::new(
                        summary_stream,
                        eventcore::ExpectedVersion::New,
                        vec![summary_event],
                    );
                    executor
                        .event_store()
                        .write_events_multi(vec![summary_stream_events])
                        .await
                        .unwrap();

                    // Configure execution options with specific limit
                    let options = eventcore::ExecutionOptions::default()
                        .with_max_stream_discovery_iterations(limit);

                    let command = StreamDiscoveryCommand::new(limit);
                    let input = DiscoveryInput {
                        base_name: "limited".to_string(),
                        target_streams: limit - 1, // Stay just within limit
                    };

                    black_box(executor.execute(&command, input, options).await.unwrap())
                });
            },
        );
    }

    group.finish();
}

/// Intensive stream discovery for flamegraph profiling
/// This benchmark creates maximum stress on the stream discovery loop
fn bench_intensive_stream_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_intensive");
    group.sample_size(10); // Fewer samples for intensive profiling

    let stream_count = 20_usize; // High number to stress the loop
    group.throughput(Throughput::Elements(stream_count as u64));

    group.bench_function("intensive_discovery", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let event_store = InMemoryEventStore::<serde_json::Value>::new();
            let executor = CommandExecutor::new(event_store);

            // Setup: create many streams for discovery
            for i in 0..stream_count {
                let stream_id = StreamId::try_new(format!("intensive-{}", i)).unwrap();

                // Add multiple events per stream to make state reconstruction more expensive
                let mut events = Vec::new();
                for j in 0..5 {
                    events.push(eventcore::EventToWrite::with_metadata(
                        eventcore::EventId::new(),
                        serde_json::to_value(SimpleEvent {
                            stream_index: i,
                            data: format!("Stream {} event {}", i, j),
                        })
                        .unwrap(),
                        eventcore::EventMetadata::new(),
                    ));
                }

                let stream_events = eventcore::StreamEvents::new(
                    stream_id,
                    eventcore::ExpectedVersion::New,
                    events,
                );

                use eventcore::EventStore;
                executor
                    .event_store()
                    .write_events_multi(vec![stream_events])
                    .await
                    .unwrap();
            }

            let summary_stream = StreamId::try_new("intensive-summary".to_string()).unwrap();
            let summary_event = eventcore::EventToWrite::with_metadata(
                eventcore::EventId::new(),
                serde_json::to_value(SimpleEvent {
                    stream_index: 999,
                    data: "Intensive summary".to_string(),
                })
                .unwrap(),
                eventcore::EventMetadata::new(),
            );
            let summary_stream_events = eventcore::StreamEvents::new(
                summary_stream,
                eventcore::ExpectedVersion::New,
                vec![summary_event],
            );
            executor
                .event_store()
                .write_events_multi(vec![summary_stream_events])
                .await
                .unwrap();

            // Run with high iteration limit to allow full discovery
            let options = eventcore::ExecutionOptions::default()
                .with_max_stream_discovery_iterations(stream_count + 5);

            let command = StreamDiscoveryCommand::new(stream_count + 2);
            let input = DiscoveryInput {
                base_name: "intensive".to_string(),
                target_streams: stream_count,
            };

            black_box(executor.execute(&command, input, options).await.unwrap())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_stream_discovery_iterations,
    bench_stream_discovery_with_limits,
    bench_intensive_stream_discovery,
);
criterion_main!(benches);
