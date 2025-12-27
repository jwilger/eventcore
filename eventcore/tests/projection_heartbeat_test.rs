//! Integration test for eventcore-2n5: Heartbeat and Liveness Detection
//!
//! Scenario: Healthy projector sends heartbeats
//! - Given projector holds leadership with heartbeat_interval of 5 seconds
//! - When projector processes events normally
//! - Then runner calls guard.heartbeat() at least every heartbeat_interval
//! - And guard.is_valid() continues to return true

use eventcore::{
    CoordinatorTrait, Event, EventStore, FailureContext, FailureStrategy, GuardTrait,
    HeartbeatConfig, ProjectionRunner, Projector, StreamId, StreamPosition, StreamVersion,
    StreamWrites,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A simple event type for testing projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CounterIncremented {
    counter_id: StreamId,
}

impl Event for CounterIncremented {
    fn stream_id(&self) -> &StreamId {
        &self.counter_id
    }
}

/// Projector that simulates work during event processing.
struct HeartbeatCountingProjector;

impl HeartbeatCountingProjector {
    fn new() -> Self {
        Self
    }
}

impl Projector for HeartbeatCountingProjector {
    type Event = CounterIncremented;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        // Simulate some processing work to allow time for heartbeats
        std::thread::sleep(std::time::Duration::from_millis(100));
        Ok(())
    }

    fn name(&self) -> &str {
        "heartbeat-counter"
    }

    fn on_error(&mut self, _ctx: FailureContext<Self::Error>) -> FailureStrategy {
        FailureStrategy::Fatal
    }
}

/// Coordinator that tracks heartbeat calls.
struct HeartbeatCountingCoordinator {
    heartbeat_count: Arc<AtomicUsize>,
    heartbeat_timeout: Duration,
}

impl HeartbeatCountingCoordinator {
    fn new(heartbeat_count: Arc<AtomicUsize>) -> Self {
        Self {
            heartbeat_count,
            heartbeat_timeout: Duration::from_millis(300),
        }
    }
}

impl CoordinatorTrait for HeartbeatCountingCoordinator {
    type Guard = HeartbeatCountingGuard;

    fn try_acquire(&self) -> impl std::future::Future<Output = Option<Self::Guard>> + Send + '_ {
        async {
            Some(HeartbeatCountingGuard {
                heartbeat_count: self.heartbeat_count.clone(),
                last_heartbeat: Arc::new(Mutex::new(Instant::now())),
                heartbeat_timeout: self.heartbeat_timeout,
            })
        }
    }
}

/// Guard that increments counter on heartbeat.
struct HeartbeatCountingGuard {
    heartbeat_count: Arc<AtomicUsize>,
    last_heartbeat: Arc<Mutex<Instant>>,
    heartbeat_timeout: Duration,
}

impl GuardTrait for HeartbeatCountingGuard {
    fn is_valid(&self) -> bool {
        let last = self.last_heartbeat.lock().unwrap();
        last.elapsed() < self.heartbeat_timeout
    }

    fn heartbeat(&self) {
        self.heartbeat_count.fetch_add(1, Ordering::SeqCst);
        *self.last_heartbeat.lock().unwrap() = Instant::now();
    }
}

#[tokio::test]
async fn runner_sends_heartbeats_during_event_processing() {
    // Given: heartbeat tracking coordinator
    let heartbeat_count = Arc::new(AtomicUsize::new(0));
    let coordinator = HeartbeatCountingCoordinator::new(heartbeat_count.clone());

    // Given: projector that simulates work
    let projector = HeartbeatCountingProjector::new();

    // Given: event store with events to process
    let store = InMemoryEventStore::new();
    let counter_id = StreamId::try_new("counter-1").expect("valid stream id");

    // Seed 10 events (with 100ms processing each = 1 second total)
    for i in 0..10 {
        let event = CounterIncremented {
            counter_id: counter_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(counter_id.clone(), StreamVersion::new(i))
            .expect("register stream")
            .append(event)
            .expect("append event");
        store
            .append_events(writes)
            .await
            .expect("append to succeed");
    }

    // When: runner processes events with heartbeat enabled (200ms interval)
    let runner = ProjectionRunner::new(projector, coordinator, &store).with_heartbeat_config(
        HeartbeatConfig {
            heartbeat_interval: std::time::Duration::from_millis(200),
            heartbeat_timeout: std::time::Duration::from_secs(5),
        },
    );

    let result = runner.run().await;

    // Then: runner completed successfully
    assert!(
        result.is_ok(),
        "Runner should complete successfully: {:?}",
        result
    );

    // Then: heartbeat was called at least once during processing
    // With 1 second of processing and 200ms interval, expect at least 4 heartbeats
    let heartbeats = heartbeat_count.load(Ordering::SeqCst);
    assert!(
        heartbeats >= 4,
        "Expected at least 4 heartbeats during 1 second of processing with 200ms interval, got {}",
        heartbeats
    );
}

#[tokio::test]
async fn guard_becomes_invalid_after_heartbeat_timeout() {
    // Given: coordinator with short heartbeat timeout (300ms)
    let heartbeat_count = Arc::new(AtomicUsize::new(0));
    let coordinator = HeartbeatCountingCoordinator::new(heartbeat_count.clone());

    // When: projector acquires guard and stops sending heartbeats (simulating hang)
    let guard = coordinator
        .try_acquire()
        .await
        .expect("should acquire leadership");

    // Wait for timeout to elapse without calling heartbeat()
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    // Then: guard becomes invalid after timeout
    assert!(
        !guard.is_valid(),
        "Guard should be invalid after heartbeat timeout elapsed without heartbeat"
    );
}

/// Coordinator that tracks shared leadership state across multiple instances.
struct SharedLeadershipCoordinator {
    state: Arc<Mutex<SharedLeadershipState>>,
    heartbeat_timeout: Duration,
}

/// Shared state for distributed leadership tracking.
struct SharedLeadershipState {
    current_guard_id: Option<usize>,
    last_heartbeat: Option<Instant>,
    next_guard_id: usize,
}

impl SharedLeadershipCoordinator {
    fn new(heartbeat_timeout: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedLeadershipState {
                current_guard_id: None,
                last_heartbeat: None,
                next_guard_id: 1,
            })),
            heartbeat_timeout,
        }
    }
}

impl CoordinatorTrait for SharedLeadershipCoordinator {
    type Guard = SharedLeadershipGuard;

    fn try_acquire(&self) -> impl std::future::Future<Output = Option<Self::Guard>> + Send + '_ {
        async {
            let mut state = self.state.lock().unwrap();

            // Check if there's a current guard and if it's still valid
            let can_acquire = match (state.current_guard_id, state.last_heartbeat) {
                (Some(_), Some(last_heartbeat)) => {
                    // There's a guard, check if it timed out
                    last_heartbeat.elapsed() >= self.heartbeat_timeout
                }
                _ => {
                    // No guard or no heartbeat, can acquire
                    true
                }
            };

            if can_acquire {
                // Create new guard with unique ID
                let guard_id = state.next_guard_id;
                state.next_guard_id += 1;
                state.current_guard_id = Some(guard_id);
                state.last_heartbeat = Some(Instant::now());

                Some(SharedLeadershipGuard {
                    guard_id,
                    state: self.state.clone(),
                    heartbeat_timeout: self.heartbeat_timeout,
                })
            } else {
                None
            }
        }
    }
}

/// Guard that validates against shared leadership state.
struct SharedLeadershipGuard {
    guard_id: usize,
    state: Arc<Mutex<SharedLeadershipState>>,
    heartbeat_timeout: Duration,
}

impl GuardTrait for SharedLeadershipGuard {
    fn is_valid(&self) -> bool {
        let state = self.state.lock().unwrap();

        // Check if this guard still holds leadership
        if state.current_guard_id != Some(self.guard_id) {
            return false;
        }

        // Check if heartbeat has not timed out
        match state.last_heartbeat {
            Some(last_heartbeat) => last_heartbeat.elapsed() < self.heartbeat_timeout,
            None => false,
        }
    }

    fn heartbeat(&self) {
        let mut state = self.state.lock().unwrap();
        if state.current_guard_id == Some(self.guard_id) {
            state.last_heartbeat = Some(Instant::now());
        }
    }
}

#[tokio::test]
async fn new_instance_takes_over_from_hung_projector() {
    // Given: coordinator with shared leadership state
    let coordinator = SharedLeadershipCoordinator::new(Duration::from_millis(300));

    // Given: first projector acquires leadership
    let _first_guard = coordinator
        .try_acquire()
        .await
        .expect("first instance should acquire leadership");

    // Given: first projector stops sending heartbeats (simulating hang)
    // Wait for timeout to elapse without calling heartbeat()
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    // When: second projector tries to acquire leadership after timeout
    let second_guard = coordinator.try_acquire().await;

    // Then: second instance successfully acquires leadership
    assert!(
        second_guard.is_some(),
        "Second instance should acquire leadership after first instance's guard timed out"
    );
}
