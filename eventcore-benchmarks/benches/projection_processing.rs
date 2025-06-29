use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use eventcore::{
    event::Event,
    metadata::EventMetadata,
    projection::{ProjectionCheckpoint, ProjectionConfig, ProjectionStatus},
    types::{EventId, StreamId},
};
use std::collections::HashMap;
use std::hint::black_box;
use tokio::runtime::Runtime;

/// Test event types for projection benchmarks
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum BenchmarkEvent {
    UserRegistered {
        user_id: String,
        email: String,
    },
    OrderPlaced {
        order_id: String,
        user_id: String,
        amount: u64,
    },
}

/// Simple user count projection for benchmarking
#[derive(Debug, Clone)]
struct UserCountProjection {
    config: ProjectionConfig,
    state: UserCountState,
}

#[derive(Debug, Clone)]
struct UserCountState {
    total_users: u64,
    users_by_domain: HashMap<String, u64>,
}

impl Default for UserCountState {
    fn default() -> Self {
        Self {
            total_users: 0,
            users_by_domain: HashMap::new(),
        }
    }
}

impl UserCountProjection {
    fn new() -> Self {
        Self {
            config: ProjectionConfig::new("user_count_projection")
                .with_streams(vec![StreamId::new("users").unwrap()]),
            state: UserCountState::default(),
        }
    }

    /// Process a single event (simulating projection processing)
    fn process_event(&mut self, event: &BenchmarkEvent) {
        match event {
            BenchmarkEvent::UserRegistered { email, .. } => {
                self.state.total_users += 1;

                if let Some(domain) = email.split('@').nth(1) {
                    *self
                        .state
                        .users_by_domain
                        .entry(domain.to_string())
                        .or_insert(0) += 1;
                }
            }
            _ => {} // Other events don't affect user count
        }
    }
}

#[async_trait::async_trait]
impl eventcore::projection::Projection for UserCountProjection {
    type State = UserCountState;
    type Event = BenchmarkEvent;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> eventcore::errors::ProjectionResult<Self::State> {
        Ok(self.state.clone())
    }

    async fn get_status(&self) -> eventcore::errors::ProjectionResult<ProjectionStatus> {
        Ok(ProjectionStatus::Running)
    }

    async fn load_checkpoint(&self) -> eventcore::errors::ProjectionResult<ProjectionCheckpoint> {
        Ok(ProjectionCheckpoint::new())
    }

    async fn save_checkpoint(
        &self,
        _checkpoint: ProjectionCheckpoint,
    ) -> eventcore::errors::ProjectionResult<()> {
        Ok(())
    }
}

/// Benchmark single event processing by projections
fn bench_single_event_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("single_event_processing");
    group.throughput(Throughput::Elements(1));

    group.bench_function("user_count_projection", |b| {
        b.to_async(&rt).iter(|| async {
            let mut projection = UserCountProjection::new();
            let event = BenchmarkEvent::UserRegistered {
                user_id: format!("user-{}", EventId::new()),
                email: format!("test-{}@example.com", EventId::new()),
            };

            black_box(projection.process_event(&event))
        });
    });

    group.finish();
}

/// Benchmark batch event processing by projections
fn bench_batch_event_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("batch_event_processing");

    for batch_size in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(batch_size));

        group.bench_with_input(
            BenchmarkId::new("user_count_batch", batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let mut projection = UserCountProjection::new();

                    for i in 0..size {
                        let event = BenchmarkEvent::UserRegistered {
                            user_id: format!("user-{}", i),
                            email: format!("test-{}@example.com", i),
                        };

                        projection.process_event(&event);
                    }

                    black_box(projection)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark projection state operations
fn bench_projection_state_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("projection_state_operations");
    group.throughput(Throughput::Elements(1));

    // Setup: create projection with substantial state
    let mut projection = UserCountProjection::new();

    rt.block_on(async {
        // Populate with test data
        for i in 0..1000 {
            let event = BenchmarkEvent::UserRegistered {
                user_id: format!("user-{}", i),
                email: format!("test-{}@{}.com", i, i % 10),
            };
            projection.process_event(&event);
        }
    });

    group.bench_function("get_projection_state", |b| {
        b.to_async(&rt)
            .iter(|| async { black_box(projection.get_state().await.unwrap()) });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_single_event_processing,
    bench_batch_event_processing,
    bench_projection_state_operations,
);
criterion_main!(benches);
