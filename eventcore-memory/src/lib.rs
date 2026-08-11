//! In-memory event store implementation for testing.
//!
//! This module provides the `InMemoryEventStore` - a lightweight, zero-dependency
//! storage backend for EventCore integration tests and development.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use eventcore_types::{
    CheckpointStore, CommandStateSnapshot, CommandStateSnapshotId, Event, EventFilter, EventPage,
    EventReader, EventStore, EventStoreError, EventStream, EventStreamSlice, Operation,
    ProjectorCoordinator, StreamId, StreamPosition, StreamVersion, StreamWriteEntry, StreamWrites,
};
use uuid::Uuid;

type StreamData = (Vec<Box<dyn std::any::Any + Send>>, StreamVersion);

/// Entry in the global event log with indexed stream_id for efficient filtering.
///
/// This structure mirrors the Postgres schema where stream_id is a separate
/// indexed column and event_id (UUID7) serves as the global position.
/// By storing stream_id and event_id separately, we can filter by stream
/// prefix and position without parsing JSON, matching the performance
/// characteristics of the database implementation.
#[derive(Debug, Clone)]
struct GlobalLogEntry {
    /// Event identifier (UUID7), used as global position
    event_id: Uuid,
    /// Stream identifier, extracted at write time for efficient filtering
    stream_id: String,
    /// Event type name, stored at write time for efficient type filtering
    event_type: String,
    /// Event data as serialized JSON (serialized once at append time)
    event_data: String,
}

/// Internal storage combining per-stream data with global event ordering.
struct StoreData {
    streams: HashMap<StreamId, StreamData>,
    /// Global log with indexed stream_id for efficient EventReader queries
    global_log: Vec<GlobalLogEntry>,
    /// Checkpoint storage for projection progress tracking
    checkpoints: HashMap<String, StreamPosition>,
    /// Durable-in-process command-state projections.
    command_state_snapshots: HashMap<CommandStateSnapshotId, CommandStateSnapshot>,
    /// Coordination locks for projector leadership
    locks: Arc<RwLock<HashMap<String, ()>>>,
}

/// In-memory event store implementation for testing.
///
/// `InMemoryEventStore` provides a lightweight, zero-dependency storage backend
/// for EventCore integration tests and development. It implements the `EventStore`,
/// `EventReader`, `CheckpointStore`, and `ProjectorCoordinator` traits using
/// standard library collections with optimistic concurrency control via version
/// checking.
///
/// # Example
///
/// ```no_run
/// use eventcore_memory::InMemoryEventStore;
///
/// let store = InMemoryEventStore::new();
/// // Use store with execute() function
/// ```
///
/// # Thread Safety
///
/// `InMemoryEventStore` uses interior mutability (`Mutex`) for concurrent access.
pub struct InMemoryEventStore {
    data: std::sync::Mutex<StoreData>,
}

impl InMemoryEventStore {
    /// Create a new in-memory event store.
    ///
    /// Returns an empty event store ready for command execution.
    /// All streams start at version 0 (no events).
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(StoreData {
                streams: HashMap::new(),
                global_log: Vec::new(),
                checkpoints: HashMap::new(),
                command_state_snapshots: HashMap::new(),
                locks: Arc::new(RwLock::new(HashMap::new())),
            }),
        }
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStream<E>, EventStoreError> {
        // The in-memory store keeps events behind a lock as type-erased
        // `Box<dyn Any>`. Producing an owned `E` per item requires a downcast
        // and clone (expected — see #363), so we materialize the per-event
        // results while holding the lock, then release it and yield the items
        // one at a time. The stream is still consumed incrementally by the
        // executor; only this local, in-process backend buffers the owned
        // clones (which already existed in memory).
        let items: Vec<Result<E, EventStoreError>> = {
            let data = self
                .data
                .lock()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                })?;
            match data.streams.get(&stream_id) {
                None => Vec::new(),
                Some((boxed_events, _version)) => boxed_events
                    .iter()
                    .map(|boxed| match boxed.downcast_ref::<E>() {
                        Some(event) => Ok(event.clone()),
                        None => Err(EventStoreError::DeserializationFailed {
                            stream_id: stream_id.clone(),
                            detail: format!(
                                "event could not be downcast to {}",
                                std::any::type_name::<E>()
                            ),
                        }),
                    })
                    .collect(),
            }
        };

        Ok(EventStream::new(futures::stream::iter(items)))
    }

    async fn read_stream_after<E: Event>(
        &self,
        stream_id: StreamId,
        exclusive_version: StreamVersion,
    ) -> Result<EventStream<E>, EventStoreError> {
        let count: usize = exclusive_version.into();
        let items: Vec<Result<E, EventStoreError>> = {
            let data = self
                .data
                .lock()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                })?;
            match data.streams.get(&stream_id) {
                None => Vec::new(),
                Some((boxed_events, _version)) => boxed_events
                    .iter()
                    .skip(count)
                    .map(|boxed| match boxed.downcast_ref::<E>() {
                        Some(event) => Ok(event.clone()),
                        None => Err(EventStoreError::DeserializationFailed {
                            stream_id: stream_id.clone(),
                            detail: format!(
                                "event could not be downcast to {}",
                                std::any::type_name::<E>()
                            ),
                        }),
                    })
                    .collect(),
            }
        };

        Ok(EventStream::new(futures::stream::iter(items)))
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::AppendEvents,
            })?;
        let expected_versions = writes.expected_versions().clone();

        // Check all version constraints before writing any events
        for (stream_id, expected_version) in &expected_versions {
            let current_version = data
                .streams
                .get(stream_id)
                .map(|(_events, version)| *version)
                .unwrap_or_else(|| StreamVersion::new(0));

            if current_version != *expected_version {
                return Err(EventStoreError::VersionConflict {
                    stream_id: stream_id.clone(),
                    expected: *expected_version,
                    actual: current_version,
                });
            }
        }

        // All versions match - proceed with writes
        for entry in writes.into_entries() {
            let StreamWriteEntry {
                stream_id,
                event,
                event_type,
                event_data,
            } = entry;

            // Generate UUID7 for this event (monotonic, timestamp-ordered)
            let event_id = Uuid::now_v7();

            // Store in global log for EventReader with indexed stream_id, event_type, and event_id.
            // event_data is already serialized JSON; keep the raw string.
            data.global_log.push(GlobalLogEntry {
                event_id,
                stream_id: stream_id.as_ref().to_string(),
                event_type: event_type.to_string(),
                event_data: event_data.get().to_owned(),
            });

            let (events, version) = data
                .streams
                .entry(stream_id)
                .or_insert_with(|| (Vec::new(), StreamVersion::new(0)));
            events.push(event);
            *version = version.increment();
        }

        Ok(EventStreamSlice)
    }

    async fn load_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
    ) -> Result<Option<CommandStateSnapshot>, EventStoreError> {
        let data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })?;
        Ok(data.command_state_snapshots.get(&snapshot_id).cloned())
    }

    async fn save_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
        snapshot: CommandStateSnapshot,
    ) -> Result<(), EventStoreError> {
        let mut data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::AppendEvents,
            })?;
        match data.command_state_snapshots.get(&snapshot_id) {
            Some(current) if !snapshot.covers(current) => {}
            _ => {
                let _ = data.command_state_snapshots.insert(snapshot_id, snapshot);
            }
        }
        Ok(())
    }
}

impl EventReader for InMemoryEventStore {
    type Error = EventStoreError;

    async fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        let data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })?;

        let after_event_id = page.after_position().map(|p| p.into_inner());

        let events: Vec<(E, StreamPosition)> = data
            .global_log
            .iter()
            .filter(|entry| {
                // Filter by event_id (UUID7 comparison)
                match after_event_id {
                    None => true,
                    Some(after_id) => entry.event_id > after_id,
                }
            })
            .filter(|entry| {
                // Filter by indexed stream_id WITHOUT parsing JSON (matches Postgres behavior)
                match filter.stream_prefix() {
                    None => true,
                    Some(prefix) => entry.stream_id.starts_with(prefix.as_ref()),
                }
            })
            .filter(|entry| {
                // Filter by glob pattern (ADR-0047) at the query level, before
                // take(), so non-matching streams don't consume batch slots.
                match filter.stream_pattern() {
                    None => true,
                    Some(pattern) => pattern.matches(&entry.stream_id),
                }
            })
            .filter(|entry| {
                // Filter by event_type BEFORE take() so non-matching types
                // don't consume batch slots (fixes issue #372).
                // Use explicit filter if set, otherwise derive from E::event_type_name().
                let type_filter = filter.event_type().unwrap_or_else(|| E::event_type_name());
                entry.event_type == type_filter
            })
            .take(page.limit().into_inner())
            .filter_map(|entry| {
                serde_json::from_str::<E>(&entry.event_data)
                    .ok()
                    .map(|e| (e, StreamPosition::new(entry.event_id)))
            })
            .collect();

        Ok(events)
    }
}

impl CheckpointStore for InMemoryEventStore {
    type Error = InMemoryCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let data = self
            .data
            .lock()
            .map_err(|e| InMemoryCheckpointError::LockFailed(e.to_string()))?;
        Ok(data.checkpoints.get(name).copied())
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let mut data = self
            .data
            .lock()
            .map_err(|e| InMemoryCheckpointError::LockFailed(e.to_string()))?;
        let _ = data.checkpoints.insert(name.to_string(), position);
        Ok(())
    }
}

impl ProjectorCoordinator for InMemoryEventStore {
    type Error = InMemoryCoordinationError;
    type Guard = InMemoryCoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        let data = self
            .data
            .lock()
            .map_err(|e| InMemoryCoordinationError::LockPoisoned {
                message: e.to_string(),
            })?;

        let mut guard =
            data.locks
                .write()
                .map_err(|e| InMemoryCoordinationError::LockPoisoned {
                    message: e.to_string(),
                })?;

        if guard.contains_key(subscription_name) {
            return Err(InMemoryCoordinationError::LeadershipNotAcquired {
                subscription_name: subscription_name.to_string(),
            });
        }

        let _ = guard.insert(subscription_name.to_string(), ());

        Ok(InMemoryCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&data.locks),
        })
    }
}

/// In-memory checkpoint store for tracking projection progress.
///
/// `InMemoryCheckpointStore` stores checkpoint positions in memory using a
/// thread-safe `Arc<RwLock<HashMap>>`. It is primarily useful for testing
/// and single-process deployments where persistence across restarts is not required.
///
/// For production deployments requiring durability, use a persistent
/// checkpoint store implementation.
///
/// # Example
///
/// ```no_run
/// use eventcore_memory::InMemoryCheckpointStore;
///
/// let checkpoint_store = InMemoryCheckpointStore::new();
/// // Implements CheckpointStore for tracking projection progress.
/// ```
#[derive(Debug, Clone, Default)]
pub struct InMemoryCheckpointStore {
    checkpoints: Arc<RwLock<HashMap<String, StreamPosition>>>,
}

impl InMemoryCheckpointStore {
    /// Create a new in-memory checkpoint store.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Error type for in-memory checkpoint store operations.
///
/// Since the in-memory store uses an `RwLock`, the only possible error
/// is a poisoned lock from a panic in another thread.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InMemoryCheckpointError {
    #[error("failed to acquire lock: {0}")]
    LockFailed(String),
}

/// Error type for in-memory coordinator operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InMemoryCoordinationError {
    /// Leadership is already held by another instance.
    #[error(
        "leadership not acquired for subscription '{subscription_name}': another instance holds the lock"
    )]
    LeadershipNotAcquired { subscription_name: String },
    /// Lock was poisoned by a panic in another thread.
    #[error("lock poisoned: {message}")]
    LockPoisoned { message: String },
}

/// Guard that releases leadership when dropped.
#[derive(Debug)]
pub struct InMemoryCoordinationGuard {
    subscription_name: String,
    locks: Arc<RwLock<HashMap<String, ()>>>,
}

impl Drop for InMemoryCoordinationGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.locks.write() {
            let _ = guard.remove(&self.subscription_name);
        } else {
            tracing::error!(
                subscription_name = %self.subscription_name,
                "failed to release coordination lock: RwLock poisoned"
            );
        }
    }
}

/// In-memory projector coordinator for single-process deployments.
///
/// `InMemoryProjectorCoordinator` provides coordination for projectors within a single
/// process using an in-memory lock table. This is suitable for testing and single-process
/// deployments where distributed coordination is not required.
///
/// For distributed deployments with multiple process instances, use a database-backed
/// coordinator implementation (e.g., PostgreSQL advisory locks).
#[derive(Debug, Clone, Default)]
pub struct InMemoryProjectorCoordinator {
    locks: Arc<RwLock<HashMap<String, ()>>>,
}

impl InMemoryProjectorCoordinator {
    /// Create a new in-memory projector coordinator.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectorCoordinator for InMemoryProjectorCoordinator {
    type Error = InMemoryCoordinationError;
    type Guard = InMemoryCoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        let mut guard =
            self.locks
                .write()
                .map_err(|e| InMemoryCoordinationError::LockPoisoned {
                    message: e.to_string(),
                })?;

        if guard.contains_key(subscription_name) {
            return Err(InMemoryCoordinationError::LeadershipNotAcquired {
                subscription_name: subscription_name.to_string(),
            });
        }

        let _ = guard.insert(subscription_name.to_string(), ());

        Ok(InMemoryCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&self.locks),
        })
    }
}

impl CheckpointStore for InMemoryCheckpointStore {
    type Error = InMemoryCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let guard = self
            .checkpoints
            .read()
            .map_err(|e| InMemoryCheckpointError::LockFailed(e.to_string()))?;
        Ok(guard.get(name).copied())
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let mut guard = self
            .checkpoints
            .write()
            .map_err(|e| InMemoryCheckpointError::LockFailed(e.to_string()))?;
        let _ = guard.insert(name.to_string(), position);
        Ok(())
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
