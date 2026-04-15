use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use eventcore::{RetryPolicy, execute};
use eventcore_postgres::{MaxConnections, PostgresConfig, PostgresEventStore};

use crate::config::{BackendChoice, StressConfig};
use crate::domain::{Deposit, new_stream_id, test_amount};
use crate::metrics::MetricsReport;
use crate::runner::{OperationResult, run_stress_with_correctness};

/// Run the pool saturation scenario (postgres only).
/// Creates a postgres store with a very small connection pool and runs many
/// concurrent tasks to observe pool contention and timeout behavior.
pub async fn run(config: &StressConfig) -> Option<MetricsReport> {
    match config.backend {
        BackendChoice::Postgres => {}
        _ => {
            println!("Pool saturation scenario is postgres-only. Skipping.");
            return None;
        }
    }

    let conn = crate::backends::postgres_connection_string();

    // Create store with a very small pool (5 connections)
    let pg_config = PostgresConfig {
        max_connections: MaxConnections::new(std::num::NonZeroU32::new(5).expect("5 is non-zero")),
        acquire_timeout: Duration::from_secs(5),
        ..PostgresConfig::default()
    };

    let store = match PostgresEventStore::with_config(&conn, pg_config).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to connect to PostgreSQL: {e}");
            return None;
        }
    };

    store.migrate().await;

    let total_successes = Arc::new(AtomicU64::new(0));
    let store_ref = Arc::clone(&store);
    let success_ref = Arc::clone(&total_successes);

    // Override concurrency to 100 tasks against 5 connections
    let saturated_config = StressConfig {
        concurrency: 100,
        ..config.clone()
    };

    let report = run_stress_with_correctness(
        &saturated_config,
        "Pool Saturation (postgres, 5 conns x 100 tasks)",
        move |_task_idx| {
            let store = Arc::clone(&store_ref);
            let successes = Arc::clone(&success_ref);
            async move {
                let account_id = new_stream_id();
                let cmd = Deposit {
                    account_id,
                    amount: test_amount(1),
                };
                // Use a modest retry policy since pool timeouts are expected
                let policy = RetryPolicy::new().max_retries(3);
                let result = execute(store.as_ref(), cmd, policy).await;
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
                // Pool saturation expects some errors; just report
                let count = successes.load(Ordering::Relaxed);
                println!("Pool saturation: {count} successful operations out of attempted");
                // Don't fail correctness for pool saturation; just report
                true
            }
        },
    )
    .await;

    Some(report)
}
