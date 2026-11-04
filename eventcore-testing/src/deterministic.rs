//! Deterministic testing utilities for event stores.
//!
//! This module provides wrapper stores that inject predictable failures
//! for testing retry logic and conflict handling.

use eventcore_types::{
    Event, EventStore, EventStoreError, EventStream, EventStreamSlice, StreamId, StreamVersion,
    StreamWrites,
};

/// A wrapper around an event store that injects a deterministic number
/// of version conflicts before delegating to the inner store.
///
/// This is useful for testing retry logic where you need predictable
/// conflict behavior rather than the probabilistic chaos testing approach.
///
/// # Examples
///
/// ```no_run
/// use eventcore_testing::deterministic::DeterministicConflictStore;
/// use eventcore_memory::InMemoryEventStore;
///
/// // Create a store that will fail with VersionConflict twice before succeeding
/// let inner = InMemoryEventStore::new();
/// let store = DeterministicConflictStore::new(inner, 2);
/// # let _ = store;
///
/// // First two append_events calls will return VersionConflict
/// // Third call will delegate to inner store
/// ```
pub struct DeterministicConflictStore<S> {
    inner: S,
    remaining_conflicts: std::sync::atomic::AtomicU32,
}

impl<S> DeterministicConflictStore<S> {
    /// Creates a new `DeterministicConflictStore` that will inject `conflict_count`
    /// version conflicts before delegating to the inner store.
    ///
    /// # Arguments
    ///
    /// * `store` - The inner event store to delegate to after conflicts are exhausted
    /// * `conflict_count` - Number of conflicts to inject before delegation
    pub fn new(store: S, conflict_count: u32) -> Self {
        Self {
            inner: store,
            remaining_conflicts: std::sync::atomic::AtomicU32::new(conflict_count),
        }
    }
}

impl<S> EventStore for DeterministicConflictStore<S>
where
    S: EventStore + Sync,
{
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStream<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut remaining = self
            .remaining_conflicts
            .load(std::sync::atomic::Ordering::SeqCst);
        while remaining > 0 {
            match self.remaining_conflicts.compare_exchange_weak(
                remaining,
                remaining - 1,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Err(EventStoreError::VersionConflict {
                        stream_id: StreamId::try_new("deterministic-conflict").expect("valid"),
                        expected: StreamVersion::new(0),
                        actual: StreamVersion::new(1),
                    });
                }
                Err(actual) => remaining = actual,
            }
        }

        self.inner.append_events(writes).await
    }
}
