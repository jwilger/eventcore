//! Event store adapter infrastructure for backend-agnostic configuration and lifecycle management.
//!
//! This module provides the interfaces for implementing event store adapters,
//! allowing different backends (PostgreSQL, EventStoreDB, etc.) to integrate
//! with the EventCore library in a uniform way.

#![allow(async_fn_in_trait)]

use async_trait::async_trait;
use std::sync::Arc;

use crate::errors::EventStoreError;
use crate::event_store::EventStore;

/// Configuration trait that all event store adapters must implement.
///
/// This trait allows backend-specific configuration while maintaining
/// a uniform interface for initialization.
pub trait AdapterConfig: Send + Sync + 'static {
    /// The type of event store this configuration produces.
    type Store: EventStore;

    /// Validates the configuration.
    ///
    /// This should check that all required settings are present and valid,
    /// but should not attempt to connect to the backend.
    fn validate(&self) -> Result<(), EventStoreError>;

    /// Creates a new event store instance from this configuration.
    ///
    /// This method should establish connections and perform any necessary
    /// initialization for the backend.
    async fn build(self) -> Result<Self::Store, EventStoreError>;
}

/// Trait for mapping backend-specific errors to `EventStoreError`.
///
/// Each adapter should implement this to provide consistent error handling
/// across different backends.
pub trait ErrorMapper {
    /// The backend-specific error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Maps a backend-specific error to an `EventStoreError`.
    fn map_error(error: Self::Error) -> EventStoreError;
}

/// Manages the lifecycle of an event store adapter.
///
/// This trait provides hooks for initialization, shutdown, and health checking
/// of event store backends.
#[async_trait]
pub trait AdapterLifecycle: Send + Sync {
    /// Initializes the adapter.
    ///
    /// This might include creating database schemas, establishing connections,
    /// or other one-time setup tasks.
    async fn initialize(&self) -> Result<(), EventStoreError>;

    /// Performs a health check on the adapter.
    ///
    /// Returns Ok(()) if the backend is healthy and ready to accept requests.
    async fn health_check(&self) -> Result<(), EventStoreError>;

    /// Gracefully shuts down the adapter.
    ///
    /// This should close connections and clean up any resources.
    async fn shutdown(&self) -> Result<(), EventStoreError>;
}

/// A wrapper that combines an event store with lifecycle management.
pub struct ManagedEventStore<S: EventStore> {
    store: Arc<S>,
    lifecycle: Arc<dyn AdapterLifecycle>,
}

impl<S: EventStore> ManagedEventStore<S> {
    /// Creates a new managed event store.
    pub fn new(store: S, lifecycle: impl AdapterLifecycle + 'static) -> Self {
        Self {
            store: Arc::new(store),
            lifecycle: Arc::new(lifecycle),
        }
    }

    /// Gets a reference to the underlying event store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Initializes the managed store.
    pub async fn initialize(&self) -> Result<(), EventStoreError> {
        self.lifecycle.initialize().await
    }

    /// Performs a health check.
    pub async fn health_check(&self) -> Result<(), EventStoreError> {
        self.lifecycle.health_check().await
    }

    /// Shuts down the managed store.
    pub async fn shutdown(&self) -> Result<(), EventStoreError> {
        self.lifecycle.shutdown().await
    }
}

/// Feature flags for optional event store backends.
///
/// These can be used with Cargo features to conditionally compile
/// support for different backends.
#[derive(Debug, Clone, Copy)]
pub struct Features {
    /// Whether `PostgreSQL` support is enabled.
    pub postgres: bool,
    /// Whether `EventStoreDB` support is enabled.
    pub eventstoredb: bool,
    /// Whether in-memory support is enabled (always true for testing).
    pub memory: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            postgres: false,     // Will be enabled by feature flag in future
            eventstoredb: false, // Will be enabled by feature flag in future
            memory: true,        // Always available for testing
        }
    }
}

impl Features {
    /// Checks if any backend is enabled.
    pub const fn any_enabled(&self) -> bool {
        self.postgres || self.eventstoredb || self.memory
    }

    /// Gets a list of enabled backend names.
    pub fn enabled_backends(&self) -> Vec<&'static str> {
        let mut backends = Vec::new();
        if self.postgres {
            backends.push("postgres");
        }
        if self.eventstoredb {
            backends.push("eventstoredb");
        }
        if self.memory {
            backends.push("memory");
        }
        backends
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::{ReadOptions, StreamData, StreamEvents};
    use crate::types::{EventVersion, StreamId};
    use std::collections::HashMap;

    // Mock event store for testing
    #[derive(Clone)]
    struct MockEventStore;

    #[async_trait]
    impl EventStore for MockEventStore {
        type Event = String;

        async fn read_streams(
            &self,
            _stream_ids: &[StreamId],
            _options: &ReadOptions,
        ) -> Result<StreamData<Self::Event>, EventStoreError> {
            Ok(StreamData::new(Vec::new(), HashMap::new()))
        }

        async fn write_events_multi(
            &self,
            _stream_events: Vec<StreamEvents<Self::Event>>,
        ) -> Result<HashMap<StreamId, EventVersion>, EventStoreError> {
            Ok(HashMap::new())
        }

        async fn stream_exists(&self, _stream_id: &StreamId) -> Result<bool, EventStoreError> {
            Ok(false)
        }

        async fn get_stream_version(
            &self,
            _stream_id: &StreamId,
        ) -> Result<Option<EventVersion>, EventStoreError> {
            Ok(None)
        }
    }

    // Mock configuration for testing
    struct MockConfig {
        should_fail_validation: bool,
        should_fail_build: bool,
    }

    impl AdapterConfig for MockConfig {
        type Store = MockEventStore;

        fn validate(&self) -> Result<(), EventStoreError> {
            if self.should_fail_validation {
                Err(EventStoreError::Configuration(
                    "Mock validation error".to_string(),
                ))
            } else {
                Ok(())
            }
        }

        async fn build(self) -> Result<Self::Store, EventStoreError> {
            if self.should_fail_build {
                Err(EventStoreError::ConnectionFailed(
                    "Mock build error".to_string(),
                ))
            } else {
                Ok(MockEventStore)
            }
        }
    }

    // Mock error mapper
    struct MockErrorMapper;

    impl ErrorMapper for MockErrorMapper {
        type Error = std::io::Error;

        fn map_error(error: Self::Error) -> EventStoreError {
            EventStoreError::Io(error)
        }
    }

    // Mock lifecycle manager
    struct MockLifecycle {
        initialized: std::sync::Arc<std::sync::Mutex<bool>>,
        healthy: bool,
        shutdown: std::sync::Arc<std::sync::Mutex<bool>>,
    }

    impl MockLifecycle {
        fn new(healthy: bool) -> Self {
            Self {
                initialized: std::sync::Arc::new(std::sync::Mutex::new(false)),
                healthy,
                shutdown: std::sync::Arc::new(std::sync::Mutex::new(false)),
            }
        }

        fn is_initialized(&self) -> bool {
            *self.initialized.lock().unwrap()
        }

        fn is_shutdown(&self) -> bool {
            *self.shutdown.lock().unwrap()
        }
    }

    #[async_trait]
    impl AdapterLifecycle for MockLifecycle {
        async fn initialize(&self) -> Result<(), EventStoreError> {
            *self.initialized.lock().unwrap() = true;
            Ok(())
        }

        async fn health_check(&self) -> Result<(), EventStoreError> {
            if self.healthy {
                Ok(())
            } else {
                Err(EventStoreError::ConnectionFailed(
                    "Mock health check failed".to_string(),
                ))
            }
        }

        async fn shutdown(&self) -> Result<(), EventStoreError> {
            *self.shutdown.lock().unwrap() = true;
            Ok(())
        }
    }

    #[test]
    fn test_features_default() {
        let features = Features::default();
        assert!(features.memory);
        assert!(features.any_enabled());
    }

    #[test]
    fn test_features_enabled_backends() {
        let features = Features {
            postgres: true,
            eventstoredb: false,
            memory: true,
        };
        let backends = features.enabled_backends();
        assert_eq!(backends, vec!["postgres", "memory"]);
    }

    #[test]
    fn test_features_any_enabled() {
        let features = Features {
            postgres: false,
            eventstoredb: false,
            memory: false,
        };
        assert!(!features.any_enabled());

        let features = Features {
            postgres: false,
            eventstoredb: true,
            memory: false,
        };
        assert!(features.any_enabled());
    }

    #[test]
    fn test_features_all_backends() {
        let features = Features {
            postgres: true,
            eventstoredb: true,
            memory: true,
        };
        let backends = features.enabled_backends();
        assert_eq!(backends, vec!["postgres", "eventstoredb", "memory"]);
    }

    #[tokio::test]
    async fn test_adapter_config_validate_success() {
        let config = MockConfig {
            should_fail_validation: false,
            should_fail_build: false,
        };
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_adapter_config_validate_failure() {
        let config = MockConfig {
            should_fail_validation: true,
            should_fail_build: false,
        };
        assert!(matches!(
            config.validate(),
            Err(EventStoreError::Configuration(_))
        ));
    }

    #[tokio::test]
    async fn test_adapter_config_build_success() {
        let config = MockConfig {
            should_fail_validation: false,
            should_fail_build: false,
        };
        assert!(config.build().await.is_ok());
    }

    #[tokio::test]
    async fn test_adapter_config_build_failure() {
        let config = MockConfig {
            should_fail_validation: false,
            should_fail_build: true,
        };
        assert!(matches!(
            config.build().await,
            Err(EventStoreError::ConnectionFailed(_))
        ));
    }

    #[test]
    fn test_error_mapper() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "test error");
        let mapped = MockErrorMapper::map_error(io_error);
        assert!(matches!(mapped, EventStoreError::Io(_)));
    }

    #[tokio::test]
    async fn test_managed_event_store_lifecycle() {
        let store = MockEventStore;
        let lifecycle = MockLifecycle::new(true);
        let managed = ManagedEventStore::new(store, lifecycle);

        // Test initialization
        assert!(managed.initialize().await.is_ok());

        // Test health check
        assert!(managed.health_check().await.is_ok());

        // Test shutdown
        assert!(managed.shutdown().await.is_ok());

        // Verify store access
        let _ = managed.store();
    }

    #[tokio::test]
    async fn test_managed_event_store_unhealthy() {
        let store = MockEventStore;
        let lifecycle = MockLifecycle::new(false);
        let managed = ManagedEventStore::new(store, lifecycle);

        // Initialize should still work
        assert!(managed.initialize().await.is_ok());

        // Health check should fail
        assert!(matches!(
            managed.health_check().await,
            Err(EventStoreError::ConnectionFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_lifecycle_state_tracking() {
        let lifecycle = MockLifecycle::new(true);

        assert!(!lifecycle.is_initialized());
        assert!(!lifecycle.is_shutdown());

        lifecycle.initialize().await.unwrap();
        assert!(lifecycle.is_initialized());
        assert!(!lifecycle.is_shutdown());

        lifecycle.shutdown().await.unwrap();
        assert!(lifecycle.is_initialized());
        assert!(lifecycle.is_shutdown());
    }
}
