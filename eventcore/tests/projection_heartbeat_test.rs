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
use std::sync::atomic::{AtomicUsize, Ordering};

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
}

impl HeartbeatCountingCoordinator {
    fn new(heartbeat_count: Arc<AtomicUsize>) -> Self {
        Self { heartbeat_count }
    }
}

impl CoordinatorTrait for HeartbeatCountingCoordinator {
    type Guard = HeartbeatCountingGuard;

    fn try_acquire(&self) -> impl std::future::Future<Output = Option<Self::Guard>> + Send + '_ {
        async {
            Some(HeartbeatCountingGuard {
                heartbeat_count: self.heartbeat_count.clone(),
            })
        }
    }
}

/// Guard that increments counter on heartbeat.
struct HeartbeatCountingGuard {
    heartbeat_count: Arc<AtomicUsize>,
}

impl GuardTrait for HeartbeatCountingGuard {
    fn is_valid(&self) -> bool {
        true
    }

    fn heartbeat(&self) {
        self.heartbeat_count.fetch_add(1, Ordering::SeqCst);
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
