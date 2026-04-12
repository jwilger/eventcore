//! Benchmarks for direct EventStore trait operations.
//!
//! Measures append_events and read_stream throughput, parameterized by backend
//! and event count. Isolates store performance from command execution overhead.

mod fixtures;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use eventcore_memory::InMemoryEventStore;
use eventcore_types::{EventStore, StreamVersion, StreamWrites};
use fixtures::{BankAccountEvent, new_stream_id, test_amount};
use std::hint::black_box;

// =============================================================================
// Helpers
// =============================================================================

/// Build a StreamWrites with N deposit events for a single stream at version 0.
fn build_writes(stream_id: &eventcore_types::StreamId, n: usize) -> StreamWrites {
    build_writes_at_version(stream_id, n, 0)
}

/// Build StreamWrites with N events, starting at the given expected version.
fn build_writes_at_version(
    stream_id: &eventcore_types::StreamId,
    n: usize,
    version: usize,
) -> StreamWrites {
    let amount = test_amount(100);
    let mut writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(version))
        .expect("register stream");
    for _ in 0..n {
        writes = writes
            .append(BankAccountEvent::MoneyDeposited {
                account_id: stream_id.clone(),
                amount,
            })
            .expect("append event");
    }
    writes
}

fn create_sqlite_store(rt: &tokio::runtime::Runtime) -> eventcore_sqlite::SqliteEventStore {
    let store = eventcore_sqlite::SqliteEventStore::in_memory().expect("create sqlite store");
    rt.block_on(async { store.migrate().await.expect("migrate") });
    store
}

/// Seed a store with N events on a stream, returning the stream ID.
fn seed_store(
    rt: &tokio::runtime::Runtime,
    store: &impl EventStore,
    n: usize,
) -> eventcore_types::StreamId {
    let stream_id = new_stream_id();
    rt.block_on(async {
        let batch_size = 100;
        let mut version = 0;
        let mut remaining = n;
        while remaining > 0 {
            let count = remaining.min(batch_size);
            let writes = build_writes_at_version(&stream_id, count, version);
            store
                .append_events(writes)
                .await
                .expect("seed should succeed");
            version += count;
            remaining -= count;
        }
    });
    stream_id
}

// =============================================================================
// Append Benchmarks
// =============================================================================

fn bench_append(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("store/append");

    for event_count in [1, 10, 100] {
        group.throughput(Throughput::Elements(event_count as u64));

        // In-memory backend: fresh store per iteration via iter_batched
        group.bench_with_input(
            BenchmarkId::new("memory", event_count),
            &event_count,
            |b, &n| {
                b.to_async(&rt).iter_batched(
                    || {
                        let store = InMemoryEventStore::new();
                        let writes = build_writes(&new_stream_id(), n);
                        (store, writes)
                    },
                    |(store, writes)| async move {
                        let _result = black_box(
                            store
                                .append_events(writes)
                                .await
                                .expect("append should succeed"),
                        );
                    },
                    criterion::BatchSize::PerIteration,
                );
            },
        );

        // SQLite backend: shared store, unique stream per iteration
        {
            let store = create_sqlite_store(&rt);
            group.bench_with_input(
                BenchmarkId::new("sqlite", event_count),
                &event_count,
                |b, &n| {
                    b.to_async(&rt).iter(|| {
                        let writes = build_writes(&new_stream_id(), n);
                        let store_ref = &store;
                        async move {
                            let _result = black_box(
                                store_ref
                                    .append_events(writes)
                                    .await
                                    .expect("append should succeed"),
                            );
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

// =============================================================================
// Read Stream Benchmarks
// =============================================================================

fn bench_read_stream(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("store/read_stream");

    for event_count in [10, 100, 1000] {
        group.throughput(Throughput::Elements(event_count as u64));

        // In-memory backend
        {
            let store = InMemoryEventStore::new();
            let stream_id = seed_store(&rt, &store, event_count);

            group.bench_with_input(
                BenchmarkId::new("memory", event_count),
                &event_count,
                |b, _n| {
                    b.to_async(&rt).iter(|| {
                        let sid = stream_id.clone();
                        let store_ref = &store;
                        async move {
                            let reader = black_box(
                                store_ref
                                    .read_stream::<BankAccountEvent>(sid)
                                    .await
                                    .expect("read should succeed"),
                            );
                            let _len = black_box(reader.len());
                        }
                    });
                },
            );
        }

        // SQLite backend
        {
            let store = create_sqlite_store(&rt);
            let stream_id = seed_store(&rt, &store, event_count);

            group.bench_with_input(
                BenchmarkId::new("sqlite", event_count),
                &event_count,
                |b, _n| {
                    b.to_async(&rt).iter(|| {
                        let sid = stream_id.clone();
                        let store_ref = &store;
                        async move {
                            let reader = black_box(
                                store_ref
                                    .read_stream::<BankAccountEvent>(sid)
                                    .await
                                    .expect("read should succeed"),
                            );
                            let _len = black_box(reader.len());
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

// =============================================================================
// PostgreSQL Benchmarks (conditional)
// =============================================================================

fn bench_append_postgres(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = match try_create_postgres_store(&rt) {
        Some(store) => store,
        None => return,
    };

    let mut group = c.benchmark_group("store/append");

    for event_count in [1, 10, 100] {
        group.throughput(Throughput::Elements(event_count as u64));
        group.bench_with_input(
            BenchmarkId::new("postgres", event_count),
            &event_count,
            |b, &n| {
                b.to_async(&rt).iter(|| {
                    let writes = build_writes(&new_stream_id(), n);
                    let store_ref = &store;
                    async move {
                        let _result = black_box(
                            store_ref
                                .append_events(writes)
                                .await
                                .expect("append should succeed"),
                        );
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_read_stream_postgres(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = match try_create_postgres_store(&rt) {
        Some(store) => store,
        None => return,
    };

    let mut group = c.benchmark_group("store/read_stream");

    for event_count in [10, 100, 1000] {
        group.throughput(Throughput::Elements(event_count as u64));

        let stream_id = seed_store(&rt, &store, event_count);

        group.bench_with_input(
            BenchmarkId::new("postgres", event_count),
            &event_count,
            |b, _n| {
                b.to_async(&rt).iter(|| {
                    let sid = stream_id.clone();
                    let store_ref = &store;
                    async move {
                        let reader = black_box(
                            store_ref
                                .read_stream::<BankAccountEvent>(sid)
                                .await
                                .expect("read should succeed"),
                        );
                        let _len = black_box(reader.len());
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Postgres Helpers
// =============================================================================

fn try_create_postgres_store(
    rt: &tokio::runtime::Runtime,
) -> Option<eventcore_postgres::PostgresEventStore> {
    let port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let url = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

    rt.block_on(async {
        match eventcore_postgres::PostgresEventStore::new(url).await {
            Ok(store) => {
                store.migrate().await;
                Some(store)
            }
            Err(e) => {
                eprintln!(
                    "Postgres not available (port {}), skipping postgres benchmarks: {}",
                    port, e
                );
                None
            }
        }
    })
}

// =============================================================================
// Criterion Harness
// =============================================================================

criterion_group!(
    benches,
    bench_append,
    bench_read_stream,
    bench_append_postgres,
    bench_read_stream_postgres,
);
criterion_main!(benches);
