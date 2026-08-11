//! Benchmarks for command execution via `eventcore::execute()`.
//!
//! Measures end-to-end command execution latency including stream reading,
//! state reconstruction, business rule evaluation, and event appending.

mod fixtures;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use eventcore::{RetryPolicy, execute};
use eventcore_memory::InMemoryEventStore;
use fixtures::{
    Deposit, DepositWithStateReconstruction, FixedHistoryStore, TransferMoney, new_stream_id,
    test_amount,
};
use std::hint::black_box;

// =============================================================================
// Single-Stream Benchmarks
// =============================================================================

fn bench_single_stream_cold(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("execute/single_stream");
    group.throughput(Throughput::Elements(1));

    // Cold start: first command on an empty stream (no state reconstruction)
    group.bench_function("memory/cold_start", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let store = InMemoryEventStore::new();
                let cmd = Deposit {
                    account_id: new_stream_id(),
                    amount: test_amount(100),
                };
                (store, cmd)
            },
            |(store, cmd)| async move {
                let _response = black_box(
                    execute(&store, cmd, RetryPolicy::new())
                        .await
                        .expect("execute should succeed"),
                );
            },
            criterion::BatchSize::PerIteration,
        );
    });

    group.finish();
}

fn bench_single_stream_warm(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("execute/single_stream/warm");

    // Warm stream: execute a DepositWithStateReconstruction (requires state
    // reconstruction via apply fold) against exactly N events. The fixed
    // history fixture discards appends so every sample has the same input.
    for event_count in [10, 100, 1000] {
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::new("memory", event_count),
            &event_count,
            |b, &event_count| {
                b.to_async(&rt).iter_batched(
                    || {
                        let account_id = new_stream_id();
                        let store = FixedHistoryStore::seeded(&account_id, event_count);
                        let cmd = DepositWithStateReconstruction {
                            account_id,
                            amount: test_amount(1),
                        };
                        (store, cmd)
                    },
                    |(store, cmd)| async move {
                        let _response = black_box(
                            execute(&store, cmd, RetryPolicy::new())
                                .await
                                .expect("execute should succeed"),
                        );
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }

    group.finish();
}

// =============================================================================
// Multi-Stream Benchmarks
// =============================================================================

fn bench_multi_stream(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("execute/multi_stream");
    group.throughput(Throughput::Elements(2)); // 2 events per transfer

    group.bench_function("memory/atomic_transfer", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let store = InMemoryEventStore::new();
                let cmd = TransferMoney {
                    from: new_stream_id(),
                    to: new_stream_id(),
                    amount: test_amount(100),
                };
                (store, cmd)
            },
            |(store, cmd)| async move {
                let _response = black_box(
                    execute(&store, cmd, RetryPolicy::new())
                        .await
                        .expect("execute should succeed"),
                );
            },
            criterion::BatchSize::PerIteration,
        );
    });

    group.finish();
}

// =============================================================================
// Criterion Harness
// =============================================================================

criterion_group!(
    benches,
    bench_single_stream_cold,
    bench_single_stream_warm,
    bench_multi_stream,
);
criterion_main!(benches);
