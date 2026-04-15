use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use eventcore::{RetryPolicy, execute};
use eventcore_types::EventStore;

use crate::config::{BackendChoice, StressConfig};
use crate::domain::{Deposit, new_stream_id, test_amount};
use crate::metrics::MetricsReport;
use crate::runner::{OperationResult, run_stress_with_correctness};

/// Run the throughput scenario: each task deposits to its own unique stream
/// (minimal contention, measures raw store throughput).
pub async fn run(config: &StressConfig) -> MetricsReport {
    match config.backend {
        BackendChoice::Memory => {
            let store = Arc::new(eventcore_memory::InMemoryEventStore::new());
            run_inner(config, store).await
        }
        BackendChoice::Sqlite => {
            let store = Arc::new(
                eventcore_sqlite::SqliteEventStore::in_memory()
                    .expect("failed to create SQLite store"),
            );
            store.migrate().await.expect("SQLite migration failed");
            run_inner(config, store).await
        }
        BackendChoice::Postgres => {
            let conn = crate::backends::postgres_connection_string();
            let store = Arc::new(
                eventcore_postgres::PostgresEventStore::new(conn)
                    .await
                    .expect("failed to connect to PostgreSQL"),
            );
            store.migrate().await;
            run_inner(config, store).await
        }
    }
}

async fn run_inner<S>(config: &StressConfig, store: Arc<S>) -> MetricsReport
where
    S: EventStore + Sync + Send + 'static,
{
    // Pre-create a unique stream ID per task
    let concurrency = config.concurrency;
    let streams: Arc<Vec<_>> = Arc::new((0..concurrency).map(|_| new_stream_id()).collect());
    let total_successes = Arc::new(AtomicU64::new(0));

    let store_ref = Arc::clone(&store);
    let streams_ref = Arc::clone(&streams);
    let success_ref = Arc::clone(&total_successes);

    run_stress_with_correctness(
        config,
        "Throughput (per-task streams)",
        move |task_idx| {
            let store = Arc::clone(&store_ref);
            let streams = Arc::clone(&streams_ref);
            let successes = Arc::clone(&success_ref);
            async move {
                let account_id = streams[task_idx as usize].clone();
                let cmd = Deposit {
                    account_id,
                    amount: test_amount(1),
                };
                let result = execute(store.as_ref(), cmd, RetryPolicy::new()).await;
                match result {
                    Ok(_) => {
                        successes.fetch_add(1, Ordering::Relaxed);
                        OperationResult {
                            success: true,
                            retries: 0,
                        }
                    }
                    Err(_) => OperationResult {
                        success: false,
                        retries: 0,
                    },
                }
            }
        },
        {
            let successes = Arc::clone(&total_successes);
            move |_| async move {
                // Simple correctness: just verify we got some successes
                let count = successes.load(Ordering::Relaxed);
                if count == 0 {
                    eprintln!("CORRECTNESS FAILURE: zero successful operations");
                    false
                } else {
                    true
                }
            }
        },
    )
    .await
}
