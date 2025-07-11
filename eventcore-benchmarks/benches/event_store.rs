//! Event store performance benchmarks for `EventCore` library.

#![allow(missing_docs)]

use criterion::{
    async_executor::FuturesExecutor, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use eventcore::{
    EventId, EventMetadata, EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamEvents,
    StreamId,
};
use eventcore_memory::InMemoryEventStore;
use std::hint::black_box;
use std::sync::Arc;

/// Benchmark single event writes
fn bench_single_event_writes(c: &mut Criterion) {
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("single_event_writes");
    group.throughput(Throughput::Elements(1));

    group.bench_function("write_single_event", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let stream_id = StreamId::try_new(format!("write-stream-{}", EventId::new())).unwrap();
            let test_event = String::from("test event data");

            let event =
                EventToWrite::with_metadata(EventId::new(), test_event, EventMetadata::new());
            let stream_events =
                StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

            black_box(
                event_store
                    .write_events_multi(vec![stream_events])
                    .await
                    .unwrap(),
            )
        });
    });

    group.finish();
}

/// Benchmark batch event writes
fn bench_batch_event_writes(c: &mut Criterion) {
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("batch_event_writes");

    for batch_size in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(batch_size));

        group.bench_with_input(
            BenchmarkId::new("write_batch", batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(FuturesExecutor).iter(|| async {
                    let stream_id =
                        StreamId::try_new(format!("batch-stream-{}", EventId::new())).unwrap();

                    let events: Vec<EventToWrite<String>> = (0..size)
                        .map(|i| {
                            EventToWrite::with_metadata(
                                EventId::new(),
                                format!("test event {i}"),
                                EventMetadata::new(),
                            )
                        })
                        .collect();

                    let stream_events =
                        StreamEvents::new(stream_id.clone(), ExpectedVersion::New, events);

                    black_box(
                        event_store
                            .write_events_multi(vec![stream_events])
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
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("single_stream_reads");

    for event_count in [10, 100, 1000] {
        group.throughput(Throughput::Elements(event_count));

        group.bench_with_input(
            BenchmarkId::new("read_stream", event_count),
            &event_count,
            |b, &count| {
                // Setup: populate the stream with events
                let stream_id =
                    StreamId::try_new(format!("read-stream-{}", EventId::new())).unwrap();

                futures::executor::block_on(async {
                    let events: Vec<EventToWrite<String>> = (0..count)
                        .map(|i| {
                            EventToWrite::with_metadata(
                                EventId::new(),
                                format!("test event {i}"),
                                EventMetadata::new(),
                            )
                        })
                        .collect();

                    let stream_events =
                        StreamEvents::new(stream_id.clone(), ExpectedVersion::New, events);

                    event_store
                        .write_events_multi(vec![stream_events])
                        .await
                        .unwrap();
                });

                b.to_async(FuturesExecutor).iter(|| async {
                    black_box(
                        event_store
                            .read_streams(&[stream_id.clone()], &ReadOptions::new())
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
    let event_store = Arc::new(InMemoryEventStore::new());

    let mut group = c.benchmark_group("concurrent_operations");
    group.throughput(Throughput::Elements(1));

    for concurrency in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_writes", concurrency),
            &concurrency,
            |b, &concurrent_count| {
                let event_store = event_store.clone();

                b.to_async(FuturesExecutor).iter(|| {
                    let event_store = event_store.clone();
                    async move {
                        let tasks: Vec<_> = (0..concurrent_count)
                            .map(|i| {
                                let event_store = event_store.clone();
                                tokio::spawn(async move {
                                    let stream_id = StreamId::try_new(format!(
                                        "concurrent-write-{}-{}",
                                        i,
                                        EventId::new()
                                    ))
                                    .unwrap();

                                    let event = EventToWrite::with_metadata(
                                        EventId::new(),
                                        format!("concurrent event {i}"),
                                        EventMetadata::new(),
                                    );

                                    let stream_events = StreamEvents::new(
                                        stream_id.clone(),
                                        ExpectedVersion::New,
                                        vec![event],
                                    );

                                    event_store.write_events_multi(vec![stream_events]).await
                                })
                            })
                            .collect();

                        let results = futures::future::join_all(tasks).await;
                        black_box(results)
                    }
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
