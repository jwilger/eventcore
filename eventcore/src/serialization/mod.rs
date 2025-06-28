//! Event serialization and deserialization infrastructure.
//!
//! This module provides traits and implementations for serializing events
//! to and from various formats, with support for schema evolution.

use crate::errors::EventStoreError;
use crate::event::StoredEvent;
use crate::metadata::EventMetadata;
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Trait for serializing and deserializing events.
///
/// Implementations of this trait handle the conversion between
/// typed events and their serialized representations, with support
/// for schema evolution and versioning.
#[async_trait]
pub trait EventSerializer: Send + Sync {
    /// Serializes an event payload to bytes.
    ///
    /// # Arguments
    ///
    /// * `event` - The event payload to serialize
    /// * `type_name` - The fully qualified type name of the event
    ///
    /// # Returns
    ///
    /// A byte array containing the serialized event data, or an error.
    async fn serialize<E>(&self, event: &E, type_name: &str) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync;

    /// Deserializes an event payload from bytes.
    ///
    /// # Arguments
    ///
    /// * `data` - The serialized event data
    /// * `type_name` - The fully qualified type name of the event
    ///
    /// # Returns
    ///
    /// The deserialized event, or an error if deserialization fails.
    async fn deserialize<E>(&self, data: &[u8], type_name: &str) -> Result<E, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync;

    /// Serializes a `StoredEvent` including all metadata.
    ///
    /// # Arguments
    ///
    /// * `event` - The stored event with metadata
    /// * `type_name` - The fully qualified type name of the event
    ///
    /// # Returns
    ///
    /// A byte array containing the serialized event with metadata.
    async fn serialize_stored_event<E>(
        &self,
        event: &StoredEvent<E>,
        type_name: &str,
    ) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync + PartialEq + Eq;

    /// Deserializes a `StoredEvent` including all metadata.
    ///
    /// # Arguments
    ///
    /// * `data` - The serialized event data
    /// * `type_name` - The fully qualified type name of the event
    ///
    /// # Returns
    ///
    /// The deserialized stored event with metadata.
    async fn deserialize_stored_event<E>(
        &self,
        data: &[u8],
        type_name: &str,
    ) -> Result<StoredEvent<E>, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + PartialEq + Eq;

    /// Gets the schema version for a given event type.
    ///
    /// This is used to support schema evolution by tracking
    /// which version of an event schema was used for serialization.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name of the event
    ///
    /// # Returns
    ///
    /// The schema version number.
    fn get_schema_version(&self, type_name: &str) -> u32;

    /// Checks if a serializer can handle a specific schema version.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The fully qualified type name of the event
    /// * `version` - The schema version to check
    ///
    /// # Returns
    ///
    /// `true` if the serializer can handle the version, `false` otherwise.
    fn supports_schema_version(&self, type_name: &str, version: u32) -> bool;
}

/// Schema migration trait for handling event evolution.
///
/// Implementations of this trait handle the migration of events
/// from older schema versions to newer ones.
#[async_trait]
pub trait SchemaEvolution: Send + Sync {
    /// Migrates event data from one schema version to another.
    ///
    /// # Arguments
    ///
    /// * `data` - The serialized event data in the old schema
    /// * `type_name` - The fully qualified type name of the event
    /// * `from_version` - The source schema version
    /// * `to_version` - The target schema version
    ///
    /// # Returns
    ///
    /// The migrated event data in the new schema format.
    async fn migrate(
        &self,
        data: &[u8],
        type_name: &str,
        from_version: u32,
        to_version: u32,
    ) -> Result<Vec<u8>, EventStoreError>;
}

/// Metadata included with serialized events for versioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedEventEnvelope {
    /// The schema version of the serialized event
    pub schema_version: u32,
    /// The fully qualified type name of the event
    pub type_name: String,
    /// The serialized event payload
    pub payload_data: Vec<u8>,
    /// The unique event ID
    pub event_id: EventId,
    /// The stream this event belongs to
    pub stream_id: StreamId,
    /// Event metadata (causation, correlation, etc.)
    pub event_metadata: EventMetadata,
    /// When the event was originally created
    pub created_at: Timestamp,
    /// The version of this event in its stream
    pub version: EventVersion,
    /// When the event was stored
    pub stored_at: Timestamp,
}

pub mod evolution;
pub mod json;

pub use evolution::{JsonSchemaEvolution, SchemaRegistry};
pub use json::JsonEventSerializer;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::testing::generators::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn serialized_event_envelope_round_trip(
            event_id in arb_event_id(),
            stream_id in arb_stream_id(),
            version in arb_event_version(),
            type_name in "[a-zA-Z][a-zA-Z0-9_]{4,20}",
            schema_version in 1u32..100,
            data_size in 10..1000usize,
            created_at in arb_timestamp(),
            stored_at in arb_timestamp()
        ) {
            use crate::metadata::EventMetadata;

            let payload_data = vec![0u8; data_size];
            let envelope = SerializedEventEnvelope {
                schema_version,
                type_name,
                payload_data,
                event_id,
                stream_id,
                event_metadata: EventMetadata::default(),
                created_at,
                version,
                stored_at,
            };

            // Serialize to JSON
            let serialized = serde_json::to_vec(&envelope)
                .expect("Serialization should succeed");

            // Deserialize back
            let deserialized: SerializedEventEnvelope = serde_json::from_slice(&serialized)
                .expect("Deserialization should succeed");

            // Verify all fields match
            prop_assert_eq!(envelope.schema_version, deserialized.schema_version);
            prop_assert_eq!(envelope.type_name, deserialized.type_name);
            prop_assert_eq!(envelope.payload_data, deserialized.payload_data);
            prop_assert_eq!(envelope.event_id, deserialized.event_id);
            prop_assert_eq!(envelope.stream_id, deserialized.stream_id);
            prop_assert_eq!(envelope.event_metadata, deserialized.event_metadata);
            prop_assert_eq!(envelope.created_at, deserialized.created_at);
            prop_assert_eq!(envelope.version, deserialized.version);
            prop_assert_eq!(envelope.stored_at, deserialized.stored_at);
        }
    }
}
