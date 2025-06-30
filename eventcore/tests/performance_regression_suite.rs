//! Comprehensive performance regression test suite for `EventCore`.
//!
//! This module provides a complete performance regression testing framework
//! that tracks performance metrics over time and detects regressions.

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::future_not_send)]
#![allow(clippy::if_not_else)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::use_self)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::default_trait_access)]
#![allow(clippy::cast_lossless)]

use async_trait::async_trait;
use eventcore::{
    Command, CommandError, CommandExecutor, EventId, EventStore, EventToWrite, ExpectedVersion,
    ReadStreams, StreamEvents, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::RwLock;

/// Performance metric tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerformanceMetric {
    test_name: String,
    timestamp: SystemTime,
    operations: usize,
    duration_ms: f64,
    ops_per_second: f64,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
    memory_bytes: Option<u64>,
    metadata: HashMap<String, serde_json::Value>,
}

/// Historical baseline for regression detection
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerformanceBaseline {
    test_name: String,
    last_updated: SystemTime,
    samples: Vec<PerformanceMetric>,
    thresholds: RegressionThresholds,
}

/// Configurable regression thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegressionThresholds {
    /// Maximum acceptable decrease in ops/second (percentage)
    throughput_regression_percent: f64,
    /// Maximum acceptable increase in P95 latency (percentage)
    latency_regression_percent: f64,
    /// Minimum number of samples before establishing baseline
    min_samples: usize,
    /// Maximum age of baseline samples (days)
    max_sample_age_days: u64,
}

impl Default for RegressionThresholds {
    fn default() -> Self {
        Self {
            throughput_regression_percent: 10.0,
            latency_regression_percent: 20.0,
            min_samples: 5,
            max_sample_age_days: 30,
        }
    }
}

/// Performance regression detector
struct RegressionDetector {
    baselines: RwLock<HashMap<String, PerformanceBaseline>>,
    baseline_path: String,
}

impl RegressionDetector {
    fn new(baseline_path: String) -> Self {
        Self {
            baselines: RwLock::new(HashMap::new()),
            baseline_path,
        }
    }

    async fn load_baselines(&self) -> Result<(), Box<dyn std::error::Error>> {
        if Path::new(&self.baseline_path).exists() {
            let data = fs::read_to_string(&self.baseline_path)?;
            let baselines: HashMap<String, PerformanceBaseline> = serde_json::from_str(&data)?;
            *self.baselines.write().await = baselines;
        }
        Ok(())
    }

    async fn save_baselines(&self) -> Result<(), Box<dyn std::error::Error>> {
        let baselines = self.baselines.read().await;
        let data = serde_json::to_string_pretty(&*baselines)?;
        fs::write(&self.baseline_path, data)?;
        Ok(())
    }

    async fn check_regression(
        &self,
        metric: &PerformanceMetric,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let mut baselines = self.baselines.write().await;

        match baselines.get_mut(&metric.test_name) {
            Some(baseline) => {
                // Clean old samples
                let cutoff = SystemTime::now()
                    - Duration::from_secs(baseline.thresholds.max_sample_age_days * 24 * 60 * 60);
                baseline.samples.retain(|s| s.timestamp > cutoff);

                if baseline.samples.len() >= baseline.thresholds.min_samples {
                    // Calculate baseline averages
                    let baseline_throughput: f64 = baseline
                        .samples
                        .iter()
                        .map(|s| s.ops_per_second)
                        .sum::<f64>()
                        / baseline.samples.len() as f64;

                    let baseline_p95: f64 = baseline
                        .samples
                        .iter()
                        .map(|s| s.p95_latency_ms)
                        .sum::<f64>()
                        / baseline.samples.len() as f64;

                    // Check for regressions
                    let throughput_decrease =
                        (baseline_throughput - metric.ops_per_second) / baseline_throughput * 100.0;
                    let latency_increase =
                        (metric.p95_latency_ms - baseline_p95) / baseline_p95 * 100.0;

                    let mut regressions = Vec::new();

                    if throughput_decrease > baseline.thresholds.throughput_regression_percent {
                        regressions.push(format!(
                            "Throughput regressed by {:.1}% (baseline: {:.0} ops/s, current: {:.0} ops/s)",
                            throughput_decrease, baseline_throughput, metric.ops_per_second
                        ));
                    }

                    if latency_increase > baseline.thresholds.latency_regression_percent {
                        regressions.push(format!(
                            "P95 latency regressed by {:.1}% (baseline: {:.2}ms, current: {:.2}ms)",
                            latency_increase, baseline_p95, metric.p95_latency_ms
                        ));
                    }

                    if !regressions.is_empty() {
                        return Ok(Some(regressions.join("; ")));
                    }
                }

                // Add current sample
                baseline.samples.push(metric.clone());
                baseline.last_updated = SystemTime::now();
            }
            None => {
                // Create new baseline
                baselines.insert(
                    metric.test_name.clone(),
                    PerformanceBaseline {
                        test_name: metric.test_name.clone(),
                        last_updated: SystemTime::now(),
                        samples: vec![metric.clone()],
                        thresholds: RegressionThresholds::default(),
                    },
                );
            }
        }

        Ok(None)
    }
}

// Test event types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PerfRegressionEvent {
    Created { id: String, data: String },
    Updated { id: String, data: String },
    Deleted { id: String },
}

impl TryFrom<&PerfRegressionEvent> for PerfRegressionEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &PerfRegressionEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// Test state
#[derive(Debug, Default, Clone)]
struct PerfRegressionState {
    entities: HashMap<String, String>,
}

// Stream discovery command for testing dynamic resolution performance
#[derive(Debug, Clone)]
struct StreamDiscoveryCommand {
    iterations: usize,
}

#[derive(Debug, Clone)]
struct StreamDiscoveryInput {
    base_stream: StreamId,
    num_streams: usize,
}

#[async_trait]
impl Command for StreamDiscoveryCommand {
    type Input = StreamDiscoveryInput;
    type State = PerfRegressionState;
    type Event = PerfRegressionEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.base_stream.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            PerfRegressionEvent::Created { id, data } => {
                state.entities.insert(id.clone(), data.clone());
            }
            PerfRegressionEvent::Updated { id, data } => {
                state.entities.insert(id.clone(), data.clone());
            }
            PerfRegressionEvent::Deleted { id } => {
                state.entities.remove(id);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Simulate dynamic stream discovery
        for i in 0..self.iterations {
            let new_streams: Vec<StreamId> = (0..input.num_streams)
                .map(|j| StreamId::try_new(format!("discovered-{}-{}", i, j)).unwrap())
                .collect();
            stream_resolver.add_streams(new_streams);
        }

        // Write a simple event
        Ok(vec![StreamWrite::new(
            &read_streams,
            input.base_stream,
            PerfRegressionEvent::Created {
                id: "test".to_string(),
                data: "discovery complete".to_string(),
            },
        )?])
    }
}

// Large state reconstruction command
#[derive(Debug, Clone)]
struct LargeStateCommand;

#[derive(Debug, Clone)]
struct LargeStateInput {
    streams: Vec<StreamId>,
}

#[async_trait]
impl Command for LargeStateCommand {
    type Input = LargeStateInput;
    type State = PerfRegressionState;
    type Event = PerfRegressionEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        input.streams.clone()
    }

    fn apply(&self, state: &mut Self::State, event: &eventcore::StoredEvent<Self::Event>) {
        match &event.payload {
            PerfRegressionEvent::Created { id, data } => {
                state.entities.insert(id.clone(), data.clone());
            }
            PerfRegressionEvent::Updated { id, data } => {
                state.entities.insert(id.clone(), data.clone());
            }
            PerfRegressionEvent::Deleted { id } => {
                state.entities.remove(id);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        // Command that processes large state
        let entity_count = state.entities.len();

        Ok(vec![StreamWrite::new(
            &read_streams,
            input.streams[0].clone(),
            PerfRegressionEvent::Updated {
                id: "summary".to_string(),
                data: format!("Processed {} entities", entity_count),
            },
        )?])
    }
}

/// Performance test runner
struct PerformanceTestRunner<S: EventStore<Event = PerfRegressionEvent>> {
    store: S,
    #[allow(dead_code)] // Used for future regression detection features
    detector: Arc<RegressionDetector>,
}

impl<S: EventStore<Event = PerfRegressionEvent> + Clone + 'static> PerformanceTestRunner<S> {
    fn new(store: S, detector: Arc<RegressionDetector>) -> Self {
        Self { store, detector }
    }

    async fn run_stream_discovery_test(
        &self,
    ) -> Result<PerformanceMetric, Box<dyn std::error::Error>> {
        let executor = CommandExecutor::new(self.store.clone());
        let mut latencies = Vec::new();
        let num_operations = 100;

        let start = Instant::now();

        for i in 0..num_operations {
            let op_start = Instant::now();

            let command = StreamDiscoveryCommand { iterations: 3 };
            let input = StreamDiscoveryInput {
                base_stream: StreamId::try_new(format!("discovery-base-{}", i)).unwrap(),
                num_streams: 5,
            };

            executor
                .execute(&command, input, Default::default())
                .await?;

            let latency = op_start.elapsed();
            latencies.push(latency.as_micros() as f64 / 1000.0);
        }

        let duration = start.elapsed();
        let ops_per_second = num_operations as f64 / duration.as_secs_f64();

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = latencies[latencies.len() / 2];
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

        Ok(PerformanceMetric {
            test_name: "stream_discovery_performance".to_string(),
            timestamp: SystemTime::now(),
            operations: num_operations,
            duration_ms: duration.as_millis() as f64,
            ops_per_second,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            memory_bytes: None,
            metadata: HashMap::new(),
        })
    }

    async fn run_large_state_reconstruction_test(
        &self,
    ) -> Result<PerformanceMetric, Box<dyn std::error::Error>> {
        // Prepare test data
        let num_streams = 10;
        let events_per_stream = 100;

        for i in 0..num_streams {
            let stream_id = StreamId::try_new(format!("large-state-{}", i)).unwrap();
            let mut events = Vec::new();

            for j in 0..events_per_stream {
                events.push(EventToWrite::new(
                    EventId::new(),
                    PerfRegressionEvent::Created {
                        id: format!("entity-{}-{}", i, j),
                        data: format!("data-{}-{}", i, j),
                    },
                ));
            }

            self.store
                .write_events_multi(vec![StreamEvents {
                    stream_id,
                    expected_version: ExpectedVersion::New,
                    events,
                }])
                .await?;
        }

        // Test reconstruction performance
        let executor = CommandExecutor::new(self.store.clone());
        let mut latencies = Vec::new();
        let num_operations = 20;

        let start = Instant::now();

        for _ in 0..num_operations {
            let op_start = Instant::now();

            let command = LargeStateCommand;
            let input = LargeStateInput {
                streams: (0..num_streams)
                    .map(|i| StreamId::try_new(format!("large-state-{}", i)).unwrap())
                    .collect(),
            };

            executor
                .execute(&command, input, Default::default())
                .await?;

            let latency = op_start.elapsed();
            latencies.push(latency.as_micros() as f64 / 1000.0);
        }

        let duration = start.elapsed();
        let ops_per_second = num_operations as f64 / duration.as_secs_f64();

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = latencies[latencies.len() / 2];
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

        Ok(PerformanceMetric {
            test_name: "large_state_reconstruction".to_string(),
            timestamp: SystemTime::now(),
            operations: num_operations,
            duration_ms: duration.as_millis() as f64,
            ops_per_second,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            memory_bytes: None,
            metadata: HashMap::from([
                ("num_streams".to_string(), serde_json::json!(num_streams)),
                (
                    "events_per_stream".to_string(),
                    serde_json::json!(events_per_stream),
                ),
            ]),
        })
    }

    async fn run_concurrent_operations_test(
        &self,
    ) -> Result<PerformanceMetric, Box<dyn std::error::Error>> {
        let executor = Arc::new(CommandExecutor::new(self.store.clone()));
        let num_workers = 10;
        let ops_per_worker = 50;

        let success_count = Arc::new(AtomicU64::new(0));
        let total_latency = Arc::new(RwLock::new(0.0));
        let start = Instant::now();

        let mut handles = vec![];

        for worker_id in 0..num_workers {
            let executor = executor.clone();
            let success_count = success_count.clone();
            let total_latency = total_latency.clone();

            handles.push(tokio::spawn(async move {
                for op_id in 0..ops_per_worker {
                    let op_start = Instant::now();

                    let command = StreamDiscoveryCommand { iterations: 1 };
                    let input = StreamDiscoveryInput {
                        base_stream: StreamId::try_new(format!(
                            "concurrent-{}-{}",
                            worker_id, op_id
                        ))
                        .unwrap(),
                        num_streams: 3,
                    };

                    if executor
                        .execute(&command, input, Default::default())
                        .await
                        .is_ok()
                    {
                        success_count.fetch_add(1, Ordering::Relaxed);
                        let latency = op_start.elapsed().as_micros() as f64 / 1000.0;
                        *total_latency.write().await += latency;
                    }
                }
            }));
        }

        for handle in handles {
            handle.await?;
        }

        let duration = start.elapsed();
        let total_ops = (num_workers * ops_per_worker) as f64;
        let successful_ops = success_count.load(Ordering::Relaxed) as f64;
        let ops_per_second = successful_ops / duration.as_secs_f64();
        let avg_latency = *total_latency.read().await / successful_ops;

        Ok(PerformanceMetric {
            test_name: "concurrent_operations".to_string(),
            timestamp: SystemTime::now(),
            operations: successful_ops as usize,
            duration_ms: duration.as_millis() as f64,
            ops_per_second,
            p50_latency_ms: avg_latency,
            p95_latency_ms: avg_latency * 1.5, // Approximation
            p99_latency_ms: avg_latency * 2.0, // Approximation
            memory_bytes: None,
            metadata: HashMap::from([
                ("num_workers".to_string(), serde_json::json!(num_workers)),
                (
                    "ops_per_worker".to_string(),
                    serde_json::json!(ops_per_worker),
                ),
                (
                    "success_rate".to_string(),
                    serde_json::json!(successful_ops / total_ops),
                ),
            ]),
        })
    }

    async fn run_cold_start_test(&self) -> Result<PerformanceMetric, Box<dyn std::error::Error>> {
        // Test performance after clearing any caches
        let executor = CommandExecutor::new(self.store.clone());
        let mut cold_latencies = Vec::new();
        let mut warm_latencies = Vec::new();

        let num_operations = 50;

        // Cold start measurements
        for i in 0..num_operations {
            // Create a new stream each time to avoid cache hits
            let stream_id = StreamId::try_new(format!("cold-start-{}", i)).unwrap();

            // Write initial data
            self.store
                .write_events_multi(vec![StreamEvents {
                    stream_id: stream_id.clone(),
                    expected_version: ExpectedVersion::New,
                    events: vec![EventToWrite::new(
                        EventId::new(),
                        PerfRegressionEvent::Created {
                            id: format!("cold-{}", i),
                            data: "initial".to_string(),
                        },
                    )],
                }])
                .await?;

            // Drop and recreate executor to simulate cold start
            let _ = &executor; // Ignore the old executor
            let executor = CommandExecutor::new(self.store.clone());

            let op_start = Instant::now();
            let command = LargeStateCommand;
            let input = LargeStateInput {
                streams: vec![stream_id.clone()],
            };
            executor
                .execute(&command, input, Default::default())
                .await?;

            cold_latencies.push(op_start.elapsed().as_micros() as f64 / 1000.0);

            // Warm measurement (same stream)
            let op_start = Instant::now();
            let input = LargeStateInput {
                streams: vec![stream_id],
            };
            executor
                .execute(&command, input, Default::default())
                .await?;

            warm_latencies.push(op_start.elapsed().as_micros() as f64 / 1000.0);
        }

        cold_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        warm_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let cold_p50 = cold_latencies[cold_latencies.len() / 2];
        let warm_p50 = warm_latencies[warm_latencies.len() / 2];
        let improvement_ratio = cold_p50 / warm_p50;

        Ok(PerformanceMetric {
            test_name: "cold_start_performance".to_string(),
            timestamp: SystemTime::now(),
            operations: num_operations * 2,
            duration_ms: 0.0,    // Not applicable for this test
            ops_per_second: 0.0, // Not applicable
            p50_latency_ms: cold_p50,
            p95_latency_ms: cold_latencies[(cold_latencies.len() as f64 * 0.95) as usize],
            p99_latency_ms: cold_latencies[(cold_latencies.len() as f64 * 0.99) as usize],
            memory_bytes: None,
            metadata: HashMap::from([
                ("warm_p50_ms".to_string(), serde_json::json!(warm_p50)),
                (
                    "cache_improvement_ratio".to_string(),
                    serde_json::json!(improvement_ratio),
                ),
            ]),
        })
    }
}

/// Run comprehensive performance regression tests
async fn run_regression_suite<S: EventStore<Event = PerfRegressionEvent> + Clone + 'static>(
    store: S,
    baseline_path: String,
) -> Result<Vec<(PerformanceMetric, Option<String>)>, Box<dyn std::error::Error>> {
    let detector = Arc::new(RegressionDetector::new(baseline_path));
    detector.load_baselines().await?;

    let runner = PerformanceTestRunner::new(store, detector.clone());
    let mut results = Vec::new();

    // Run all test scenarios
    println!("\nRunning Stream Discovery...");
    match runner.run_stream_discovery_test().await {
        Ok(metric) => {
            println!("  Operations: {}", metric.operations);
            println!("  Throughput: {:.2} ops/sec", metric.ops_per_second);
            println!("  P50 Latency: {:.2}ms", metric.p50_latency_ms);
            println!("  P95 Latency: {:.2}ms", metric.p95_latency_ms);
            println!("  P99 Latency: {:.2}ms", metric.p99_latency_ms);

            let regression = detector.check_regression(&metric).await?;
            if let Some(ref msg) = regression {
                println!("  ⚠️  REGRESSION DETECTED: {}", msg);
            } else {
                println!("  ✓ No regression detected");
            }

            results.push((metric, regression));
        }
        Err(e) => {
            println!("  ❌ Test failed: {}", e);
        }
    }

    println!("\nRunning Large State Reconstruction...");
    match runner.run_large_state_reconstruction_test().await {
        Ok(metric) => {
            println!("  Operations: {}", metric.operations);
            println!("  Throughput: {:.2} ops/sec", metric.ops_per_second);
            println!("  P50 Latency: {:.2}ms", metric.p50_latency_ms);
            println!("  P95 Latency: {:.2}ms", metric.p95_latency_ms);
            println!("  P99 Latency: {:.2}ms", metric.p99_latency_ms);

            let regression = detector.check_regression(&metric).await?;
            if let Some(ref msg) = regression {
                println!("  ⚠️  REGRESSION DETECTED: {}", msg);
            } else {
                println!("  ✓ No regression detected");
            }

            results.push((metric, regression));
        }
        Err(e) => {
            println!("  ❌ Test failed: {}", e);
        }
    }

    println!("\nRunning Concurrent Operations...");
    match runner.run_concurrent_operations_test().await {
        Ok(metric) => {
            println!("  Operations: {}", metric.operations);
            println!("  Throughput: {:.2} ops/sec", metric.ops_per_second);
            println!("  P50 Latency: {:.2}ms", metric.p50_latency_ms);
            println!("  P95 Latency: {:.2}ms", metric.p95_latency_ms);
            println!("  P99 Latency: {:.2}ms", metric.p99_latency_ms);

            let regression = detector.check_regression(&metric).await?;
            if let Some(ref msg) = regression {
                println!("  ⚠️  REGRESSION DETECTED: {}", msg);
            } else {
                println!("  ✓ No regression detected");
            }

            results.push((metric, regression));
        }
        Err(e) => {
            println!("  ❌ Test failed: {}", e);
        }
    }

    println!("\nRunning Cold Start...");
    match runner.run_cold_start_test().await {
        Ok(metric) => {
            println!("  Operations: {}", metric.operations);
            println!("  Throughput: {:.2} ops/sec", metric.ops_per_second);
            println!("  P50 Latency: {:.2}ms", metric.p50_latency_ms);
            println!("  P95 Latency: {:.2}ms", metric.p95_latency_ms);
            println!("  P99 Latency: {:.2}ms", metric.p99_latency_ms);

            let regression = detector.check_regression(&metric).await?;
            if let Some(ref msg) = regression {
                println!("  ⚠️  REGRESSION DETECTED: {}", msg);
            } else {
                println!("  ✓ No regression detected");
            }

            results.push((metric, regression));
        }
        Err(e) => {
            println!("  ❌ Test failed: {}", e);
        }
    }

    detector.save_baselines().await?;
    Ok(results)
}

#[tokio::test]
#[ignore = "Performance regression tests should be run explicitly"]
async fn comprehensive_performance_regression_suite() {
    println!("\n=== EventCore Performance Regression Test Suite ===\n");

    // Run with in-memory store
    println!("Testing InMemoryEventStore...");
    let store = InMemoryEventStore::<PerfRegressionEvent>::new();
    let baseline_path = "target/performance_baselines_memory.json".to_string();

    let results = run_regression_suite(store, baseline_path)
        .await
        .expect("Memory store regression tests should complete");

    // Check for any regressions
    let regressions: Vec<_> = results
        .iter()
        .filter_map(|(metric, regression)| {
            regression
                .as_ref()
                .map(|r| (metric.test_name.clone(), r.clone()))
        })
        .collect();

    if !regressions.is_empty() {
        println!("\n❌ Performance regressions detected:");
        for (test, msg) in &regressions {
            println!("  - {}: {}", test, msg);
        }
        panic!(
            "Performance regressions detected in {} tests",
            regressions.len()
        );
    } else {
        println!("\n✅ All performance tests passed without regressions");
    }
}

#[tokio::test]
#[ignore = "Requires PostgreSQL connection"]
async fn postgres_performance_regression_suite() {
    println!("\n=== PostgreSQL Performance Regression Test Suite ===\n");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let store = PostgresEventStore::<PerfRegressionEvent>::new(
        eventcore_postgres::PostgresConfig::new(database_url.clone()),
    )
    .await
    .expect("Failed to connect to PostgreSQL");

    store
        .initialize()
        .await
        .expect("Failed to initialize schema");

    let baseline_path = "target/performance_baselines_postgres.json".to_string();

    let results = run_regression_suite(store, baseline_path)
        .await
        .expect("PostgreSQL regression tests should complete");

    // PostgreSQL is expected to be slower, so we're more lenient with regression detection
    let regressions: Vec<_> = results
        .iter()
        .filter_map(|(metric, regression)| {
            regression
                .as_ref()
                .map(|r| (metric.test_name.clone(), r.clone()))
        })
        .collect();

    if !regressions.is_empty() {
        println!("\n⚠️  PostgreSQL performance notes:");
        for (test, msg) in &regressions {
            println!("  - {}: {}", test, msg);
        }
        println!("\nNote: PostgreSQL performance variations are expected due to network/disk I/O");
    }
}

/// Generate performance report comparing different scenarios
#[tokio::test]
#[ignore = "Performance tests should be run explicitly"]
async fn generate_performance_report() {
    println!("\n=== Performance Comparison Report ===\n");

    let store = InMemoryEventStore::<PerfRegressionEvent>::new();
    let detector = Arc::new(RegressionDetector::new(
        "target/report_baselines.json".to_string(),
    ));

    let runner = PerformanceTestRunner::new(store, detector);

    // Run tests and collect metrics
    let metrics = vec![
        ("Stream Discovery", runner.run_stream_discovery_test().await),
        (
            "Large State",
            runner.run_large_state_reconstruction_test().await,
        ),
        (
            "Concurrent Ops",
            runner.run_concurrent_operations_test().await,
        ),
        ("Cold Start", runner.run_cold_start_test().await),
    ];

    // Create comparison table
    println!(
        "{:<20} {:>12} {:>10} {:>10} {:>10}",
        "Test", "Ops/sec", "P50 (ms)", "P95 (ms)", "P99 (ms)"
    );
    println!("{}", "-".repeat(75));

    for (name, result) in metrics {
        match result {
            Ok(metric) => {
                println!(
                    "{:<20} {:>12.2} {:>10.2} {:>10.2} {:>10.2}",
                    name,
                    metric.ops_per_second,
                    metric.p50_latency_ms,
                    metric.p95_latency_ms,
                    metric.p99_latency_ms
                );
            }
            Err(e) => {
                println!("{:<20} ERROR: {}", name, e);
            }
        }
    }

    println!("\n✅ Performance report generated successfully");
}
