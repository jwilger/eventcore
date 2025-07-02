//! Checkpoint storage for CQRS projections.
//!
//! This module provides abstractions and implementations for persisting
//! projection checkpoints, enabling projections to resume from where they
//! left off after restarts.

use super::CqrsError;
use crate::projection::ProjectionCheckpoint;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Trait for storing and retrieving projection checkpoints.
///
/// Checkpoints allow projections to resume processing from their last
/// known position, ensuring exactly-once processing semantics.
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    /// The error type for checkpoint operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Load a checkpoint for a projection.
    ///
    /// Returns `None` if no checkpoint exists for the given projection.
    async fn load(
        &self,
        projection_name: &str,
    ) -> Result<Option<ProjectionCheckpoint>, Self::Error>;

    /// Save a checkpoint for a projection.
    ///
    /// If a checkpoint already exists, it will be replaced.
    async fn save(
        &self,
        projection_name: &str,
        checkpoint: ProjectionCheckpoint,
    ) -> Result<(), Self::Error>;

    /// Delete a checkpoint for a projection.
    ///
    /// Used when rebuilding projections from scratch.
    async fn delete(&self, projection_name: &str) -> Result<(), Self::Error>;

    /// List all stored checkpoints.
    ///
    /// Returns a map of projection names to their checkpoints.
    async fn list_all(&self) -> Result<HashMap<String, ProjectionCheckpoint>, Self::Error>;

    /// Check if a checkpoint exists for a projection.
    async fn exists(&self, projection_name: &str) -> Result<bool, Self::Error> {
        Ok(self.load(projection_name).await?.is_some())
    }
}

/// In-memory implementation of CheckpointStore for testing.
pub struct InMemoryCheckpointStore {
    checkpoints: Arc<RwLock<HashMap<String, ProjectionCheckpoint>>>,
}

impl InMemoryCheckpointStore {
    /// Creates a new in-memory checkpoint store.
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryCheckpointStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CheckpointStore for InMemoryCheckpointStore {
    type Error = CqrsError;

    async fn load(
        &self,
        projection_name: &str,
    ) -> Result<Option<ProjectionCheckpoint>, Self::Error> {
        Ok(self
            .checkpoints
            .read()
            .map_err(|e| CqrsError::checkpoint(format!("Lock poisoned: {e}")))?
            .get(projection_name)
            .cloned())
    }

    async fn save(
        &self,
        projection_name: &str,
        checkpoint: ProjectionCheckpoint,
    ) -> Result<(), Self::Error> {
        self.checkpoints
            .write()
            .map_err(|e| CqrsError::checkpoint(format!("Lock poisoned: {e}")))?
            .insert(projection_name.to_string(), checkpoint);
        Ok(())
    }

    async fn delete(&self, projection_name: &str) -> Result<(), Self::Error> {
        self.checkpoints
            .write()
            .map_err(|e| CqrsError::checkpoint(format!("Lock poisoned: {e}")))?
            .remove(projection_name);
        Ok(())
    }

    async fn list_all(&self) -> Result<HashMap<String, ProjectionCheckpoint>, Self::Error> {
        Ok(self
            .checkpoints
            .read()
            .map_err(|e| CqrsError::checkpoint(format!("Lock poisoned: {e}")))?
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EventId, StreamId, Timestamp};

    #[tokio::test]
    async fn in_memory_checkpoint_store_operations() {
        let store = InMemoryCheckpointStore::new();

        // Test save and load
        let checkpoint = ProjectionCheckpoint {
            last_event_id: Some(EventId::new()),
            checkpoint_time: Timestamp::now(),
            stream_positions: HashMap::new(),
        };

        store
            .save("test_projection", checkpoint.clone())
            .await
            .unwrap();

        let loaded = store.load("test_projection").await.unwrap();
        assert_eq!(loaded, Some(checkpoint));

        // Test exists
        assert!(store.exists("test_projection").await.unwrap());
        assert!(!store.exists("nonexistent").await.unwrap());

        // Test delete
        store.delete("test_projection").await.unwrap();
        assert!(store.load("test_projection").await.unwrap().is_none());

        // Test list_all
        let checkpoint1 = ProjectionCheckpoint::initial();
        let checkpoint2 = ProjectionCheckpoint::from_event_id(EventId::new());

        store.save("proj1", checkpoint1.clone()).await.unwrap();
        store.save("proj2", checkpoint2.clone()).await.unwrap();

        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("proj1"), Some(&checkpoint1));
        assert_eq!(all.get("proj2"), Some(&checkpoint2));
    }

    #[tokio::test]
    async fn checkpoint_with_stream_positions() {
        let store = InMemoryCheckpointStore::new();

        let stream1 = StreamId::try_new("stream1").unwrap();
        let stream2 = StreamId::try_new("stream2").unwrap();
        let event1 = EventId::new();
        let event2 = EventId::new();

        let checkpoint = ProjectionCheckpoint::initial()
            .with_stream_position(stream1.clone(), event1)
            .with_stream_position(stream2.clone(), event2);

        store.save("test_proj", checkpoint.clone()).await.unwrap();

        let loaded = store.load("test_proj").await.unwrap().unwrap();
        assert_eq!(loaded.get_stream_position(&stream1), Some(&event1));
        assert_eq!(loaded.get_stream_position(&stream2), Some(&event2));
    }
}
