use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use eventcore::{
    event::Event,
    event_store::EventStore,
    metadata::EventMetadata,
    types::{EventId, EventVersion, StreamId},
};
use eventcore_memory::InMemoryEventStore;
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Benchmark single event writes
fn bench_single_event_writes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("single_event_writes");
    group.throughput(Throughput::Elements(1));

    group.bench_function("write_single_event", |b| {
        b.to_async(&rt).iter(|| async {
            let stream_id = StreamId::new(&format!("write-stream-{}", EventId::new())).unwrap();
            let test_event = String::from("test event data");

            let event = Event::new(stream_id.clone(), test_event, EventMetadata::new());

            black_box(
                event_store
                    .write_events(&stream_id, EventVersion::new(0).unwrap(), vec![event])
                    .await
                    .unwrap(),
            )
        });
    });

    group.finish();
}

/// Benchmark batch event writes
fn bench_batch_event_writes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("batch_event_writes");

    for batch_size in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(batch_size));

        group.bench_with_input(
            BenchmarkId::new("write_batch", batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let stream_id =
                        StreamId::new(&format!("batch-stream-{}", EventId::new())).unwrap();

                    let events: Vec<Event<String>> = (0..size)
                        .map(|i| {
                            Event::new(
                                stream_id.clone(),
                                format!("test event {}", i),
                                EventMetadata::new(),
                            )
                        })
                        .collect();

                    black_box(
                        event_store
                            .write_events(&stream_id, EventVersion::new(0).unwrap(), events)
                            .await
                            .unwrap(),
                    )
                });
            },
        );
    }
    group.finish();
}

/// Benchmark single stream reads
fn bench_single_stream_reads(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("single_stream_reads");

    for event_count in [10, 100, 1000] {
        group.throughput(Throughput::Elements(event_count));

        group.bench_with_input(
            BenchmarkId::new("read_stream", event_count),
            &event_count,
            |b, &count| {
                // Setup: populate the stream with events
                let stream_id = StreamId::new(&format!("read-stream-{}", EventId::new())).unwrap();

                rt.block_on(async {
                    let events: Vec<Event<String>> = (0..count)
                        .map(|i| {
                            Event::new(
                                stream_id.clone(),
                                format!("test event {}", i),
                                EventMetadata::new(),
                            )
                        })
                        .collect();

                    event_store
                        .write_events(&stream_id, EventVersion::new(0).unwrap(), events)
                        .await
                        .unwrap();
                });

                b.to_async(&rt).iter(|| async {
                    black_box(
                        event_store
                            .read_stream::<String>(&stream_id, EventVersion::new(0).unwrap())
                            .await
                            .unwrap(),
                    )
                });
            },
        );
    }
    group.finish();
}

/// Benchmark concurrent reads and writes
fn bench_concurrent_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("concurrent_operations");
    group.throughput(Throughput::Elements(1));

    for concurrency in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_writes", concurrency),
            &concurrency,
            |b, &concurrent_count| {
                let event_store = event_store.clone();

                b.to_async(&rt).iter(|| async move {
                    let tasks: Vec<_> = (0..concurrent_count)
                        .map(|i| {
                            let event_store = event_store.clone();
                            tokio::spawn(async move {
                                let stream_id = StreamId::new(&format!(
                                    "concurrent-write-{}-{}",
                                    i,
                                    EventId::new()
                                ))
                                .unwrap();

                                let event = Event::new(
                                    stream_id.clone(),
                                    format!("concurrent event {}", i),
                                    EventMetadata::new(),
                                );

                                event_store
                                    .write_events(
                                        &stream_id,
                                        EventVersion::new(0).unwrap(),
                                        vec![event],
                                    )
                                    .await
                            })
                        })
                        .collect();

                    let results = futures::future::join_all(tasks).await;
                    black_box(results)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_single_event_writes,
    bench_batch_event_writes,
    bench_single_stream_reads,
    bench_concurrent_operations,
);
criterion_main!(benches);
