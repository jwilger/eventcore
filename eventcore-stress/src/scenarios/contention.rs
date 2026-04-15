use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use eventcore::{RetryPolicy, StreamId, execute};
use eventcore_types::EventStore;

use crate::config::{BackendChoice, StressConfig};
use crate::domain::{Deposit, new_stream_id, test_amount};
use crate::metrics::{MetricsReport, RetryCounter};
use crate::runner::{OperationResult, run_stress_with_correctness};

/// Run the contention scenario: N tasks all deposit to a single shared stream.
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
    let shared_stream = Arc::new(new_stream_id());
    let retry_counter = RetryCounter::new();
    let success_count = Arc::new(AtomicU64::new(0));

    let store_ref = Arc::clone(&store);
    let stream_ref = Arc::clone(&shared_stream);
    let counter_ref = retry_counter.clone();
    let success_ref = Arc::clone(&success_count);

    run_stress_with_correctness(
        config,
        "Contention (single-stream)",
        move |_task_idx| {
            let store = Arc::clone(&store_ref);
            let stream = Arc::clone(&stream_ref);
            let counter = counter_ref.clone();
            let successes = Arc::clone(&success_ref);
            async move {
                let cmd = Deposit {
                    account_id: StreamId::clone(&stream),
                    amount: test_amount(1),
                };
                let policy = RetryPolicy::new()
                    .max_retries(20)
                    .with_metrics_hook(counter.clone());
                let retries_before = counter.count();
                let result = execute(store.as_ref(), cmd, policy).await;
                let retries_after = counter.count();
                let op_retries = retries_after.saturating_sub(retries_before);
                match result {
                    Ok(_) => {
                        successes.fetch_add(1, Ordering::Relaxed);
                        OperationResult {
                            success: true,
                            retries: op_retries,
                        }
                    }
                    Err(_) => OperationResult {
                        success: false,
                        retries: op_retries,
                    },
                }
            }
        },
        {
            let store = Arc::clone(&store);
            let stream = Arc::clone(&shared_stream);
            let successes = Arc::clone(&success_count);
            move |_reported_successes| async move {
                // Correctness: event count in the stream must equal successful ops
                let reader = store
                    .read_stream::<crate::domain::BankAccountEvent>(StreamId::clone(&stream))
                    .await;
                match reader {
                    Ok(r) => {
                        let event_count = r.len() as u64;
                        let expected = successes.load(Ordering::Relaxed);
                        if event_count != expected {
                            eprintln!(
                                "CORRECTNESS FAILURE: expected {expected} events, found {event_count}"
                            );
                            false
                        } else {
                            true
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to read stream for correctness check: {e}");
                        false
                    }
                }
            }
        },
    )
    .await
}
