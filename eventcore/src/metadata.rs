//! Event metadata types for the `EventCore` event sourcing library.
//!
//! This module defines types for tracking event metadata such as causation,
//! correlation, and actor information. These types follow the parse-don't-validate
//! principle with validation at construction boundaries.

use crate::types::{EventId, Timestamp};
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A correlation identifier that links related events across command boundaries.
///
/// Correlation IDs help trace related events that belong to the same logical workflow
/// or user session, even when they span multiple commands or services.
#[nutype(
    validate(predicate = |id: &Uuid| id.get_version() == Some(uuid::Version::SortRand)),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    /// Creates a new correlation ID with the current timestamp.
    ///
    /// This generates a new `UUIDv7` for correlation tracking.
    pub fn new() -> Self {
        // This will always succeed as Uuid::now_v7() always returns a valid v7 UUID
        Self::try_new(Uuid::now_v7()).expect("Uuid::now_v7() should always return a valid v7 UUID")
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

/// A causation identifier that links an event to the specific event that caused it.
///
/// Causation IDs create a direct parent-child relationship between events,
/// enabling precise event lineage tracking.
#[nutype(
    validate(predicate = |id: &Uuid| id.get_version() == Some(uuid::Version::SortRand)),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct CausationId(Uuid);

impl From<EventId> for CausationId {
    /// Creates a causation ID from an event ID.
    ///
    /// This is the typical way to set causation - the ID of the event
    /// that directly caused this new event.
    fn from(event_id: EventId) -> Self {
        // EventId is guaranteed to be v7, so this conversion is always safe
        Self::try_new(*event_id.as_ref())
            .expect("EventId should always be a valid v7 UUID for CausationId")
    }
}

/// A user identifier that tracks which user or system actor performed an action.
///
/// User IDs are validated to be non-empty and within reasonable length limits.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct UserId(String);

/// Comprehensive metadata for events in the event store.
///
/// This struct captures all the contextual information about when, why,
/// and by whom an event was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMetadata {
    /// When the event was created
    pub timestamp: Timestamp,
    /// Links events in the same logical workflow or session
    pub correlation_id: CorrelationId,
    /// Links this event to the specific event that caused it
    pub causation_id: Option<CausationId>,
    /// Identifies the user or system that created the event
    pub user_id: Option<UserId>,
    /// Additional custom metadata
    #[serde(default)]
    pub custom: std::collections::HashMap<String, serde_json::Value>,
}

impl EventMetadata {
    /// Creates a new event metadata with the current timestamp and a new correlation ID.
    pub fn new() -> Self {
        Self {
            timestamp: Timestamp::now(),
            correlation_id: CorrelationId::new(),
            causation_id: None,
            user_id: None,
            custom: std::collections::HashMap::new(),
        }
    }

    /// Creates metadata for an event caused by another event.
    pub fn caused_by(
        causing_event_id: EventId,
        correlation_id: CorrelationId,
        user_id: Option<UserId>,
    ) -> Self {
        Self {
            timestamp: Timestamp::now(),
            correlation_id,
            causation_id: Some(CausationId::from(causing_event_id)),
            user_id,
            custom: std::collections::HashMap::new(),
        }
    }

    /// Sets the causation ID.
    #[must_use]
    pub const fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    /// Sets the correlation ID.
    #[must_use]
    pub const fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    /// Sets the user ID.
    #[must_use]
    pub fn with_user_id(mut self, user_id: Option<UserId>) -> Self {
        self.user_id = user_id;
        self
    }

    /// Adds custom metadata.
    #[must_use]
    pub fn with_custom(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.custom.insert(key.into(), value);
        self
    }
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing event metadata with a fluent interface.
#[derive(Debug, Clone)]
pub struct EventMetadataBuilder {
    timestamp: Option<Timestamp>,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    user_id: Option<UserId>,
    custom: std::collections::HashMap<String, serde_json::Value>,
}

impl EventMetadataBuilder {
    /// Creates a new metadata builder.
    pub fn new() -> Self {
        Self {
            timestamp: None,
            correlation_id: None,
            causation_id: None,
            user_id: None,
            custom: std::collections::HashMap::new(),
        }
    }

    /// Sets the timestamp for the event.
    #[must_use]
    pub const fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the correlation ID for the event.
    #[must_use]
    pub const fn correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Sets the causation ID for the event.
    #[must_use]
    pub const fn causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    /// Sets causation from an event ID.
    #[must_use]
    pub fn caused_by(mut self, event_id: EventId) -> Self {
        self.causation_id = Some(CausationId::from(event_id));
        self
    }

    /// Sets the user ID for the event.
    #[must_use]
    pub fn user_id(mut self, user_id: UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Adds a custom metadata field.
    #[must_use]
    pub fn custom<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<serde_json::Value>,
    {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Builds the event metadata.
    ///
    /// Uses defaults for any unspecified fields:
    /// - timestamp: current time
    /// - `correlation_id`: new random ID
    /// - `causation_id`: None
    /// - `user_id`: None
    pub fn build(self) -> EventMetadata {
        EventMetadata {
            timestamp: self.timestamp.unwrap_or_else(Timestamp::now),
            correlation_id: self.correlation_id.unwrap_or_default(),
            causation_id: self.causation_id,
            user_id: self.user_id,
            custom: self.custom,
        }
    }
}

impl Default for EventMetadataBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventId;
    use proptest::prelude::*;

    // CorrelationId property tests
    proptest! {
        #[test]
        fn correlation_id_accepts_valid_uuid_v7(uuid_bytes in any::<[u8; 16]>()) {
            // Create a valid v7 UUID by setting the correct version and variant bits
            let mut bytes = uuid_bytes;
            // Set version to 7 (0111) in the high nibble of the 7th byte
            bytes[6] = (bytes[6] & 0x0F) | 0x70;
            // Set variant to RFC4122 (10) in the high bits of the 9th byte
            bytes[8] = (bytes[8] & 0x3F) | 0x80;

            let uuid = Uuid::from_bytes(bytes);
            let result = CorrelationId::try_new(uuid);
            prop_assert!(result.is_ok());
            prop_assert_eq!(*result.unwrap().as_ref(), uuid);
        }

        #[test]
        fn correlation_id_rejects_non_v7_uuids(uuid_bytes in any::<[u8; 16]>(), version in 0u8..=6u8) {
            // Create UUIDs with versions other than 7
            let mut bytes = uuid_bytes;
            bytes[6] = (bytes[6] & 0x0F) | (version << 4);
            bytes[8] = (bytes[8] & 0x3F) | 0x80;

            let uuid = Uuid::from_bytes(bytes);
            let result = CorrelationId::try_new(uuid);
            prop_assert!(result.is_err());
        }

        #[test]
        fn correlation_id_roundtrip_serialization(_: ()) {
            let correlation_id = CorrelationId::new();
            let json = serde_json::to_string(&correlation_id).unwrap();
            let deserialized: CorrelationId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(correlation_id, deserialized);
        }
    }

    // CausationId property tests
    proptest! {
        #[test]
        fn causation_id_accepts_valid_uuid_v7(uuid_bytes in any::<[u8; 16]>()) {
            // Create a valid v7 UUID by setting the correct version and variant bits
            let mut bytes = uuid_bytes;
            // Set version to 7 (0111) in the high nibble of the 7th byte
            bytes[6] = (bytes[6] & 0x0F) | 0x70;
            // Set variant to RFC4122 (10) in the high bits of the 9th byte
            bytes[8] = (bytes[8] & 0x3F) | 0x80;

            let uuid = Uuid::from_bytes(bytes);
            let result = CausationId::try_new(uuid);
            prop_assert!(result.is_ok());
            prop_assert_eq!(*result.unwrap().as_ref(), uuid);
        }

        #[test]
        fn causation_id_from_event_id_preserves_value(_: ()) {
            let event_id = EventId::new();
            let causation_id = CausationId::from(event_id);
            prop_assert_eq!(*causation_id.as_ref(), *event_id.as_ref());
        }

        #[test]
        fn causation_id_roundtrip_serialization(_: ()) {
            let event_id = EventId::new();
            let causation_id = CausationId::from(event_id);
            let json = serde_json::to_string(&causation_id).unwrap();
            let deserialized: CausationId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(causation_id, deserialized);
        }
    }

    // UserId property tests
    proptest! {
        #[test]
        fn user_id_accepts_valid_strings(s in "[a-zA-Z0-9_@.-]{1,255}") {
            let result = UserId::try_new(s.clone());
            prop_assert!(result.is_ok());
            let user_id = result.unwrap();
            prop_assert_eq!(user_id.as_ref(), &s);
        }

        #[test]
        fn user_id_trims_whitespace(s in " {0,10}[a-zA-Z0-9_@.-]{1,240} {0,10}") {
            let result = UserId::try_new(s.clone());
            prop_assert!(result.is_ok());
            let user_id = result.unwrap();
            prop_assert_eq!(user_id.as_ref(), s.trim());
        }

        #[test]
        fn user_id_rejects_empty_strings(s in " {0,50}") {
            let result = UserId::try_new(s);
            prop_assert!(result.is_err());
        }

        #[test]
        fn user_id_rejects_strings_over_255_chars(s in "[a-zA-Z0-9]{256,500}") {
            let result = UserId::try_new(s);
            prop_assert!(result.is_err());
        }

        #[test]
        fn user_id_roundtrip_serialization(s in "[a-zA-Z0-9_@.-]{1,255}") {
            let user_id = UserId::try_new(s).unwrap();
            let json = serde_json::to_string(&user_id).unwrap();
            let deserialized: UserId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(user_id, deserialized);
        }
    }

    // EventMetadata property tests
    proptest! {
        #[test]
        fn event_metadata_roundtrip_serialization(_: ()) {
            let metadata = EventMetadata::new();
            let json = serde_json::to_string(&metadata).unwrap();
            let deserialized: EventMetadata = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(metadata, deserialized);
        }

        #[test]
        fn event_metadata_with_all_fields_roundtrip_serialization(_: ()) {
            let event_id = EventId::new();
            let user_id = UserId::try_new("test-user").unwrap();
            let metadata = EventMetadata::caused_by(event_id, CorrelationId::new(), Some(user_id));

            let json = serde_json::to_string(&metadata).unwrap();
            let deserialized: EventMetadata = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(metadata, deserialized);
        }
    }

    // EventMetadataBuilder property tests
    proptest! {
        #[test]
        fn builder_creates_valid_metadata(_: ()) {
            let metadata = EventMetadataBuilder::new().build();

            // Verify required fields are set
            prop_assert!(metadata.timestamp.as_datetime() <= &chrono::Utc::now());
            prop_assert_eq!(
                metadata.correlation_id.as_ref().get_version(),
                Some(uuid::Version::SortRand)
            );
        }

        #[test]
        fn builder_with_custom_fields_preserves_values(_: ()) {
            let correlation_id = CorrelationId::new();
            let event_id = EventId::new();
            let user_id = UserId::try_new("test-user").unwrap();

            let metadata = EventMetadataBuilder::new()
                .correlation_id(correlation_id)
                .caused_by(event_id)
                .user_id(user_id.clone())
                .custom("key1", "value1")
                .custom("key2", 42)
                .build();

            prop_assert_eq!(metadata.correlation_id, correlation_id);
            prop_assert_eq!(metadata.causation_id, Some(CausationId::from(event_id)));
            prop_assert_eq!(metadata.user_id, Some(user_id));
            let expected_value1 = serde_json::Value::String("value1".to_string());
            let expected_value2 = serde_json::Value::Number(serde_json::Number::from(42));
            prop_assert_eq!(metadata.custom.get("key1"), Some(&expected_value1));
            prop_assert_eq!(metadata.custom.get("key2"), Some(&expected_value2));
        }
    }

    // Unit tests for specific behaviors
    #[test]
    fn correlation_id_new_creates_valid_v7() {
        let correlation_id = CorrelationId::new();
        assert_eq!(
            correlation_id.as_ref().get_version(),
            Some(uuid::Version::SortRand)
        );
    }

    #[test]
    fn correlation_id_default_creates_new() {
        let id1 = CorrelationId::default();
        let id2 = CorrelationId::default();
        // They should be different (extremely high probability)
        assert_ne!(id1, id2);
    }

    #[test]
    fn causation_id_from_event_id_works() {
        let event_id = EventId::new();
        let causation_id = CausationId::from(event_id);
        assert_eq!(*causation_id.as_ref(), *event_id.as_ref());
    }

    #[test]
    fn user_id_rejects_specific_invalid_cases() {
        // Empty string
        assert!(UserId::try_new("").is_err());

        // Only whitespace
        assert!(UserId::try_new("   ").is_err());
        assert!(UserId::try_new("\t\n\r").is_err());

        // String that's too long (256 chars)
        let long_string = "a".repeat(256);
        assert!(UserId::try_new(long_string).is_err());

        // Valid edge case: exactly 255 chars
        let max_string = "a".repeat(255);
        assert!(UserId::try_new(max_string).is_ok());
    }

    #[test]
    fn event_metadata_new_sets_defaults() {
        let metadata = EventMetadata::new();

        assert!(metadata.timestamp.as_datetime() <= &chrono::Utc::now());
        assert_eq!(
            metadata.correlation_id.as_ref().get_version(),
            Some(uuid::Version::SortRand)
        );
        assert_eq!(metadata.causation_id, None);
        assert_eq!(metadata.user_id, None);
        assert!(metadata.custom.is_empty());
    }

    #[test]
    fn event_metadata_caused_by_sets_causation() {
        let event_id = EventId::new();
        let correlation_id = CorrelationId::new();
        let user_id = UserId::try_new("test-user").unwrap();

        let metadata = EventMetadata::caused_by(event_id, correlation_id, Some(user_id.clone()));

        assert_eq!(metadata.correlation_id, correlation_id);
        assert_eq!(metadata.causation_id, Some(CausationId::from(event_id)));
        assert_eq!(metadata.user_id, Some(user_id));
    }

    #[test]
    fn builder_default_creates_new_builder() {
        let builder = EventMetadataBuilder::default();
        let metadata = builder.build();

        // Should have defaults for required fields
        assert!(metadata.timestamp.as_datetime() <= &chrono::Utc::now());
        assert_eq!(
            metadata.correlation_id.as_ref().get_version(),
            Some(uuid::Version::SortRand)
        );
    }

    #[test]
    fn builder_custom_metadata_serialization() {
        let metadata = EventMetadataBuilder::new()
            .custom("string_field", "test")
            .custom("number_field", 42)
            .custom("bool_field", true)
            .custom("array_field", vec![1, 2, 3])
            .build();

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: EventMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata, deserialized);
        assert_eq!(
            deserialized.custom.get("string_field"),
            Some(&serde_json::Value::String("test".to_string()))
        );
        assert_eq!(
            deserialized.custom.get("number_field"),
            Some(&serde_json::Value::Number(serde_json::Number::from(42)))
        );
        assert_eq!(
            deserialized.custom.get("bool_field"),
            Some(&serde_json::Value::Bool(true))
        );
    }
}
