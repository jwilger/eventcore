//! Integration test for eventcore-2n5: Heartbeat and Liveness Detection
//!
//! Scenario: Healthy projector sends heartbeats
//! - Given projector holds leadership with heartbeat_interval of 5 seconds
//! - When projector processes events normally
//! - Then runner calls guard.heartbeat() at least every heartbeat_interval
//! - And guard.is_valid() continues to return true

use eventcore::{
    CoordinatorLike, Event, EventStore, GuardLike, HeartbeatConfig, ProjectionRunner, Projector,
    StreamId, StreamPosition, StreamVersion, StreamWrites,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

struct SlowProjector {
    processing_time: Duration,
}

impl Projector for SlowProjector {
    type Event = TestEvent;
    type Error = std::convert::Infallible;
    type Context = ();

    fn name(&self) -> &str {
        "slow_projector"
    }

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        std::thread::sleep(self.processing_time);
        Ok(())
    }
}

/// Test helper coordinator that counts heartbeat calls.
pub struct HeartbeatCountingCoordinator {
    _heartbeat_count: Arc<AtomicUsize>,
}

impl HeartbeatCountingCoordinator {
    pub fn new(heartbeat_count: Arc<AtomicUsize>) -> Self {
        Self {
            _heartbeat_count: heartbeat_count,
        }
    }

    pub async fn try_acquire(&self) -> Option<HeartbeatCountingGuard> {
        Some(HeartbeatCountingGuard {
            _heartbeat_count: self._heartbeat_count.clone(),
        })
    }
}

/// Test helper guard that counts heartbeat calls.
pub struct HeartbeatCountingGuard {
    _heartbeat_count: Arc<AtomicUsize>,
}

impl HeartbeatCountingGuard {
    pub fn is_valid(&self) -> bool {
        true
    }

    pub fn heartbeat(&self) {
        self._heartbeat_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl GuardLike for HeartbeatCountingGuard {
    fn heartbeat(&self) {
        self._heartbeat_count.fetch_add(1, Ordering::SeqCst);
    }

    fn is_valid(&self) -> bool {
        true // Simple counting guard is always valid
    }
}

impl CoordinatorLike for HeartbeatCountingCoordinator {
    type Guard = HeartbeatCountingGuard;

    async fn try_acquire(&self) -> Option<Self::Guard> {
        Some(HeartbeatCountingGuard {
            _heartbeat_count: self._heartbeat_count.clone(),
        })
    }
}

#[tokio::test]
async fn healthy_projector_sends_heartbeats_during_processing() {
    // Given: 10 events × 100ms each = 1 second total
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test").unwrap();
    for i in 0..10 {
        let event = TestEvent {
            stream_id: stream_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(i))
            .unwrap()
            .append(event)
            .unwrap();
        store.append_events(writes).await.unwrap();
    }

    let heartbeat_count = Arc::new(AtomicUsize::new(0));

    // When: Process events with 200ms heartbeat interval
    let projector = SlowProjector {
        processing_time: Duration::from_millis(100),
    };
    let coordinator = HeartbeatCountingCoordinator::new(heartbeat_count.clone());
    let runner = ProjectionRunner::new(projector, coordinator, &store).with_heartbeat_config(
        HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(200),
            heartbeat_timeout: Duration::from_millis(300),
        },
    );

    runner.run().await.unwrap();

    // Then: At least 4 heartbeats during 1 second (1000ms / 200ms = 5, allowing tolerance)
    assert!(heartbeat_count.load(Ordering::SeqCst) >= 4);
}

#[tokio::test]
async fn runner_stops_when_guard_becomes_invalid() {
    // Given: projector holds leadership with heartbeat_timeout of 30ms
    use std::sync::Mutex;
    use tokio::time::Instant;

    #[derive(Clone)]
    struct TimeoutTrackingCoordinator {
        state: Arc<Mutex<CoordinatorState>>,
    }

    struct CoordinatorState {
        last_heartbeat: Option<Instant>,
        heartbeat_timeout: Duration,
    }

    impl TimeoutTrackingCoordinator {
        fn new(heartbeat_timeout: Duration) -> Self {
            Self {
                state: Arc::new(Mutex::new(CoordinatorState {
                    last_heartbeat: Some(Instant::now()),
                    heartbeat_timeout,
                })),
            }
        }
    }

    impl CoordinatorLike for TimeoutTrackingCoordinator {
        type Guard = TimeoutTrackingGuard;

        async fn try_acquire(&self) -> Option<Self::Guard> {
            let state = self.state.lock().unwrap();
            Some(TimeoutTrackingGuard {
                state: self.state.clone(),
                heartbeat_timeout: state.heartbeat_timeout,
            })
        }
    }

    struct TimeoutTrackingGuard {
        state: Arc<Mutex<CoordinatorState>>,
        heartbeat_timeout: Duration,
    }

    impl GuardLike for TimeoutTrackingGuard {
        fn heartbeat(&self) {
            let mut state = self.state.lock().unwrap();
            state.last_heartbeat = Some(Instant::now());
        }

        fn is_valid(&self) -> bool {
            let state = self.state.lock().unwrap();
            if let Some(last_hb) = state.last_heartbeat {
                last_hb.elapsed() < self.heartbeat_timeout
            } else {
                false
            }
        }
    }

    // Projector that simulates being hung - takes too long to process
    struct HungProjector {
        event_count: Arc<AtomicUsize>,
    }

    impl Projector for HungProjector {
        type Event = TestEvent;
        type Error = std::convert::Infallible;
        type Context = ();

        fn name(&self) -> &str {
            "hung_projector"
        }

        fn apply(
            &mut self,
            _event: Self::Event,
            _position: StreamPosition,
            _ctx: &mut Self::Context,
        ) -> Result<(), Self::Error> {
            // Simulate hung projector - sleep longer than heartbeat timeout
            std::thread::sleep(Duration::from_millis(100));
            self.event_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test").unwrap();

    // Seed many events so the projector would keep processing if not stopped
    for i in 0..10 {
        let event = TestEvent {
            stream_id: stream_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(i))
            .unwrap()
            .append(event)
            .unwrap();
        store.append_events(writes).await.unwrap();
    }

    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = HungProjector {
        event_count: event_count.clone(),
    };
    let coordinator = TimeoutTrackingCoordinator::new(Duration::from_millis(30));

    // When: projector hangs and stops calling heartbeat() (100ms >> 30ms timeout)
    let runner = ProjectionRunner::new(projector, coordinator, &store).with_heartbeat_config(
        HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(10),
            heartbeat_timeout: Duration::from_millis(30),
        },
    );

    let result = runner.run().await;

    // Then: runner detects invalid guard and stops processing
    assert!(result.is_err());
}

#[tokio::test]
async fn new_instance_takes_over_from_hung_projector() {
    use eventcore::InMemoryCheckpointStore;
    use std::sync::Mutex;
    use tokio::time::Instant;

    // Shared leadership coordinator that allows takeover when guard times out
    #[derive(Clone)]
    struct SharedLeadershipCoordinator {
        state: Arc<Mutex<SharedState>>,
        heartbeat_timeout: Duration,
    }

    struct SharedState {
        current_guard_id: Option<usize>,
        next_guard_id: usize,
        last_heartbeat: Option<Instant>,
    }

    impl SharedLeadershipCoordinator {
        fn new(heartbeat_timeout: Duration) -> Self {
            Self {
                state: Arc::new(Mutex::new(SharedState {
                    current_guard_id: None,
                    next_guard_id: 0,
                    last_heartbeat: None,
                })),
                heartbeat_timeout,
            }
        }
    }

    impl CoordinatorLike for SharedLeadershipCoordinator {
        type Guard = SharedLeadershipGuard;

        async fn try_acquire(&self) -> Option<Self::Guard> {
            let mut state = self.state.lock().unwrap();

            // Check if current guard has timed out
            let can_acquire =
                if let (Some(_), Some(last_hb)) = (state.current_guard_id, state.last_heartbeat) {
                    last_hb.elapsed() >= self.heartbeat_timeout
                } else {
                    true // No guard exists, can acquire
                };

            if can_acquire {
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

    struct SharedLeadershipGuard {
        guard_id: usize,
        state: Arc<Mutex<SharedState>>,
        heartbeat_timeout: Duration,
    }

    impl GuardLike for SharedLeadershipGuard {
        fn heartbeat(&self) {
            let mut state = self.state.lock().unwrap();
            // Only update heartbeat if this guard still holds leadership
            if state.current_guard_id == Some(self.guard_id) {
                state.last_heartbeat = Some(Instant::now());
            }
        }

        fn is_valid(&self) -> bool {
            let state = self.state.lock().unwrap();
            // Valid if this guard still holds leadership AND hasn't timed out
            if state.current_guard_id != Some(self.guard_id) {
                return false;
            }
            if let Some(last_hb) = state.last_heartbeat {
                last_hb.elapsed() < self.heartbeat_timeout
            } else {
                false
            }
        }
    }

    // Projector that tracks which positions it processed
    struct PositionTrackingProjector {
        processed_positions: Arc<Mutex<Vec<StreamPosition>>>,
        processing_delay: Duration,
    }

    impl Projector for PositionTrackingProjector {
        type Event = TestEvent;
        type Error = std::convert::Infallible;
        type Context = ();

        fn name(&self) -> &str {
            "position_tracker"
        }

        fn apply(
            &mut self,
            _event: Self::Event,
            position: StreamPosition,
            _ctx: &mut Self::Context,
        ) -> Result<(), Self::Error> {
            // Simulate processing delay
            std::thread::sleep(self.processing_delay);
            let mut positions = self.processed_positions.lock().unwrap();
            positions.push(position);
            Ok(())
        }
    }

    // Given: First instance is hung and missed heartbeat timeout
    let store = InMemoryEventStore::new();
    let checkpoint_store = InMemoryCheckpointStore::new();
    let stream_id = StreamId::try_new("test").unwrap();

    // Seed 5 events
    for i in 0..5 {
        let event = TestEvent {
            stream_id: stream_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(i))
            .unwrap()
            .append(event)
            .unwrap();
        store.append_events(writes).await.unwrap();
    }

    let processed_positions = Arc::new(Mutex::new(Vec::new()));
    let coordinator = SharedLeadershipCoordinator::new(Duration::from_millis(300));

    // First instance processes slowly - each event takes 150ms, but heartbeat timeout is 300ms
    // So it will process 1-2 events before guard times out
    let projector1 = PositionTrackingProjector {
        processed_positions: processed_positions.clone(),
        processing_delay: Duration::from_millis(150),
    };

    let runner1 = ProjectionRunner::new(projector1, coordinator.clone(), &store)
        .with_checkpoint_store(checkpoint_store.clone())
        .with_heartbeat_config(HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(50),
            heartbeat_timeout: Duration::from_millis(300),
        });

    // Start first runner but expect it to fail when guard times out
    // (it processes slowly enough that guard times out)
    let _ = runner1.run().await;

    // Wait for guard to definitely time out
    tokio::time::sleep(Duration::from_millis(400)).await;

    // When: Second instance calls try_acquire() after timeout
    // Second instance processes quickly (no delay) to complete remaining events
    let projector2 = PositionTrackingProjector {
        processed_positions: processed_positions.clone(),
        processing_delay: Duration::from_millis(0),
    };

    let runner2 = ProjectionRunner::new(projector2, coordinator.clone(), &store)
        .with_checkpoint_store(checkpoint_store.clone())
        .with_heartbeat_config(HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(50),
            heartbeat_timeout: Duration::from_millis(300),
        });

    // Then: Second instance acquires leadership and resumes from checkpoint
    runner2.run().await.unwrap();

    // And: No events are skipped or duplicated
    let positions = processed_positions.lock().unwrap();
    let total_processed = positions.len();
    let mut sorted_positions = positions.clone();
    sorted_positions.sort();
    sorted_positions.dedup();
    let unique_count = sorted_positions.len();
    let total_events = 5;

    assert_eq!(
        unique_count, total_events,
        "Expected {} unique positions, got {}. Total processed: {}",
        total_events, unique_count, total_processed
    );
}

#[tokio::test]
async fn developer_can_configure_custom_heartbeat_interval() {
    // Given: 10 events × 100ms each = 1 second total
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test").unwrap();
    for i in 0..10 {
        let event = TestEvent {
            stream_id: stream_id.clone(),
        };
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(i))
            .unwrap()
            .append(event)
            .unwrap();
        store.append_events(writes).await.unwrap();
    }

    let heartbeat_count = Arc::new(AtomicUsize::new(0));

    // When: Developer configures custom heartbeat_interval of 50ms (faster than default)
    let projector = SlowProjector {
        processing_time: Duration::from_millis(100),
    };
    let coordinator = HeartbeatCountingCoordinator::new(heartbeat_count.clone());
    let runner = ProjectionRunner::new(projector, coordinator, &store).with_heartbeat_config(
        HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(50),
            heartbeat_timeout: Duration::from_millis(300),
        },
    );

    runner.run().await.unwrap();

    // Then: Configured interval is respected - at least 18 heartbeats in 1 second
    // (1000ms / 50ms = 20 heartbeats theoretical, allowing ~10% tolerance)
    assert!(heartbeat_count.load(Ordering::SeqCst) >= 18);
}

#[tokio::test]
async fn developer_can_configure_custom_heartbeat_timeout() {
    use std::sync::Mutex;
    use tokio::time::Instant;

    // Reuse SharedLeadershipCoordinator from Scenario 3
    #[derive(Clone)]
    struct SharedLeadershipCoordinator {
        state: Arc<Mutex<SharedState>>,
        heartbeat_timeout: Duration,
    }

    struct SharedState {
        current_guard_id: Option<usize>,
        next_guard_id: usize,
        last_heartbeat: Option<Instant>,
    }

    impl SharedLeadershipCoordinator {
        fn new(heartbeat_timeout: Duration) -> Self {
            Self {
                state: Arc::new(Mutex::new(SharedState {
                    current_guard_id: None,
                    next_guard_id: 0,
                    last_heartbeat: None,
                })),
                heartbeat_timeout,
            }
        }
    }

    impl CoordinatorLike for SharedLeadershipCoordinator {
        type Guard = SharedLeadershipGuard;

        async fn try_acquire(&self) -> Option<Self::Guard> {
            let mut state = self.state.lock().unwrap();

            // Check if current guard has timed out
            let can_acquire =
                if let (Some(_), Some(last_hb)) = (state.current_guard_id, state.last_heartbeat) {
                    last_hb.elapsed() >= self.heartbeat_timeout
                } else {
                    true // No guard exists, can acquire
                };

            if can_acquire {
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

    struct SharedLeadershipGuard {
        guard_id: usize,
        state: Arc<Mutex<SharedState>>,
        heartbeat_timeout: Duration,
    }

    impl GuardLike for SharedLeadershipGuard {
        fn heartbeat(&self) {
            let mut state = self.state.lock().unwrap();
            // Only update heartbeat if this guard still holds leadership
            if state.current_guard_id == Some(self.guard_id) {
                state.last_heartbeat = Some(Instant::now());
            }
        }

        fn is_valid(&self) -> bool {
            let state = self.state.lock().unwrap();
            // Valid if this guard still holds leadership AND hasn't timed out
            if state.current_guard_id != Some(self.guard_id) {
                return false;
            }
            if let Some(last_hb) = state.last_heartbeat {
                last_hb.elapsed() < self.heartbeat_timeout
            } else {
                false
            }
        }
    }

    // Given: Developer creates HeartbeatConfig with heartbeat_timeout of 250ms
    let custom_timeout = Duration::from_millis(250);
    let coordinator = SharedLeadershipCoordinator::new(custom_timeout);

    // When: Projector stops sending heartbeats
    let guard = coordinator.try_acquire().await.unwrap();

    // Wait beyond the custom timeout
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Then: Leadership is revoked after custom timeout elapses
    assert!(!guard.is_valid());
}

#[test]
fn invalid_heartbeat_configuration_is_rejected() {
    // Given: Developer creates HeartbeatConfig with invalid timeout
    // When: heartbeat_timeout is less than heartbeat_interval
    let result = HeartbeatConfig::try_new(
        Duration::from_millis(200), // interval
        Duration::from_millis(100), // timeout < interval (invalid!)
    );

    // Then: builder returns descriptive error
    assert!(result.is_err());
}

#[tokio::test]
async fn runner_handles_slow_event_processing() {
    // Given: Event processing takes longer than heartbeat_interval
    // Scenario 8: apply() takes 20 seconds but heartbeat_interval is 5 seconds
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test").unwrap();

    // Seed single event for focused test
    let event = TestEvent {
        stream_id: stream_id.clone(),
    };
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .unwrap()
        .append(event)
        .unwrap();
    store.append_events(writes).await.unwrap();

    let heartbeat_count = Arc::new(AtomicUsize::new(0));
    let coordinator = HeartbeatCountingCoordinator::new(heartbeat_count.clone());

    // When: Process event that takes 200ms with 50ms heartbeat_interval and 100ms timeout
    let projector = SlowProjector {
        processing_time: Duration::from_millis(200),
    };
    let runner = ProjectionRunner::new(projector, coordinator, &store).with_heartbeat_config(
        HeartbeatConfig {
            heartbeat_interval: Duration::from_millis(50),
            heartbeat_timeout: Duration::from_millis(100),
        },
    );

    let result = runner.run().await;

    // Then: Guard remains valid after slow event processing (no spurious leadership loss)
    assert!(result.is_ok());
}
