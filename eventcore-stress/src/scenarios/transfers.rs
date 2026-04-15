use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use eventcore::{RetryPolicy, StreamId, execute};
use eventcore_types::EventStore;
use rand::Rng;

use crate::config::{BackendChoice, StressConfig};
use crate::domain::{TransferEvent, TransferMoney, new_stream_id, test_amount};
use crate::metrics::{MetricsReport, RetryCounter};
use crate::runner::{OperationResult, run_stress_with_correctness};

/// Run the transfers scenario: concurrent multi-stream atomic transfers
/// between a pool of accounts.
pub async fn run(config: &StressConfig, num_accounts: u32) -> MetricsReport {
    match config.backend {
        BackendChoice::Memory => {
            let store = Arc::new(eventcore_memory::InMemoryEventStore::new());
            run_inner(config, store, num_accounts).await
        }
        BackendChoice::Sqlite => {
            let store = Arc::new(
                eventcore_sqlite::SqliteEventStore::in_memory()
                    .expect("failed to create SQLite store"),
            );
            store.migrate().await.expect("SQLite migration failed");
            run_inner(config, store, num_accounts).await
        }
        BackendChoice::Postgres => {
            let conn = crate::backends::postgres_connection_string();
            let store = Arc::new(
                eventcore_postgres::PostgresEventStore::new(conn)
                    .await
                    .expect("failed to connect to PostgreSQL"),
            );
            store.migrate().await;
            run_inner(config, store, num_accounts).await
        }
    }
}

async fn run_inner<S>(config: &StressConfig, store: Arc<S>, num_accounts: u32) -> MetricsReport
where
    S: EventStore + Sync + Send + 'static,
{
    // Create N account stream IDs
    let accounts: Arc<Vec<StreamId>> =
        Arc::new((0..num_accounts).map(|_| new_stream_id()).collect());

    let retry_counter = RetryCounter::new();
    let total_successes = Arc::new(AtomicU64::new(0));

    let store_ref = Arc::clone(&store);
    let accounts_ref = Arc::clone(&accounts);
    let counter_ref = retry_counter.clone();
    let success_ref = Arc::clone(&total_successes);

    run_stress_with_correctness(
        config,
        "Transfers (multi-stream)",
        move |_task_idx| {
            let store = Arc::clone(&store_ref);
            let accounts = Arc::clone(&accounts_ref);
            let counter = counter_ref.clone();
            let successes = Arc::clone(&success_ref);
            async move {
                let (from_idx, to_idx) = {
                    let mut rng = rand::rng();
                    let n = accounts.len();
                    let from_idx = rng.random_range(0..n);
                    let mut to_idx = rng.random_range(0..n);
                    while to_idx == from_idx {
                        to_idx = rng.random_range(0..n);
                    }
                    (from_idx, to_idx)
                };

                let cmd = TransferMoney {
                    from: accounts[from_idx].clone(),
                    to: accounts[to_idx].clone(),
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
            let accounts = Arc::clone(&accounts);
            move |_| async move {
                // Correctness: conservation of money.
                // Every successful transfer debits 1 from one account and credits 1 to
                // another, so the sum across all accounts should be 0.
                let mut total_balance: i64 = 0;
                for account_id in accounts.iter() {
                    let reader = store.read_stream::<TransferEvent>(account_id.clone()).await;
                    match reader {
                        Ok(r) => {
                            for event in r.into_iter() {
                                match event {
                                    TransferEvent::Credited { amount, .. } => {
                                        let amt: u16 = amount.into();
                                        total_balance += amt as i64;
                                    }
                                    TransferEvent::Debited { amount, .. } => {
                                        let amt: u16 = amount.into();
                                        total_balance -= amt as i64;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to read stream {} for correctness: {e}", account_id);
                            return false;
                        }
                    }
                }

                if total_balance != 0 {
                    eprintln!(
                        "CORRECTNESS FAILURE: money not conserved, net balance = {total_balance}"
                    );
                    false
                } else {
                    true
                }
            }
        },
    )
    .await
}
