//! Serialization format configuration and factory.

use super::{
    BincodeEventSerializer, EventSerializer, EventStoreError, JsonEventSerializer,
    MessagePackEventSerializer, SchemaEvolution,
};
use crate::event::StoredEvent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Supported serialization formats for event storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SerializationFormat {
    /// JSON format (default) - human-readable, good for debugging
    Json,
    /// MessagePack format - efficient binary format, smaller than JSON
    MessagePack,
    /// Bincode format - fast binary format, optimized for speed
    Bincode,
}

impl Default for SerializationFormat {
    fn default() -> Self {
        Self::Json
    }
}

/// Serializer that can handle multiple formats
#[derive(Clone)]
pub enum FormatSerializer {
    /// JSON serializer
    Json(JsonEventSerializer),
    /// MessagePack serializer
    MessagePack(MessagePackEventSerializer),
    /// Bincode serializer
    Bincode(BincodeEventSerializer),
}

impl SerializationFormat {
    /// Creates a new event serializer for this format.
    pub fn create_serializer(&self) -> FormatSerializer {
        match self {
            Self::Json => FormatSerializer::Json(JsonEventSerializer::new()),
            Self::MessagePack => FormatSerializer::MessagePack(MessagePackEventSerializer::new()),
            Self::Bincode => FormatSerializer::Bincode(BincodeEventSerializer::new()),
        }
    }

    /// Creates a new event serializer for this format with schema evolution support.
    pub fn create_serializer_with_evolution(
        &self,
        evolution: Arc<dyn SchemaEvolution>,
    ) -> FormatSerializer {
        match self {
            Self::Json => {
                FormatSerializer::Json(JsonEventSerializer::with_evolution(evolution.clone()))
            }
            Self::MessagePack => FormatSerializer::MessagePack(
                MessagePackEventSerializer::with_evolution(evolution.clone()),
            ),
            Self::Bincode => {
                FormatSerializer::Bincode(BincodeEventSerializer::with_evolution(evolution))
            }
        }
    }

    /// Returns the file extension commonly associated with this format.
    pub const fn file_extension(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::MessagePack => "msgpack",
            Self::Bincode => "bincode",
        }
    }

    /// Returns the MIME type for this format.
    pub const fn mime_type(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::MessagePack => "application/msgpack",
            Self::Bincode => "application/octet-stream",
        }
    }
}

impl std::fmt::Display for SerializationFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "JSON"),
            Self::MessagePack => write!(f, "MessagePack"),
            Self::Bincode => write!(f, "Bincode"),
        }
    }
}

impl std::str::FromStr for SerializationFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "messagepack" | "msgpack" => Ok(Self::MessagePack),
            "bincode" => Ok(Self::Bincode),
            _ => Err(format!("Unknown serialization format: {s}")),
        }
    }
}

#[async_trait]
impl EventSerializer for FormatSerializer {
    async fn serialize<E>(&self, event: &E, type_name: &str) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync,
    {
        match self {
            Self::Json(s) => s.serialize(event, type_name).await,
            Self::MessagePack(s) => s.serialize(event, type_name).await,
            Self::Bincode(s) => s.serialize(event, type_name).await,
        }
    }

    async fn deserialize<E>(&self, data: &[u8], type_name: &str) -> Result<E, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync,
    {
        match self {
            Self::Json(s) => s.deserialize(data, type_name).await,
            Self::MessagePack(s) => s.deserialize(data, type_name).await,
            Self::Bincode(s) => s.deserialize(data, type_name).await,
        }
    }

    async fn serialize_stored_event<E>(
        &self,
        event: &StoredEvent<E>,
        type_name: &str,
    ) -> Result<Vec<u8>, EventStoreError>
    where
        E: Serialize + Send + Sync + PartialEq + Eq,
    {
        match self {
            Self::Json(s) => s.serialize_stored_event(event, type_name).await,
            Self::MessagePack(s) => s.serialize_stored_event(event, type_name).await,
            Self::Bincode(s) => s.serialize_stored_event(event, type_name).await,
        }
    }

    async fn deserialize_stored_event<E>(
        &self,
        data: &[u8],
        type_name: &str,
    ) -> Result<StoredEvent<E>, EventStoreError>
    where
        E: for<'de> Deserialize<'de> + Send + Sync + PartialEq + Eq,
    {
        match self {
            Self::Json(s) => s.deserialize_stored_event(data, type_name).await,
            Self::MessagePack(s) => s.deserialize_stored_event(data, type_name).await,
            Self::Bincode(s) => s.deserialize_stored_event(data, type_name).await,
        }
    }

    fn get_schema_version(&self, type_name: &str) -> u32 {
        match self {
            Self::Json(s) => s.get_schema_version(type_name),
            Self::MessagePack(s) => s.get_schema_version(type_name),
            Self::Bincode(s) => s.get_schema_version(type_name),
        }
    }

    fn supports_schema_version(&self, type_name: &str, version: u32) -> bool {
        match self {
            Self::Json(s) => s.supports_schema_version(type_name, version),
            Self::MessagePack(s) => s.supports_schema_version(type_name, version),
            Self::Bincode(s) => s.supports_schema_version(type_name, version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_format() {
        assert_eq!(SerializationFormat::default(), SerializationFormat::Json);
    }

    #[test]
    fn test_file_extensions() {
        assert_eq!(SerializationFormat::Json.file_extension(), "json");
        assert_eq!(SerializationFormat::MessagePack.file_extension(), "msgpack");
        assert_eq!(SerializationFormat::Bincode.file_extension(), "bincode");
    }

    #[test]
    fn test_mime_types() {
        assert_eq!(SerializationFormat::Json.mime_type(), "application/json");
        assert_eq!(
            SerializationFormat::MessagePack.mime_type(),
            "application/msgpack"
        );
        assert_eq!(
            SerializationFormat::Bincode.mime_type(),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "json".parse::<SerializationFormat>().unwrap(),
            SerializationFormat::Json
        );
        assert_eq!(
            "JSON".parse::<SerializationFormat>().unwrap(),
            SerializationFormat::Json
        );
        assert_eq!(
            "messagepack".parse::<SerializationFormat>().unwrap(),
            SerializationFormat::MessagePack
        );
        assert_eq!(
            "msgpack".parse::<SerializationFormat>().unwrap(),
            SerializationFormat::MessagePack
        );
        assert_eq!(
            "bincode".parse::<SerializationFormat>().unwrap(),
            SerializationFormat::Bincode
        );
        assert!("invalid".parse::<SerializationFormat>().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(SerializationFormat::Json.to_string(), "JSON");
        assert_eq!(SerializationFormat::MessagePack.to_string(), "MessagePack");
        assert_eq!(SerializationFormat::Bincode.to_string(), "Bincode");
    }

    #[tokio::test]
    async fn test_format_serializer() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct TestData {
            value: String,
        }

        let data = TestData {
            value: "test".to_string(),
        };

        // Test each format
        for format in [
            SerializationFormat::Json,
            SerializationFormat::MessagePack,
            SerializationFormat::Bincode,
        ] {
            let serializer = format.create_serializer();
            let serialized_data = serializer.serialize(&data, "TestData").await.unwrap();
            let deserialized: TestData = serializer
                .deserialize(&serialized_data, "TestData")
                .await
                .unwrap();
            assert_eq!(data, deserialized);
        }
    }
}
