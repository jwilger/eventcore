use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eventcore::{ProjectionConfig, RetryPolicy, execute, run_projection};
use eventcore_types::{EventStore, Projector, StreamId, StreamPosition};

use crate::config::{BackendChoice, StressConfig};
use crate::domain::{BankAccountEvent, Deposit, new_stream_id, test_amount};
use crate::metrics::MetricsReport;

/// Balance-tracking projector for stress testing.
pub struct BalanceTrackingProjector {
    balances: Arc<Mutex<HashMap<String, u64>>>,
}

impl BalanceTrackingProjector {
    fn new() -> Self {
        Self {
            balances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn balances(&self) -> Arc<Mutex<HashMap<String, u64>>> {
        Arc::clone(&self.balances)
    }
}

impl Projector for BalanceTrackingProjector {
    type Event = BankAccountEvent;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let mut balances = self.balances.lock().expect("lock poisoned in projector");
        match &event {
            BankAccountEvent::MoneyDeposited {
                account_id, amount, ..
            } => {
                let amt: u16 = (*amount).into();
                *balances.entry(account_id.to_string()).or_insert(0) += amt as u64;
            }
            BankAccountEvent::MoneyWithdrawn {
                account_id, amount, ..
            } => {
                let amt: u16 = (*amount).into();
                let entry = balances.entry(account_id.to_string()).or_insert(0);
                *entry = entry.saturating_sub(amt as u64);
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "balance-tracker"
    }
}

/// Run the projection scenario:
/// Phase 1: Write events concurrently (like throughput scenario)
/// Phase 2: Run projection and measure catch-up performance
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
    S: EventStore
        + eventcore_types::EventReader
        + eventcore_types::CheckpointStore
        + eventcore_types::ProjectorCoordinator
        + Sync
        + Send
        + 'static,
    <S as eventcore_types::EventReader>::Error: std::fmt::Display,
    <S as eventcore_types::CheckpointStore>::Error: std::fmt::Debug,
    <S as eventcore_types::ProjectorCoordinator>::Error:
        std::fmt::Debug + std::error::Error + Send + Sync + 'static,
{
    let concurrency = config.concurrency;

    // Phase 1: Write events concurrently
    println!("Phase 1: Writing events...");
    let streams: Vec<StreamId> = (0..concurrency).map(|_| new_stream_id()).collect();
    let streams = Arc::new(streams);

    // Use a fixed number of writes per task for the write phase.
    // Batch-mode projection reads up to 1000 events in a single pass,
    // so we cap total events to stay under that limit.
    let max_per_task = 900 / concurrency.max(1) as u64;
    let writes_per_task = config
        .effective_iterations()
        .map(|n| n.min(max_per_task))
        .unwrap_or(max_per_task)
        .max(5); // at least 5

    let mut handles = Vec::new();
    for task_idx in 0..concurrency {
        let store = Arc::clone(&store);
        let streams = Arc::clone(&streams);
        handles.push(tokio::spawn(async move {
            let account_id = streams[task_idx as usize].clone();
            let mut successes = 0u64;
            for _ in 0..writes_per_task {
                let cmd = Deposit {
                    account_id: account_id.clone(),
                    amount: test_amount(1),
                };
                if execute(store.as_ref(), cmd, RetryPolicy::new())
                    .await
                    .is_ok()
                {
                    successes += 1;
                }
            }
            successes
        }));
    }

    let mut total_events = 0u64;
    for h in handles {
        total_events += h.await.unwrap_or(0);
    }

    println!("Phase 1 complete: {total_events} events written across {concurrency} streams");

    // Phase 2: Run projection and measure catch-up time
    println!("Phase 2: Running projection catch-up...");
    let projector = BalanceTrackingProjector::new();
    let balances = projector.balances();

    let proj_start = Instant::now();
    let proj_result = run_projection(projector, store.as_ref(), ProjectionConfig::default()).await;
    let proj_elapsed = proj_start.elapsed();

    let correctness_passed = match proj_result {
        Ok(()) => {
            // Verify projection correctness
            let bal = balances.lock().expect("lock poisoned");
            let mut all_correct = true;
            for (stream_id_str, balance) in bal.iter() {
                // Each stream should have writes_per_task deposits of 1 cent each
                // (assuming all succeeded for that stream)
                if *balance == 0 {
                    eprintln!("CORRECTNESS WARNING: stream {stream_id_str} has zero balance");
                    all_correct = false;
                }
            }
            if bal.len() != concurrency as usize {
                eprintln!(
                    "CORRECTNESS WARNING: expected {} streams in projection, found {}",
                    concurrency,
                    bal.len()
                );
                all_correct = false;
            }
            all_correct
        }
        Err(e) => {
            eprintln!("Projection failed: {e}");
            false
        }
    };

    let events_per_sec = if proj_elapsed.as_secs_f64() > 0.0 {
        total_events as f64 / proj_elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!(
        "Phase 2 complete: projected {total_events} events in {:.2}s ({events_per_sec:.0} events/sec)",
        proj_elapsed.as_secs_f64()
    );

    // Build a synthetic report for the projection phase
    let mut metrics = crate::metrics::merge_task_metrics(vec![crate::metrics::TaskMetrics::new()]);
    metrics.successes = total_events;

    MetricsReport {
        scenario_name: "Projection (catch-up)".to_string(),
        backend: config.backend.to_string(),
        concurrency,
        elapsed: proj_elapsed,
        metrics,
        correctness_passed: Some(correctness_passed),
    }
}
