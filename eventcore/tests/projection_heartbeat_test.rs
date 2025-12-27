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
