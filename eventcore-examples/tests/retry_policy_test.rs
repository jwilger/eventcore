//! Integration test for retry policy and metrics hooks.
//!
//! This test demonstrates:
//! - MetricsHook integration for observability during retries
//! - Verifying RetryContext contains correct attempt numbers
//! - Testing retry behavior with injected version conflicts

use eventcore::{
    AttemptNumber, CommandError, CommandLogic, CommandStreams, DelayMilliseconds, Event,
    EventStore, EventStoreError, EventStreamReader, EventStreamSlice, MetricsHook, NewEvents,
    RetryContext, RetryPolicy, StreamDeclarations, StreamId, StreamWrites, execute,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// =============================================================================
// Test Domain Types
// =============================================================================

/// Simple test event for retry scenarios.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

/// Simple test command for triggering retries.
struct TestCommand {
    stream_id: StreamId,
}

impl CommandStreams for TestCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::single(self.stream_id.clone())
    }
}

impl CommandLogic for TestCommand {
    type Event = TestEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![TestEvent {
            stream_id: self.stream_id.clone(),
        }]
        .into())
    }
}

// =============================================================================
// Test Infrastructure
// =============================================================================

/// Event store wrapper that injects N version conflicts before succeeding.
///
/// This is test infrastructure (not a public API example) that enables
/// deterministic testing of retry behavior by controlling exactly how
/// many conflicts occur before the store allows a write to succeed.
struct ConflictNTimesStore {
    inner: InMemoryEventStore,
    conflict_count: Arc<tokio::sync::Mutex<u32>>,
    conflicts_to_inject: u32,
}

impl ConflictNTimesStore {
    fn new(conflicts_to_inject: u32) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            conflict_count: Arc::new(tokio::sync::Mutex::new(0)),
            conflicts_to_inject,
        }
    }
}

impl EventStore for ConflictNTimesStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut count = self.conflict_count.lock().await;
        if *count < self.conflicts_to_inject {
            // Inject conflict
            *count += 1;
            Err(EventStoreError::VersionConflict)
        } else {
            // Succeed normally
            self.inner.append_events(writes).await
        }
    }
}

// =============================================================================
// Test Helpers
// =============================================================================

fn test_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

// =============================================================================
// Integration Tests
// =============================================================================

/// Scenario: MetricsHook receives correct retry attempt numbers
///
/// Given: A metrics hook that captures all RetryContext values
/// And: A retry policy allowing up to 4 retries with the metrics hook
/// And: An event store that conflicts 3 times before succeeding
/// When: A command is executed
/// Then: The command succeeds after 3 retries
/// And: The metrics hook receives exactly 3 RetryContext values
/// And: The first retry has attempt=1
/// And: The second retry has attempt=2
/// And: The third retry has attempt=3
/// And: All retries reference the declared stream
/// And: All retries have non-zero delay_ms values
#[tokio::test]
async fn metrics_hook_receives_correct_attempt_numbers() {
    // Given: A metrics hook that captures all RetryContext values
    struct ContextCapturingHook {
        contexts: Arc<Mutex<Vec<RetryContext>>>,
    }

    impl MetricsHook for ContextCapturingHook {
        fn on_retry_attempt(&self, ctx: &RetryContext) {
            self.contexts.lock().unwrap().push(ctx.clone());
        }
    }

    let captured_contexts = Arc::new(Mutex::new(Vec::new()));
    let hook = ContextCapturingHook {
        contexts: Arc::clone(&captured_contexts),
    };

    // And: A retry policy allowing up to 4 retries with the metrics hook
    let policy = RetryPolicy::new().max_retries(4).with_metrics_hook(hook);

    // And: An event store that conflicts 3 times before succeeding
    let store = ConflictNTimesStore::new(3);

    // And: A test command
    let stream_id = test_stream_id();
    let command = TestCommand {
        stream_id: stream_id.clone(),
    };

    // When: The command is executed
    let result = execute(&store, command, policy).await;

    // Then: The command succeeds after 3 retries
    assert!(result.is_ok(), "command should succeed after 3 retries");

    // And: The metrics hook receives exactly 3 RetryContext values
    let contexts = captured_contexts.lock().unwrap();
    assert_eq!(contexts.len(), 3, "should have captured 3 retry contexts");

    // And: The first retry has attempt=1
    assert_eq!(
        contexts[0].attempt,
        AttemptNumber::new(NonZeroU32::new(1).expect("1 is non-zero")),
        "first retry should have attempt=1"
    );

    // And: The second retry has attempt=2
    assert_eq!(
        contexts[1].attempt,
        AttemptNumber::new(NonZeroU32::new(2).expect("2 is non-zero")),
        "second retry should have attempt=2"
    );

    // And: The third retry has attempt=3
    assert_eq!(
        contexts[2].attempt,
        AttemptNumber::new(NonZeroU32::new(3).expect("3 is non-zero")),
        "third retry should have attempt=3"
    );

    // And: All retries reference the declared stream
    assert_eq!(contexts[0].streams, vec![stream_id.clone()]);
    assert_eq!(contexts[1].streams, vec![stream_id.clone()]);
    assert_eq!(contexts[2].streams, vec![stream_id]);

    // And: All retries have non-zero delay_ms values (exponential backoff applied)
    assert!(
        contexts[0].delay_ms > DelayMilliseconds::new(0),
        "first retry should have delay"
    );
    assert!(
        contexts[1].delay_ms > DelayMilliseconds::new(0),
        "second retry should have delay"
    );
    assert!(
        contexts[2].delay_ms > DelayMilliseconds::new(0),
        "third retry should have delay"
    );
}
