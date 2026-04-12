//! Benchmarks for event serialization/deserialization.
//!
//! Isolates serde_json overhead from storage I/O to understand how much
//! of the total execute() latency is spent on (de)serialization.

mod fixtures;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fixtures::{BankAccountEvent, new_stream_id, test_amount};
use std::hint::black_box;

// =============================================================================
// Serialize
// =============================================================================

fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization/serialize");

    let event = BankAccountEvent::MoneyDeposited {
        account_id: new_stream_id(),
        amount: test_amount(100),
    };
    let json = serde_json::to_string(&event).expect("serialize for size measurement");
    group.throughput(Throughput::Bytes(json.len() as u64));

    group.bench_function("bank_account_event", |b| {
        b.iter(|| {
            let _json = black_box(serde_json::to_string(black_box(&event)).expect("serialize"));
        });
    });

    group.finish();
}

// =============================================================================
// Deserialize
// =============================================================================

fn bench_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization/deserialize");

    let event = BankAccountEvent::MoneyDeposited {
        account_id: new_stream_id(),
        amount: test_amount(100),
    };
    let json = serde_json::to_string(&event).expect("serialize for deserialization benchmark");
    group.throughput(Throughput::Bytes(json.len() as u64));

    group.bench_function("bank_account_event", |b| {
        b.iter(|| {
            let _event = black_box(
                serde_json::from_str::<BankAccountEvent>(black_box(&json)).expect("deserialize"),
            );
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Harness
// =============================================================================

criterion_group!(benches, bench_serialize, bench_deserialize);
criterion_main!(benches);
