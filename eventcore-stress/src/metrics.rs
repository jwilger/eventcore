use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use hdrhistogram::Histogram;

/// Per-task metrics collector. Each spawned task owns one of these to avoid
/// contention on the histogram. After the task finishes, its metrics are sent
/// back via a channel for merging.
pub struct TaskMetrics {
    histogram: Histogram<u64>,
    successes: u64,
    errors: u64,
    retries: u64,
}

impl TaskMetrics {
    pub fn new() -> Self {
        Self {
            // 3 significant figures, max value 60 seconds in microseconds
            histogram: Histogram::new_with_max(60_000_000, 3).expect("valid histogram config"),
            successes: 0,
            errors: 0,
            retries: 0,
        }
    }

    pub fn record(&mut self, latency: Duration, success: bool, retries: u64) {
        let micros = latency.as_micros() as u64;
        let _ = self.histogram.record(micros);
        if success {
            self.successes += 1;
        } else {
            self.errors += 1;
        }
        self.retries += retries;
    }
}

/// Merged metrics from all tasks in a scenario run.
pub struct StressMetrics {
    histogram: Histogram<u64>,
    pub successes: u64,
    pub errors: u64,
    pub retries: u64,
}

impl StressMetrics {
    pub fn total(&self) -> u64 {
        self.successes + self.errors
    }

    pub fn p50(&self) -> u64 {
        self.histogram.value_at_quantile(0.50)
    }

    pub fn p95(&self) -> u64 {
        self.histogram.value_at_quantile(0.95)
    }

    pub fn p99(&self) -> u64 {
        self.histogram.value_at_quantile(0.99)
    }

    pub fn max(&self) -> u64 {
        self.histogram.max()
    }
}

/// Merge multiple per-task metrics into a single StressMetrics.
pub fn merge_task_metrics(tasks: Vec<TaskMetrics>) -> StressMetrics {
    let mut merged = Histogram::<u64>::new_with_max(60_000_000, 3).expect("valid histogram config");
    let mut successes = 0u64;
    let mut errors = 0u64;
    let mut retries = 0u64;

    for t in tasks {
        merged
            .add(&t.histogram)
            .expect("histogram merge should succeed");
        successes += t.successes;
        errors += t.errors;
        retries += t.retries;
    }

    StressMetrics {
        histogram: merged,
        successes,
        errors,
        retries,
    }
}

/// Final report for a stress test scenario.
pub struct MetricsReport {
    pub scenario_name: String,
    pub backend: String,
    pub concurrency: u32,
    pub elapsed: Duration,
    pub metrics: StressMetrics,
    pub correctness_passed: Option<bool>,
}

impl fmt::Display for MetricsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.metrics.total();
        let ops_per_sec = if self.elapsed.as_secs_f64() > 0.0 {
            total as f64 / self.elapsed.as_secs_f64()
        } else {
            0.0
        };
        let retry_pct = if total > 0 {
            (self.metrics.retries as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        writeln!(f)?;
        writeln!(f, "=== Stress Test: {} ===", self.scenario_name)?;
        writeln!(f, "Backend:      {}", self.backend)?;
        writeln!(f, "Concurrency:  {}", self.concurrency)?;
        writeln!(f, "Duration:     {:.2}s", self.elapsed.as_secs_f64())?;
        writeln!(f, "Total ops:    {total}")?;
        writeln!(f, "Throughput:   {ops_per_sec:.0} ops/sec")?;
        writeln!(f, "Latency:")?;
        writeln!(f, "  p50:    {} us", self.metrics.p50())?;
        writeln!(f, "  p95:    {} us", self.metrics.p95())?;
        writeln!(f, "  p99:    {} us", self.metrics.p99())?;
        writeln!(f, "  max:    {} us", self.metrics.max())?;
        writeln!(f, "Errors:       {}", self.metrics.errors)?;
        writeln!(
            f,
            "Retries:      {} ({retry_pct:.1}% of ops)",
            self.metrics.retries
        )?;

        if let Some(passed) = self.correctness_passed {
            writeln!(f, "Correctness:  {}", if passed { "PASS" } else { "FAIL" })?;
        }

        Ok(())
    }
}

/// Shared retry counter using atomics. Clone-friendly because the inner
/// AtomicU64 is behind an Arc.
#[derive(Clone)]
pub struct RetryCounter(pub std::sync::Arc<AtomicU64>);

impl RetryCounter {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(AtomicU64::new(0)))
    }

    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

impl eventcore::MetricsHook for RetryCounter {
    fn on_retry_attempt(&self, _ctx: &eventcore::RetryContext) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
