//! Integration tests for GitHub issue #372: read_events bug with mixed event types
//!
//! When `read_events` is called, `.take(limit)` is applied BEFORE
//! `.filter_map(.ok())` deserialization. This means non-matching event types
//! consume LIMIT slots. If a batch is full of non-matching events, 0 matching
//! events are returned even though they exist later in the log.
//!
//! Scenario: read_events_returns_matching_events_when_preceded_by_other_types
//! - Given an InMemoryEventStore with 5 AlphaEvents followed by 1 BetaEvent
//! - When read_events::<BetaEvent> is called with a batch size of 3
//! - Then the result should contain exactly 1 BetaEvent
//!
//! Scenario: run_projection_processes_events_when_other_event_types_exist
//! - Given a TestBackend with 5 AlphaEvents and 1 BetaEvent appended
//! - And a projector for BetaEvent
//! - When run_projection is called with default config
//! - Then the projector should have processed exactly 1 event

use eventcore::{Event, ProjectionConfig, Projector, StreamId, StreamPosition, run_projection};
use eventcore_memory::{InMemoryCheckpointStore, InMemoryEventStore, InMemoryProjectorCoordinator};
use eventcore_types::{
    BatchSize, CheckpointStore, EventFilter, EventPage, EventReader, EventStore, EventStoreError,
    ProjectorCoordinator, StreamVersion, StreamWrites,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// AlphaEvent has a required `alpha_value` field that BetaEvent lacks,
/// ensuring serde cannot deserialize an AlphaEvent JSON blob as a BetaEvent
/// (and vice versa). This structural difference is what makes the take-before-
/// filter bug observable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlphaEvent {
    stream_id: StreamId,
    alpha_value: String,
}

impl Event for AlphaEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "AlphaEvent"
    }
}

/// BetaEvent has a required `beta_value` field that AlphaEvent lacks,
/// ensuring serde cannot deserialize a BetaEvent JSON blob as an AlphaEvent
/// (and vice versa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BetaEvent {
    stream_id: StreamId,
    beta_value: String,
}

impl Event for BetaEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "BetaEvent"
    }
}

// ---------------------------------------------------------------------------
// TestBackend (same pattern as projection_config_test.rs)
// ---------------------------------------------------------------------------

struct TestBackend {
    event_store: InMemoryEventStore,
    checkpoint_store: InMemoryCheckpointStore,
    coordinator: InMemoryProjectorCoordinator,
}

impl TestBackend {
    fn new() -> Self {
        Self {
            event_store: InMemoryEventStore::new(),
            checkpoint_store: InMemoryCheckpointStore::new(),
            coordinator: InMemoryProjectorCoordinator::new(),
        }
    }
}

impl EventReader for TestBackend {
    type Error = EventStoreError;

    fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send {
        self.event_store.read_events(filter, page)
    }
}

impl CheckpointStore for TestBackend {
    type Error = eventcore_memory::InMemoryCheckpointError;

    fn load(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<StreamPosition>, Self::Error>> + Send {
        self.checkpoint_store.load(name)
    }

    fn save(
        &self,
        name: &str,
        position: StreamPosition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.checkpoint_store.save(name, position)
    }
}

impl ProjectorCoordinator for TestBackend {
    type Error = eventcore_memory::InMemoryCoordinationError;
    type Guard = eventcore_memory::InMemoryCoordinationGuard;

    fn try_acquire(
        &self,
        subscription_name: &str,
    ) -> impl Future<Output = Result<Self::Guard, Self::Error>> + Send {
        self.coordinator.try_acquire(subscription_name)
    }
}

impl EventStore for TestBackend {
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<eventcore_types::EventStreamReader<E>, EventStoreError>> + Send
    {
        self.event_store.read_stream(stream_id)
    }

    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<Output = Result<eventcore_types::EventStreamSlice, EventStoreError>> + Send
    {
        self.event_store.append_events(writes)
    }
}

// ---------------------------------------------------------------------------
// BetaEvent projector
// ---------------------------------------------------------------------------

struct BetaEventProjector {
    count: Arc<AtomicUsize>,
}

impl BetaEventProjector {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl Projector for BetaEventProjector {
    type Event = BetaEvent;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        _event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let _ = self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "beta-event-projector"
    }
}

// ---------------------------------------------------------------------------
// Helper: seed events into an InMemoryEventStore
// ---------------------------------------------------------------------------

async fn seed_alpha_and_beta_events(store: &InMemoryEventStore) {
    let alpha_stream = StreamId::try_new("alpha-1").expect("valid stream id");

    // Append 5 AlphaEvents, each in its own StreamWrites at incrementing versions
    for version in 0usize..5 {
        let event = AlphaEvent {
            stream_id: alpha_stream.clone(),
            alpha_value: format!("alpha-{version}"),
        };
        let writes = StreamWrites::new()
            .register_stream(alpha_stream.clone(), StreamVersion::new(version))
            .expect("register stream")
            .append(event)
            .expect("append event");
        let _ = store
            .append_events(writes)
            .await
            .expect("append alpha event to succeed");
    }

    // Append 1 BetaEvent
    let beta_stream = StreamId::try_new("beta-1").expect("valid stream id");
    let beta_event = BetaEvent {
        stream_id: beta_stream.clone(),
        beta_value: "beta-0".to_string(),
    };
    let writes = StreamWrites::new()
        .register_stream(beta_stream.clone(), StreamVersion::new(0))
        .expect("register stream")
        .append(beta_event)
        .expect("append event");
    let _ = store
        .append_events(writes)
        .await
        .expect("append beta event to succeed");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Demonstrates the read_events bug: `.take(limit)` is applied before
/// deserialization filtering, so non-matching event types consume batch slots.
///
/// With 5 AlphaEvents followed by 1 BetaEvent and a batch size of 3,
/// the first 3 slots are consumed by AlphaEvents (which fail BetaEvent
/// deserialization). The BetaEvent at position 6 is never reached.
#[tokio::test]
async fn read_events_returns_matching_events_when_preceded_by_other_types() {
    // Given: An InMemoryEventStore
    let store = InMemoryEventStore::new();

    // And: 5 AlphaEvents followed by 1 BetaEvent are appended
    seed_alpha_and_beta_events(&store).await;

    // When: read_events::<BetaEvent> is called with a batch size of 3
    let events: Vec<(BetaEvent, StreamPosition)> = store
        .read_events(EventFilter::all(), EventPage::first(BatchSize::new(3)))
        .await
        .expect("read_events should not error");

    // Then: The result should contain exactly 1 BetaEvent
    assert_eq!(
        events.len(),
        1,
        "expected 1 BetaEvent but got {}; take(limit) is applied before deserialization filtering",
        events.len()
    );
}

/// Full integration test using run_projection to verify that a projector for
/// BetaEvent processes events even when other event types exist in the store.
#[tokio::test]
async fn run_projection_processes_events_when_other_event_types_exist() {
    // Given: A TestBackend with 5 AlphaEvents and 1 BetaEvent appended
    let backend = TestBackend::new();
    seed_alpha_and_beta_events(&backend.event_store).await;

    // And: A projector for BetaEvent
    let event_count = Arc::new(AtomicUsize::new(0));
    let projector = BetaEventProjector::new(event_count.clone());

    // When: run_projection is called with default config
    let config = ProjectionConfig::default();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_projection(projector, &backend, config),
    )
    .await
    .expect("should complete within timeout");

    // Then: run_projection should succeed
    assert!(
        result.is_ok(),
        "run_projection should succeed: {:?}",
        result
    );

    // And: The projector should have processed exactly 1 BetaEvent
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        1,
        "projector should have processed exactly 1 BetaEvent"
    );
}
