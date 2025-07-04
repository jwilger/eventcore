//! MessagePack serialization implementation for events.

use super::{EventSerializer, SchemaEvolution, SerializedEventEnvelope};
use crate::errors::EventStoreError;
use crate::event::{Event, StoredEvent};
use async_trait::async_trait;
use rmp_serde;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// MessagePack-based event serializer with schema versioning support.
#[derive(Clone)]
pub struct MessagePackEventSerializer {
    /// Map of type names to their current schema versions
    schema_versions: Arc<RwLock<HashMap<String, u32>>>,
    /// Optional schema evolution handler
    evolution: Option<Arc<dyn SchemaEvolution>>,
}

impl MessagePackEventSerializer {
    /// Creates a new MessagePack event serializer.
    pub fn new() -> Self {
        Self {
            schema_versions: Arc::new(RwLock::new(HashMap::new())),
            evolution: None,
        }
    }

    /// Creates a new MessagePack event serializer with schema evolution support.
    pub fn with_evolution(evolution: Arc<dyn SchemaEvolution>) -> Self {
        Self {
            schema_versions: Arc::new(RwLock::new(HashMap::new())),
            evolution: Some(evolution),
        }
    }

    /// Registers a schema version for a type.
    pub async fn register_schema_version(&self, type_name: String, version: u32) {
        let mut versions = self.schema_versions.write().await;
        versions.insert(type_name, version);
    }
}

impl Default for MessagePackEventSerializer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventSerializer for MessagePackEventSerializer {
    async fn serialize<E>(&self, event: &E, _type_name: &str) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync,
    {
        rmp_serde::to_vec(event).map_err(|e| {
            EventStoreError::SerializationFailed(format!(
                "Failed to serialize event to MessagePack: {e}"
            ))
        })
    }

    async fn deserialize<E>(&self, data: &[u8], _type_name: &str) -> Result<E, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync,
    {
        rmp_serde::from_slice(data).map_err(|e| {
            EventStoreError::DeserializationFailed(format!(
                "Failed to deserialize event from MessagePack: {e}"
            ))
        })
    }

    async fn serialize_stored_event<E>(
        &self,
        event: &StoredEvent<E>,
        type_name: &str,
    ) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync + PartialEq + Eq,
    {
        // First serialize the payload
        let payload_data = self.serialize(&event.event.payload, type_name).await?;

        let envelope = SerializedEventEnvelope {
            schema_version: self.get_schema_version(type_name),
            type_name: type_name.to_string(),
            payload_data,
            event_id: event.event.id,
            stream_id: event.event.stream_id.clone(),
            event_metadata: event.event.metadata.clone(),
            created_at: event.event.created_at,
            version: event.version,
            stored_at: event.stored_at,
        };

        rmp_serde::to_vec(&envelope).map_err(|e| {
            EventStoreError::SerializationFailed(format!(
                "Failed to serialize event envelope to MessagePack: {e}"
            ))
        })
    }

    async fn deserialize_stored_event<E>(
        &self,
        data: &[u8],
        type_name: &str,
    ) -> Result<StoredEvent<E>, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + PartialEq + Eq,
    {
        let envelope: SerializedEventEnvelope = rmp_serde::from_slice(data).map_err(|e| {
            EventStoreError::DeserializationFailed(format!(
                "Failed to deserialize event envelope from MessagePack: {e}"
            ))
        })?;

        // Check if we need to migrate the event data
        let current_version = self.get_schema_version(&envelope.type_name);
        let payload_data = if envelope.schema_version < current_version {
            // Migrate if we have an evolution handler
            if let Some(evolution) = &self.evolution {
                evolution
                    .migrate(
                        &envelope.payload_data,
                        &envelope.type_name,
                        envelope.schema_version,
                        current_version,
                    )
                    .await?
            } else {
                // No migration available, use as-is
                envelope.payload_data
            }
        } else {
            envelope.payload_data
        };

        // Deserialize the payload
        let payload: E = self.deserialize(&payload_data, type_name).await?;

        // Reconstruct the event
        let event = Event {
            id: envelope.event_id,
            stream_id: envelope.stream_id,
            payload,
            metadata: envelope.event_metadata,
            created_at: envelope.created_at,
        };

        Ok(StoredEvent {
            event,
            version: envelope.version,
            stored_at: envelope.stored_at,
        })
    }

    fn get_schema_version(&self, type_name: &str) -> u32 {
        // Use try_read() to avoid blocking in async context
        self.schema_versions
            .try_read()
            .map_or(1, |versions| versions.get(type_name).copied().unwrap_or(1))
    }

    fn supports_schema_version(&self, type_name: &str, version: u32) -> bool {
        let current_version = self.get_schema_version(type_name);
        version <= current_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::metadata::EventMetadata;
    use crate::types::{EventVersion, StreamId};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestEvent {
        id: String,
        value: i32,
    }

    #[tokio::test]
    #[allow(clippy::similar_names)]
    async fn test_serialize_deserialize_event() {
        let serializer = MessagePackEventSerializer::new();
        let event = TestEvent {
            id: "test-123".to_string(),
            value: 42,
        };

        let serialized = serializer
            .serialize(&event, "TestEvent")
            .await
            .expect("Failed to serialize");
        let deserialized: TestEvent = serializer
            .deserialize(&serialized, "TestEvent")
            .await
            .expect("Failed to deserialize");

        assert_eq!(event, deserialized);
    }

    #[tokio::test]
    async fn test_schema_version_registration() {
        let serializer = MessagePackEventSerializer::new();

        // Default version should be 1
        assert_eq!(serializer.get_schema_version("TestEvent"), 1);

        // Register a new version
        serializer
            .register_schema_version("TestEvent".to_string(), 3)
            .await;
        assert_eq!(serializer.get_schema_version("TestEvent"), 3);
    }

    #[tokio::test]
    async fn test_supports_schema_version() {
        let serializer = MessagePackEventSerializer::new();
        serializer
            .register_schema_version("TestEvent".to_string(), 3)
            .await;

        assert!(serializer.supports_schema_version("TestEvent", 1));
        assert!(serializer.supports_schema_version("TestEvent", 2));
        assert!(serializer.supports_schema_version("TestEvent", 3));
        assert!(!serializer.supports_schema_version("TestEvent", 4));
    }

    #[tokio::test]
    #[allow(clippy::similar_names)]
    async fn test_stored_event_serialization() {
        let serializer = MessagePackEventSerializer::new();
        let test_payload = TestEvent {
            id: "test-123".to_string(),
            value: 42,
        };

        let stream_id = StreamId::try_new("test-stream").expect("Invalid stream ID");
        let event = Event::new(
            stream_id.clone(),
            test_payload.clone(),
            EventMetadata::default(),
        );
        let stored_event =
            StoredEvent::new(event, EventVersion::try_new(1).expect("Invalid version"));

        let serialized = serializer
            .serialize_stored_event(&stored_event, "TestEvent")
            .await
            .expect("Failed to serialize stored event");

        let deserialized: StoredEvent<TestEvent> = serializer
            .deserialize_stored_event(&serialized, "TestEvent")
            .await
            .expect("Failed to deserialize stored event");

        assert_eq!(stored_event.event.id, deserialized.event.id);
        assert_eq!(stored_event.event.stream_id, deserialized.event.stream_id);
        assert_eq!(stored_event.event.payload, deserialized.event.payload);
        assert_eq!(stored_event.version, deserialized.version);
    }
}
