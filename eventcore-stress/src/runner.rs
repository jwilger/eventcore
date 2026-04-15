use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::config::StressConfig;
use crate::metrics::{MetricsReport, TaskMetrics, merge_task_metrics};

/// Result of a single stress test operation.
pub struct OperationResult {
    pub success: bool,
    pub retries: u64,
}

/// Run a stress test scenario with the given configuration.
///
/// Spawns `concurrency` tokio tasks, each running the `operation` closure in a
/// loop until the termination condition (duration or iteration count) is met.
///
/// The `operation` closure receives the task index (0-based) and returns an
/// `OperationResult`.
pub async fn run_stress<F, Fut>(
    config: &StressConfig,
    scenario_name: &str,
    operation: F,
) -> MetricsReport
where
    F: Fn(u32) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = OperationResult> + Send,
{
    let concurrency = config.concurrency;
    let effective_duration = config.effective_duration();
    let effective_iterations = config.effective_iterations();

    let (tx, mut rx) = mpsc::channel::<TaskMetrics>(concurrency as usize);
    let operation = Arc::new(operation);
    let start = Instant::now();

    // Per-task iteration count: divide total iterations evenly, give remainder
    // to the first tasks.
    let per_task_iters = effective_iterations.map(|total| {
        let base = total / concurrency as u64;
        let remainder = total % concurrency as u64;
        (base, remainder)
    });

    for task_idx in 0..concurrency {
        let tx = tx.clone();
        let op = Arc::clone(&operation);
        let task_iters = per_task_iters.map(|(base, remainder)| {
            if (task_idx as u64) < remainder {
                base + 1
            } else {
                base
            }
        });

        tokio::spawn(async move {
            let mut metrics = TaskMetrics::new();
            let mut count = 0u64;

            loop {
                // Check termination condition
                if let Some(max_iters) = task_iters
                    && count >= max_iters
                {
                    break;
                }
                if let Some(dur) = effective_duration
                    && start.elapsed() >= dur
                {
                    break;
                }

                let op_start = Instant::now();
                let result = op(task_idx).await;
                let latency = op_start.elapsed();

                metrics.record(latency, result.success, result.retries);
                count += 1;
            }

            let _ = tx.send(metrics).await;
        });
    }

    // Drop our sender so the receiver closes when all tasks are done.
    drop(tx);

    let mut all_metrics = Vec::with_capacity(concurrency as usize);
    while let Some(m) = rx.recv().await {
        all_metrics.push(m);
    }

    let elapsed = start.elapsed();
    let merged = merge_task_metrics(all_metrics);

    MetricsReport {
        scenario_name: scenario_name.to_string(),
        backend: config.backend.to_string(),
        concurrency,
        elapsed,
        metrics: merged,
        correctness_passed: None,
    }
}

/// Like `run_stress`, but allows a post-run correctness check that can set
/// the correctness_passed field on the report.
pub async fn run_stress_with_correctness<F, Fut, C, CFut>(
    config: &StressConfig,
    scenario_name: &str,
    operation: F,
    correctness_check: C,
) -> MetricsReport
where
    F: Fn(u32) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = OperationResult> + Send,
    C: FnOnce(u64) -> CFut,
    CFut: Future<Output = bool>,
{
    let mut report = run_stress(config, scenario_name, operation).await;
    let passed = correctness_check(report.metrics.successes).await;
    report.correctness_passed = Some(passed);
    report
}

/// Helper to run a stress test with duration-based termination using a
/// pre-constructed `Instant` deadline. Returns the elapsed Duration.
pub fn is_past_deadline(deadline: Instant) -> bool {
    Instant::now() >= deadline
}
