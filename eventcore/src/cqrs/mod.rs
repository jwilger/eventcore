//! CQRS (Command Query Responsibility Segregation) support for EventCore.
//!
//! This module provides abstractions and implementations for building
//! CQRS systems with EventCore, including:
//!
//! - Read model storage abstractions
//! - Checkpoint persistence for projections
//! - Query API for read models
//! - Projection rebuild and migration support
//! - Monitoring and health checks
//!
//! # Example
//!
//! ```rust,ignore
//! use eventcore::cqrs::{CqrsProjection, ReadModelStore, PostgresReadModelStore};
//!
//! // Define your read model
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! struct UserProfile {
//!     user_id: String,
//!     name: String,
//!     email: String,
//!     last_login: Timestamp,
//! }
//!
//! // Create a CQRS projection
//! struct UserProfileProjection;
//!
//! #[async_trait]
//! impl CqrsProjection for UserProfileProjection {
//!     type ReadModel = UserProfile;
//!     type Query = UserQuery;
//!     
//!     // ... implement required methods
//! }
//!
//! // Use with PostgreSQL storage
//! let read_store = PostgresReadModelStore::new(pool, "user_profiles").await?;
//! let projection = UserProfileProjection::new();
//! ```

mod checkpoint_store;
mod projection;
mod query;
mod read_model_store;
mod rebuild;

pub use checkpoint_store::{CheckpointStore, InMemoryCheckpointStore};
pub use projection::{CqrsProjection, CqrsProjectionRunner};
pub use query::{
    ConsistencyLevel, Direction, Filter, FilterOp, Ordering, Query, QueryBuilder, Value,
};
pub use read_model_store::{
    InMemoryReadModelStore, ReadModelStore, VersionedReadModel, VersionedReadModelStore,
};
pub use rebuild::{RebuildCoordinator, RebuildProgress, RebuildStrategy};

/// Result type for CQRS operations
pub type CqrsResult<T> = Result<T, CqrsError>;

/// Errors that can occur in CQRS operations
#[derive(Debug, thiserror::Error)]
pub enum CqrsError {
    /// Storage operation failed
    #[error("Storage error: {0}")]
    Storage(String),

    /// Query parsing or execution failed
    #[error("Query error: {0}")]
    Query(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Projection error
    #[error("Projection error: {0}")]
    Projection(#[from] crate::errors::ProjectionError),

    /// Checkpoint error
    #[error("Checkpoint error: {0}")]
    Checkpoint(String),

    /// Rebuild operation failed
    #[error("Rebuild error: {0}")]
    Rebuild(String),

    /// Consistency violation
    #[error("Consistency error: {0}")]
    Consistency(String),

    /// Custom error
    #[error("{0}")]
    Custom(String),
}

impl CqrsError {
    /// Creates a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Creates a query error
    pub fn query(msg: impl Into<String>) -> Self {
        Self::Query(msg.into())
    }

    /// Creates a serialization error
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// Creates a checkpoint error
    pub fn checkpoint(msg: impl Into<String>) -> Self {
        Self::Checkpoint(msg.into())
    }

    /// Creates a rebuild error
    pub fn rebuild(msg: impl Into<String>) -> Self {
        Self::Rebuild(msg.into())
    }

    /// Creates a consistency error
    pub fn consistency(msg: impl Into<String>) -> Self {
        Self::Consistency(msg.into())
    }

    /// Creates a custom error
    pub fn custom(msg: impl Into<String>) -> Self {
        Self::Custom(msg.into())
    }
}
