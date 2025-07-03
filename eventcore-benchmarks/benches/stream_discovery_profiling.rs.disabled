//! Stream discovery loop profiling benchmarks for `EventCore` library.
//!
//! This benchmark specifically targets the stream discovery loop in the CommandExecutor
//! to identify bottlenecks in the dynamic stream resolution process. The loop is found
//! in executor.rs:680-786 and can iterate up to max_stream_discovery_iterations times.

#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::missing_const_for_fn)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    Command, CommandExecutor, CommandResult, EventId, EventMetadata, EventStore, EventToWrite,
    ExpectedVersion, ReadStreams, StoredEvent, StreamEvents, StreamId, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hint::black_box;

// ============================================================================
// Commands that stress the stream discovery loop
// ============================================================================

/// Event type for profiling
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEvent {
    pub data: String,
    pub sequence: u32,
}

impl<'a> TryFrom<&'a serde_json::Value> for ProfileEvent {
    type Error = serde_json::Error;

    fn try_from(value: &'a serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<ProfileEvent> for serde_json::Value {
    fn from(event: ProfileEvent) -> Self {
        serde_json::to_value(event).unwrap()
    }
}

/// Command that progressively discovers streams in each iteration
/// This stresses the stream discovery loop by requesting additional streams
/// after analyzing state from previously read streams.
#[derive(Debug, Clone)]
pub struct ProgressiveDiscoveryCommand {
    /// Maximum number of streams to discover
    max_streams: usize,
}

impl ProgressiveDiscoveryCommand {
    pub fn new(max_streams: usize) -> Self {
        Self { max_streams }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressiveInput {
    pub base_stream_pattern: String,
    pub target_stream_count: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ProgressiveState {
    /// Track discovered stream data
    pub stream_data: HashMap<StreamId, Vec<ProfileEvent>>,
    /// Track how many streams we've seen
    pub discovered_count: usize,
}

#[async_trait::async_trait]
impl Command for ProgressiveDiscoveryCommand {
    type Input = ProgressiveInput;
    type State = ProgressiveState;
    type Event = ProfileEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Start with just the first stream - will discover more during execution
        vec![StreamId::try_new(format!("{}-0", input.base_stream_pattern)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_events = state
            .stream_data
            .entry(event.stream_id.clone())
            .or_default();
        stream_events.push(event.payload.clone());

        // Count unique streams
        state.discovered_count = state.stream_data.len();
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Progressive discovery: request next stream based on current state
        let current_stream_count = state.discovered_count.max(1); // Start with at least 1
        let next_stream_index = current_stream_count;

        // If we haven't reached our target and haven't exceeded max iterations
        if next_stream_index < input.target_stream_count.min(self.max_streams) {
            let next_stream = StreamId::try_new(format!(
                "{}-{}",
                input.base_stream_pattern, next_stream_index
            ))
            .unwrap();

            // Request the next stream for discovery
            stream_resolver.add_streams(vec![next_stream]);

            // Return early to trigger stream discovery iteration
            return Ok(vec![]);
        }

        // Once we've discovered all streams, write a summary event
        let summary_event = ProfileEvent {
            data: format!("Discovered {} streams", state.discovered_count),
            sequence: state.discovered_count as u32,
        };

        let summary_stream =
            StreamId::try_new(format!("{}-summary", input.base_stream_pattern)).unwrap();

        Ok(vec![StreamWrite::new(
            &read_streams,
            summary_stream,
            summary_event,
        )?])
    }
}

/// Command that discovers a burst of streams in one iteration
/// This tests the overhead of discovering many streams at once
#[derive(Debug, Clone)]
pub struct BurstDiscoveryCommand {
    streams_per_burst: usize,
}

impl BurstDiscoveryCommand {
    pub fn new(streams_per_burst: usize) -> Self {
        Self { streams_per_burst }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstInput {
    pub base_stream_pattern: String,
}

#[derive(Debug, Default, Clone)]
pub struct BurstState {
    pub total_events: u32,
    pub streams_seen: HashMap<StreamId, u32>,
}

#[async_trait::async_trait]
impl Command for BurstDiscoveryCommand {
    type Input = BurstInput;
    type State = BurstState;
    type Event = ProfileEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Start with base stream
        vec![StreamId::try_new(format!("{}-base", input.base_stream_pattern)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.total_events += 1;
        *state
            .streams_seen
            .entry(event.stream_id.clone())
            .or_insert(0) += 1;
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // If this is the first iteration (only base stream), discover all at once
        if state.streams_seen.len() <= 1 {
            let burst_streams: Vec<StreamId> = (0..self.streams_per_burst)
                .map(|i| {
                    StreamId::try_new(format!("{}-burst-{}", input.base_stream_pattern, i)).unwrap()
                })
                .collect();

            stream_resolver.add_streams(burst_streams);
            return Ok(vec![]);
        }

        // Second iteration: write to all discovered streams
        let mut events = Vec::new();
        for (i, stream_id) in state.streams_seen.keys().enumerate() {
            if stream_id
                != &StreamId::try_new(format!("{}-base", input.base_stream_pattern)).unwrap()
            {
                let event = ProfileEvent {
                    data: format!("Burst discovery result {}", i),
                    sequence: i as u32,
                };
                events.push(StreamWrite::new(&read_streams, stream_id.clone(), event)?);
            }
        }

        Ok(events)
    }
}

/// Command that simulates the e-commerce cancel order scenario
/// This mimics the real-world pattern where an order's items determine
/// which product streams need to be read
#[derive(Debug, Clone)]
pub struct DynamicDependencyCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicInput {
    pub order_id: String,
    pub expected_item_count: usize,
}

#[derive(Debug, Default, Clone)]
pub struct DynamicState {
    pub order_items: HashMap<String, u32>, // product_id -> quantity
    pub product_data: HashMap<String, String>, // product_id -> data
}

#[async_trait::async_trait]
impl Command for DynamicDependencyCommand {
    type Input = DynamicInput;
    type State = DynamicState;
    type Event = ProfileEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Start by reading only the order stream
        vec![StreamId::try_new(format!("order-{}", input.order_id)).unwrap()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if event.stream_id.as_ref().starts_with("order-") {
            // Parse order events to extract item dependencies
            if event.payload.data.starts_with("item:") {
                let parts: Vec<&str> = event.payload.data.split(':').collect();
                if parts.len() >= 3 {
                    let product_id = parts[1].to_string();
                    let quantity = parts[2].parse().unwrap_or(1);
                    state.order_items.insert(product_id, quantity);
                }
            }
        } else if event.stream_id.as_ref().starts_with("product-") {
            // Collect product data
            let product_id = event.stream_id.as_ref().strip_prefix("product-").unwrap();
            state
                .product_data
                .insert(product_id.to_string(), event.payload.data.clone());
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // If we have items but not their product data, request product streams
        let missing_products: Vec<StreamId> = state
            .order_items
            .keys()
            .filter(|product_id| !state.product_data.contains_key(*product_id))
            .map(|product_id| StreamId::try_new(format!("product-{}", product_id)).unwrap())
            .collect();

        if !missing_products.is_empty() {
            stream_resolver.add_streams(missing_products);
            return Ok(vec![]);
        }

        // All dependencies resolved, process the command
        let result_event = ProfileEvent {
            data: format!(
                "Processed {} items with {} products",
                state.order_items.len(),
                state.product_data.len()
            ),
            sequence: state.order_items.len() as u32,
        };

        Ok(vec![StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
            result_event,
        )?])
    }
}

// ============================================================================
// Profiling Benchmarks
// ============================================================================

/// Profile progressive stream discovery (one stream per iteration)
fn bench_progressive_stream_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_progressive");

    // Test different numbers of discovery iterations
    for stream_count in [2_usize, 5, 10, 15] {
        group.throughput(Throughput::Elements(stream_count as u64));

        group.bench_with_input(
            BenchmarkId::new("progressive_discovery", stream_count),
            &stream_count,
            |b, &count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup: populate streams with some data to make state reconstruction realistic
                    for i in 0..count {
                        let stream_id = StreamId::try_new(format!("progressive-{}", i)).unwrap();
                        let event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(ProfileEvent {
                                data: format!("Initial data for stream {}", i),
                                sequence: 0,
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let stream_events =
                            StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Benchmark: command that progressively discovers streams
                    let command = ProgressiveDiscoveryCommand::new(count);
                    let input = ProgressiveInput {
                        base_stream_pattern: "progressive".to_string(),
                        target_stream_count: count,
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

/// Profile burst stream discovery (many streams in one iteration)  
fn bench_burst_stream_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_burst");

    // Test different burst sizes
    for burst_size in [5_usize, 10, 20, 50] {
        group.throughput(Throughput::Elements(burst_size as u64));

        group.bench_with_input(
            BenchmarkId::new("burst_discovery", burst_size),
            &burst_size,
            |b, &size| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup: create base stream
                    let base_stream = StreamId::try_new("burst-base".to_string()).unwrap();
                    let event = EventToWrite::with_metadata(
                        EventId::new(),
                        serde_json::to_value(ProfileEvent {
                            data: "Base stream data".to_string(),
                            sequence: 0,
                        })
                        .unwrap(),
                        EventMetadata::new(),
                    );
                    let stream_events =
                        StreamEvents::new(base_stream, ExpectedVersion::New, vec![event]);
                    executor
                        .event_store()
                        .write_events_multi(vec![stream_events])
                        .await
                        .unwrap();

                    // Setup: create burst target streams with some events
                    for i in 0..size {
                        let stream_id = StreamId::try_new(format!("burst-burst-{}", i)).unwrap();
                        let event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(ProfileEvent {
                                data: format!("Burst stream {} data", i),
                                sequence: i as u32,
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let stream_events =
                            StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Benchmark: command that discovers many streams at once
                    let command = BurstDiscoveryCommand::new(size);
                    let input = BurstInput {
                        base_stream_pattern: "burst".to_string(),
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

/// Profile dynamic dependency discovery (simulates real e-commerce patterns)
fn bench_dynamic_dependency_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_dynamic");

    // Test different numbers of dependencies
    for dependency_count in [3_usize, 8, 15, 25] {
        group.throughput(Throughput::Elements(dependency_count as u64));

        group.bench_with_input(
            BenchmarkId::new("dynamic_dependencies", dependency_count),
            &dependency_count,
            |b, &dep_count| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    let order_id = format!(
                        "order-{}",
                        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
                    );

                    // Setup: create order with item events
                    let order_stream = StreamId::try_new(format!("order-{}", order_id)).unwrap();
                    let mut order_events = vec![EventToWrite::with_metadata(
                        EventId::new(),
                        serde_json::to_value(ProfileEvent {
                            data: "Order created".to_string(),
                            sequence: 0,
                        })
                        .unwrap(),
                        EventMetadata::new(),
                    )];

                    // Add item events to order (these create dependencies)
                    for i in 0..dep_count {
                        order_events.push(EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(ProfileEvent {
                                data: format!("item:product-{}:2", i),
                                sequence: (i + 1) as u32,
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        ));
                    }

                    let order_stream_events =
                        StreamEvents::new(order_stream, ExpectedVersion::New, order_events);
                    executor
                        .event_store()
                        .write_events_multi(vec![order_stream_events])
                        .await
                        .unwrap();

                    // Setup: create product streams (the dependencies)
                    for i in 0..dep_count {
                        let product_stream = StreamId::try_new(format!("product-{}", i)).unwrap();
                        let event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(ProfileEvent {
                                data: format!("Product {} specifications", i),
                                sequence: 0,
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let stream_events =
                            StreamEvents::new(product_stream, ExpectedVersion::New, vec![event]);
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Benchmark: command that discovers dependencies dynamically
                    let command = DynamicDependencyCommand;
                    let input = DynamicInput {
                        order_id,
                        expected_item_count: dep_count,
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

/// Profile stream discovery with varying max iterations limit
fn bench_stream_discovery_iteration_limits(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_discovery_limits");

    // Test different iteration limits
    for max_iterations in [2_usize, 5, 10, 20] {
        group.throughput(Throughput::Elements(max_iterations as u64));

        group.bench_with_input(
            BenchmarkId::new("iteration_limit", max_iterations),
            &max_iterations,
            |b, &limit| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let event_store = InMemoryEventStore::<serde_json::Value>::new();
                    let executor = CommandExecutor::new(event_store);

                    // Setup streams for progressive discovery
                    for i in 0..limit {
                        let stream_id = StreamId::try_new(format!("limit-test-{}", i)).unwrap();
                        let event = EventToWrite::with_metadata(
                            EventId::new(),
                            serde_json::to_value(ProfileEvent {
                                data: format!("Stream {} data", i),
                                sequence: i as u32,
                            })
                            .unwrap(),
                            EventMetadata::new(),
                        );
                        let stream_events =
                            StreamEvents::new(stream_id, ExpectedVersion::New, vec![event]);
                        executor
                            .event_store()
                            .write_events_multi(vec![stream_events])
                            .await
                            .unwrap();
                    }

                    // Configure execution options with custom iteration limit
                    let options = eventcore::ExecutionOptions::default()
                        .with_max_stream_discovery_iterations(limit);

                    // Benchmark: progressive discovery with specific iteration limit
                    let command = ProgressiveDiscoveryCommand::new(limit);
                    let input = ProgressiveInput {
                        base_stream_pattern: "limit-test".to_string(),
                        target_stream_count: limit - 1, // Stay within limit
                    };

                    black_box(executor.execute(&command, input, options).await.unwrap())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_progressive_stream_discovery,
    bench_burst_stream_discovery,
    bench_dynamic_dependency_discovery,
    bench_stream_discovery_iteration_limits,
);
criterion_main!(benches);
