//! Read model storage abstractions for CQRS.
//!
//! This module provides traits and implementations for storing and querying
//! read models in a CQRS system.

use super::{CqrsError, Query};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, RwLock},
};

/// Trait for storing and retrieving read models.
///
/// Implementations of this trait provide persistence for CQRS read models,
/// allowing projections to store their state durably.
#[async_trait]
pub trait ReadModelStore: Send + Sync {
    /// The type of read model this store handles
    type Model: Send + Sync;

    /// The type of queries this store can execute
    type Query: Send + Sync;

    /// The error type for store operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Store or update a read model.
    ///
    /// If a model with the given ID already exists, it will be replaced.
    async fn upsert(&self, id: &str, model: Self::Model) -> Result<(), Self::Error>;

    /// Retrieve a read model by ID.
    ///
    /// Returns `None` if no model exists with the given ID.
    async fn get(&self, id: &str) -> Result<Option<Self::Model>, Self::Error>;

    /// Query read models.
    ///
    /// Execute a query and return all matching read models.
    async fn query(&self, query: Self::Query) -> Result<Vec<Self::Model>, Self::Error>;

    /// Delete a read model.
    ///
    /// Returns success even if the model doesn't exist.
    async fn delete(&self, id: &str) -> Result<(), Self::Error>;

    /// Perform bulk upsert operations for efficiency.
    ///
    /// Default implementation calls upsert for each model individually.
    async fn bulk_upsert(&self, models: Vec<(String, Self::Model)>) -> Result<(), Self::Error> {
        for (id, model) in models {
            self.upsert(&id, model).await?;
        }
        Ok(())
    }

    /// Delete all read models.
    ///
    /// Useful for rebuilding projections from scratch.
    async fn clear(&self) -> Result<(), Self::Error>;

    /// Count the total number of read models.
    async fn count(&self) -> Result<usize, Self::Error>;

    /// Check if a read model exists.
    async fn exists(&self, id: &str) -> Result<bool, Self::Error> {
        Ok(self.get(id).await?.is_some())
    }
}

/// In-memory implementation of ReadModelStore for testing.
pub struct InMemoryReadModelStore<M, Q> {
    models: Arc<RwLock<HashMap<String, M>>>,
    _phantom: PhantomData<Q>,
}

impl<M, Q> InMemoryReadModelStore<M, Q>
where
    M: Clone + Send + Sync,
    Q: Send + Sync,
{
    /// Creates a new in-memory read model store.
    pub fn new() -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
        }
    }
}

impl<M, Q> Default for InMemoryReadModelStore<M, Q>
where
    M: Clone + Send + Sync,
    Q: Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<M, Q> ReadModelStore for InMemoryReadModelStore<M, Q>
where
    M: Clone + Send + Sync + 'static,
    Q: Query<Model = M> + Send + Sync + 'static,
{
    type Model = M;
    type Query = Q;
    type Error = CqrsError;

    async fn upsert(&self, id: &str, model: Self::Model) -> Result<(), Self::Error> {
        self.models
            .write()
            .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?
            .insert(id.to_string(), model);
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Self::Model>, Self::Error> {
        Ok(self
            .models
            .read()
            .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?
            .get(id)
            .cloned())
    }

    async fn query(&self, query: Self::Query) -> Result<Vec<Self::Model>, Self::Error> {
        let results = {
            let models = self
                .models
                .read()
                .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?;

            models
                .values()
                .filter(|model| query.matches(model))
                .cloned()
                .collect()
        };

        Ok(query.apply_ordering_and_limits(results))
    }

    async fn delete(&self, id: &str) -> Result<(), Self::Error> {
        self.models
            .write()
            .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?
            .remove(id);
        Ok(())
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        self.models
            .write()
            .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?
            .clear();
        Ok(())
    }

    async fn count(&self) -> Result<usize, Self::Error> {
        Ok(self
            .models
            .read()
            .map_err(|e| CqrsError::storage(format!("Lock poisoned: {e}")))?
            .len())
    }
}

/// Versioned read model for supporting migrations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedReadModel<M> {
    /// The version of this read model
    pub version: u32,
    /// The actual model data
    pub model: M,
}

/// Extension trait for versioned read model stores.
#[async_trait]
pub trait VersionedReadModelStore: ReadModelStore {
    /// Migrate read models from one version to another.
    async fn migrate_version(
        &self,
        from_version: u32,
        to_version: u32,
        migration_fn: Box<dyn Fn(Self::Model) -> Self::Model + Send + Sync>,
    ) -> Result<(), Self::Error>;

    /// Get all models of a specific version.
    async fn get_by_version(&self, version: u32) -> Result<Vec<Self::Model>, Self::Error>;

    /// Delete all models of a specific version.
    async fn delete_version(&self, version: u32) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cqrs::QueryBuilder;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestModel {
        id: String,
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn in_memory_store_basic_operations() {
        let store: InMemoryReadModelStore<TestModel, QueryBuilder<TestModel>> =
            InMemoryReadModelStore::new();

        let model = TestModel {
            id: "test1".to_string(),
            name: "Test Model".to_string(),
            value: 42,
        };

        // Test upsert
        store.upsert("test1", model.clone()).await.unwrap();

        // Test get
        let retrieved = store.get("test1").await.unwrap();
        assert_eq!(retrieved, Some(model.clone()));

        // Test exists
        assert!(store.exists("test1").await.unwrap());
        assert!(!store.exists("nonexistent").await.unwrap());

        // Test count
        assert_eq!(store.count().await.unwrap(), 1);

        // Test delete
        store.delete("test1").await.unwrap();
        assert!(store.get("test1").await.unwrap().is_none());

        // Test clear
        store.upsert("test1", model.clone()).await.unwrap();
        store.upsert("test2", model).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
        store.clear().await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn in_memory_store_bulk_operations() {
        let store: InMemoryReadModelStore<TestModel, QueryBuilder<TestModel>> =
            InMemoryReadModelStore::new();

        let models = vec![
            (
                "test1".to_string(),
                TestModel {
                    id: "test1".to_string(),
                    name: "Model 1".to_string(),
                    value: 10,
                },
            ),
            (
                "test2".to_string(),
                TestModel {
                    id: "test2".to_string(),
                    name: "Model 2".to_string(),
                    value: 20,
                },
            ),
        ];

        store.bulk_upsert(models).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        let retrieved_model = store.get("test1").await.unwrap().unwrap();
        assert_eq!(retrieved_model.name, "Model 1");
    }
}
