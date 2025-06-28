//! Type registry for dynamic event type resolution and deserialization.
//!
//! This module provides a registry system that maps event type names to Rust types,
//! enabling dynamic deserialization of events whose types are not known at compile time.
//! It handles unknown event types gracefully and supports schema evolution.

use crate::errors::EventStoreError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Error types specific to the type registry.
#[derive(Debug, thiserror::Error)]
pub enum TypeRegistryError {
    /// An event type is not registered in the registry.
    #[error("Unknown event type: {type_name}")]
    UnknownEventType {
        /// The unregistered type name
        type_name: String,
    },

    /// A type name is already registered with a different `TypeId`.
    #[error("Type name '{type_name}' is already registered with a different type")]
    TypeNameConflict {
        /// The conflicting type name
        type_name: String,
    },

    /// Deserialization failed for a registered type.
    #[error("Failed to deserialize event of type '{type_name}': {source}")]
    DeserializationFailed {
        /// The type name that failed to deserialize
        type_name: String,
        /// The underlying error
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A type was registered but the deserializer function is missing.
    #[error("Deserializer not found for type: {type_name}")]
    DeserializerNotFound {
        /// The type name without a deserializer
        type_name: String,
    },
}

/// Trait for registering and resolving event types dynamically.
///
/// This trait provides the core functionality for mapping type names to Rust types
/// and supporting dynamic deserialization of events.
#[async_trait]
pub trait TypeRegistry: Send + Sync {
    /// Registers an event type with the registry.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name
    ///
    /// # Returns
    ///
    /// A result indicating success or failure of registration.
    ///
    /// # Type Parameters
    ///
    /// * `E` - The event type to register. Must be deserializable.
    fn register_type<E>(&mut self, type_name: &str) -> Result<(), TypeRegistryError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + 'static;

    /// Checks if a type is registered in the registry.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name to check
    ///
    /// # Returns
    ///
    /// `true` if the type is registered, `false` otherwise.
    fn is_type_registered(&self, type_name: &str) -> bool;

    /// Gets all registered type names.
    ///
    /// # Returns
    ///
    /// A vector of all registered type names.
    fn registered_types(&self) -> Vec<String>;

    /// Dynamically deserializes an event using the registry.
    ///
    /// The deserialization method is determined by how the type was registered.
    /// Each registered type includes its own deserialization logic.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name of the event
    /// * `data` - The serialized event data (typically JSON)
    ///
    /// # Returns
    ///
    /// A boxed `Any` trait object containing the deserialized event,
    /// or an error if deserialization fails or the type is unknown.
    async fn deserialize_dynamic(
        &self,
        type_name: &str,
        data: &[u8],
    ) -> Result<Box<dyn Any + Send>, TypeRegistryError>;

    /// Handles unknown event types gracefully.
    ///
    /// This method is called when an event type is encountered that is not
    /// registered in the registry. The default behavior returns an error,
    /// but implementations can override this to provide custom handling.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The unknown type name
    /// * `data` - The serialized event data
    ///
    /// # Returns
    ///
    /// A result containing a representation of the unknown event or an error.
    async fn handle_unknown_type(
        &self,
        type_name: &str,
        data: &[u8],
    ) -> Result<UnknownEvent, TypeRegistryError> {
        // Default implementation returns an UnknownEvent
        Ok(UnknownEvent {
            type_name: type_name.to_string(),
            raw_data: data.to_vec(),
        })
    }
}

/// Represents an event whose type is not registered in the registry.
///
/// This allows the system to continue processing even when encountering
/// unknown event types, enabling forward compatibility and graceful degradation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnknownEvent {
    /// The type name of the unknown event
    pub type_name: String,
    /// The raw serialized data of the event
    pub raw_data: Vec<u8>,
}

/// Type-erased deserializer function for dynamic deserialization.
/// This function takes raw bytes and returns a deserialized event as a trait object.
type DeserializerFn = Arc<
    dyn Fn(
            Vec<u8>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Box<dyn Any + Send>, EventStoreError>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

/// Registry metadata for a registered type.
#[derive(Clone)]
struct TypeInfo {
    /// The Rust `TypeId` for type safety
    type_id: TypeId,
    /// The type-erased deserializer function
    deserializer: DeserializerFn,
}

impl std::fmt::Debug for TypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeInfo")
            .field("type_id", &self.type_id)
            .field("deserializer", &"<function>")
            .finish()
    }
}

/// In-memory implementation of the `TypeRegistry` trait.
///
/// This implementation stores type mappings in memory and provides
/// thread-safe access through read-write locks.
#[derive(Debug)]
pub struct InMemoryTypeRegistry {
    /// Map from type name to type information
    types: Arc<RwLock<HashMap<String, TypeInfo>>>,
}

impl InMemoryTypeRegistry {
    /// Creates a new empty type registry.
    ///
    /// # Returns
    ///
    /// A new `InMemoryTypeRegistry` instance.
    pub fn new() -> Self {
        Self {
            types: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a type registry with common event types pre-registered.
    ///
    /// This is a convenience method for applications that want to start
    /// with a set of commonly used event types already registered.
    ///
    /// # Returns
    ///
    /// A new `InMemoryTypeRegistry` with common types registered.
    pub fn with_common_types() -> Self {
        // Note: In a real application, you would register your common event types here
        // For now, we return an empty registry as we don't have concrete event types defined
        Self::new()
    }
}

impl Default for InMemoryTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TypeRegistry for InMemoryTypeRegistry {
    fn register_type<E>(&mut self, type_name: &str) -> Result<(), TypeRegistryError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();

        // Create the deserializer function for this type using JSON deserialization
        let deserializer: DeserializerFn = Arc::new(move |data| {
            Box::pin(async move {
                let event: E = serde_json::from_slice(&data)
                    .map_err(|e| EventStoreError::SerializationError(e.to_string()))?;
                Ok(Box::new(event) as Box<dyn Any + Send>)
            })
        });

        let type_info = TypeInfo {
            type_id,
            deserializer,
        };

        {
            let mut types = self.types.write().unwrap();

            // Check for conflicts
            if let Some(existing) = types.get(type_name) {
                if existing.type_id != type_id {
                    return Err(TypeRegistryError::TypeNameConflict {
                        type_name: type_name.to_string(),
                    });
                }
                // Same type, just update the deserializer
            }

            types.insert(type_name.to_string(), type_info);
        } // Drop the lock explicitly
        Ok(())
    }

    fn is_type_registered(&self, type_name: &str) -> bool {
        let types = self.types.read().unwrap();
        types.contains_key(type_name)
    }

    fn registered_types(&self) -> Vec<String> {
        let types = self.types.read().unwrap();
        types.keys().cloned().collect()
    }

    async fn deserialize_dynamic(
        &self,
        type_name: &str,
        data: &[u8],
    ) -> Result<Box<dyn Any + Send>, TypeRegistryError> {
        // Get the deserializer function without holding the lock across await
        let deserializer_fn = self
            .types
            .read()
            .unwrap()
            .get(type_name)
            .ok_or_else(|| TypeRegistryError::UnknownEventType {
                type_name: type_name.to_string(),
            })?
            .deserializer
            .clone();

        // Call the deserializer function with owned data
        let data = data.to_vec();
        deserializer_fn(data)
            .await
            .map_err(|e| TypeRegistryError::DeserializationFailed {
                type_name: type_name.to_string(),
                source: Box::new(e),
            })
    }
}

/// Builder for constructing type registries with multiple types.
///
/// This builder provides a fluent interface for registering multiple
/// event types and configuring registry behavior.
#[derive(Debug)]
pub struct TypeRegistryBuilder {
    registry: InMemoryTypeRegistry,
}

impl TypeRegistryBuilder {
    /// Creates a new type registry builder.
    pub fn new() -> Self {
        Self {
            registry: InMemoryTypeRegistry::new(),
        }
    }

    /// Registers an event type with the builder.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name
    ///
    /// # Returns
    ///
    /// The builder for method chaining.
    ///
    /// # Type Parameters
    ///
    /// * `E` - The event type to register
    pub fn register<E>(mut self, type_name: &str) -> Result<Self, TypeRegistryError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        self.registry.register_type::<E>(type_name)?;
        Ok(self)
    }

    /// Builds the type registry.
    ///
    /// # Returns
    ///
    /// The constructed `InMemoryTypeRegistry`.
    pub fn build(self) -> InMemoryTypeRegistry {
        self.registry
    }
}

impl Default for TypeRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Convert TypeRegistryError to EventStoreError for compatibility
impl From<TypeRegistryError> for EventStoreError {
    fn from(err: TypeRegistryError) -> Self {
        match err {
            TypeRegistryError::UnknownEventType { type_name } => {
                Self::SerializationError(format!("Unknown event type: {type_name}"))
            }
            TypeRegistryError::TypeNameConflict { type_name } => {
                Self::SerializationError(format!("Type name conflict for: {type_name}"))
            }
            TypeRegistryError::DeserializationFailed { type_name, source } => {
                Self::SerializationError(format!(
                    "Deserialization failed for {type_name}: {source}"
                ))
            }
            TypeRegistryError::DeserializerNotFound { type_name } => {
                Self::SerializationError(format!("Deserializer not found for: {type_name}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestEvent {
        pub id: u64,
        pub message: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct AnotherTestEvent {
        pub value: i32,
    }

    #[tokio::test]
    async fn test_register_and_check_type() {
        let mut registry = InMemoryTypeRegistry::new();

        assert!(!registry.is_type_registered("TestEvent"));

        registry
            .register_type::<TestEvent>("TestEvent")
            .expect("Registration should succeed");

        assert!(registry.is_type_registered("TestEvent"));

        let registered = registry.registered_types();
        assert_eq!(registered, vec!["TestEvent"]);
    }

    #[tokio::test]
    async fn test_type_name_conflict() {
        let mut registry = InMemoryTypeRegistry::new();

        // Register first type
        registry
            .register_type::<TestEvent>("ConflictEvent")
            .expect("First registration should succeed");

        // Try to register different type with same name
        let result = registry.register_type::<AnotherTestEvent>("ConflictEvent");

        assert!(matches!(
            result,
            Err(TypeRegistryError::TypeNameConflict { .. })
        ));
    }

    #[tokio::test]
    async fn test_same_type_re_registration() {
        let mut registry = InMemoryTypeRegistry::new();

        // Register type
        registry
            .register_type::<TestEvent>("TestEvent")
            .expect("First registration should succeed");

        // Re-register same type - should succeed
        registry
            .register_type::<TestEvent>("TestEvent")
            .expect("Re-registration of same type should succeed");
    }

    #[tokio::test]
    async fn test_dynamic_deserialization() {
        let mut registry = InMemoryTypeRegistry::new();

        // Register type
        registry
            .register_type::<TestEvent>("TestEvent")
            .expect("Registration should succeed");

        // Create test event
        let event = TestEvent {
            id: 42,
            message: "test message".to_string(),
        };

        // Serialize event using JSON directly
        let serialized = serde_json::to_vec(&event).expect("JSON serialization should succeed");

        // Deserialize dynamically
        let deserialized = registry
            .deserialize_dynamic("TestEvent", &serialized)
            .await
            .expect("Dynamic deserialization should succeed");

        // Downcast and verify
        let typed_event = deserialized
            .downcast::<TestEvent>()
            .expect("Downcast should succeed");

        assert_eq!(*typed_event, event);
    }

    #[tokio::test]
    async fn test_unknown_type_handling() {
        let registry = InMemoryTypeRegistry::new();

        // Try to deserialize unknown type
        let result = registry
            .deserialize_dynamic("UnknownType", b"test data")
            .await;

        assert!(matches!(
            result,
            Err(TypeRegistryError::UnknownEventType { .. })
        ));

        // Test handle_unknown_type method
        let unknown = registry
            .handle_unknown_type("UnknownType", b"test data")
            .await
            .expect("handle_unknown_type should succeed");

        assert_eq!(unknown.type_name, "UnknownType");
        assert_eq!(unknown.raw_data, b"test data");
    }

    #[tokio::test]
    async fn test_builder_pattern() {
        let registry = TypeRegistryBuilder::new()
            .register::<TestEvent>("TestEvent")
            .expect("Registration should succeed")
            .register::<AnotherTestEvent>("AnotherTestEvent")
            .expect("Registration should succeed")
            .build();

        assert!(registry.is_type_registered("TestEvent"));
        assert!(registry.is_type_registered("AnotherTestEvent"));

        let registered = registry.registered_types();
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&"TestEvent".to_string()));
        assert!(registered.contains(&"AnotherTestEvent".to_string()));
    }

    #[tokio::test]
    async fn test_unknown_event_serialization() {
        let unknown = UnknownEvent {
            type_name: "TestType".to_string(),
            raw_data: vec![1, 2, 3, 4],
        };

        // Test serialization round-trip
        let serialized = serde_json::to_vec(&unknown).expect("Serialization should succeed");

        let deserialized: UnknownEvent =
            serde_json::from_slice(&serialized).expect("Deserialization should succeed");

        assert_eq!(unknown, deserialized);
    }

    #[tokio::test]
    async fn test_error_conversions() {
        let type_registry_error = TypeRegistryError::UnknownEventType {
            type_name: "TestType".to_string(),
        };

        let event_store_error: EventStoreError = type_registry_error.into();

        assert!(matches!(
            event_store_error,
            EventStoreError::SerializationError(_)
        ));
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct PropertyTestEvent {
        pub data: String,
    }

    proptest! {
        #[test]
        fn prop_type_registration_is_idempotent(
            type_name in "[a-zA-Z][a-zA-Z0-9_]{3,30}"
        ) {
            tokio_test::block_on(async {
                let mut registry = InMemoryTypeRegistry::new();

                // Register the same type multiple times
                registry.register_type::<PropertyTestEvent>(&type_name)
                    .expect("First registration should succeed");

                registry.register_type::<PropertyTestEvent>(&type_name)
                    .expect("Second registration should succeed");

                registry.register_type::<PropertyTestEvent>(&type_name)
                    .expect("Third registration should succeed");

                // Should still be registered only once
                assert!(registry.is_type_registered(&type_name));
                assert_eq!(registry.registered_types().len(), 1);
            });
        }

        #[test]
        fn prop_unknown_events_preserve_data(
            type_name in "[a-zA-Z][a-zA-Z0-9_]{3,30}",
            data in prop::collection::vec(any::<u8>(), 0..1000)
        ) {
            tokio_test::block_on(async {
                let registry = InMemoryTypeRegistry::new();

                let unknown = registry.handle_unknown_type(&type_name, &data)
                    .await
                    .expect("handle_unknown_type should succeed");

                assert_eq!(unknown.type_name, type_name);
                assert_eq!(unknown.raw_data, data);
            });
        }

        #[test]
        fn prop_builder_preserves_all_registrations(
            type_names in prop::collection::vec("[a-zA-Z][a-zA-Z0-9_]{3,30}", 1..10)
        ) {
            tokio_test::block_on(async {
                let mut builder = TypeRegistryBuilder::new();

                // Register all types
                for type_name in &type_names {
                    builder = builder.register::<PropertyTestEvent>(type_name)
                        .expect("Registration should succeed");
                }

                let registry = builder.build();

                // All types should be registered
                for type_name in &type_names {
                    assert!(registry.is_type_registered(type_name));
                }

                // Number of registered types should match (accounting for duplicates)
                let unique_names: std::collections::HashSet<_> = type_names.iter().collect();
                assert_eq!(registry.registered_types().len(), unique_names.len());
            });
        }
    }
}
