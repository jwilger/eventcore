//! Core types for the `EventCore` event sourcing library.
//!
//! This module defines the fundamental types used throughout the library.
//! All types use smart constructors to ensure validity at construction time,
//! following the "parse, don't validate" principle.

use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A stream identifier that uniquely identifies an event stream.
///
/// `StreamId` values are guaranteed to be non-empty and at most 255 characters.
/// Once constructed, a `StreamId` is always valid - no further validation needed.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct StreamId(String);

/// A globally unique event identifier using UUIDv7 format.
///
/// `EventId` values are guaranteed to be UUIDv7, which provides:
/// - Time-based ordering capability
/// - Globally unique identification
/// - Monotonic sort order for events created in sequence
#[nutype(
    validate(predicate = |id: &Uuid| id.get_version() == Some(uuid::Version::SortRand)),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct EventId(Uuid);

impl EventId {
    /// Creates a new `EventId` with the current timestamp.
    ///
    /// This is a convenience method that generates a new `UUIDv7`.
    pub fn new() -> Self {
        // This will always succeed as Uuid::now_v7() always returns a valid v7 UUID
        Self::try_new(Uuid::now_v7()).expect("Uuid::now_v7() should always return a valid v7 UUID")
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

/// The version of an event within a stream.
///
/// Versions start at 0 and increment monotonically with each event.
/// The type system ensures versions can never be negative.
#[nutype(
    validate(greater_or_equal = 0),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        Display,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct EventVersion(u64);

impl EventVersion {
    /// The minimum possible event version (0).
    ///
    /// Note: This is implemented as a function rather than a const
    /// because nutype prevents direct construction.
    pub fn min() -> Self {
        Self::try_new(0).expect("0 is always a valid version")
    }

    /// Creates the initial version (0) for a new stream.
    pub fn initial() -> Self {
        Self::try_new(0).expect("0 is always a valid version")
    }

    /// Returns the next version after this one.
    #[must_use]
    pub fn next(self) -> Self {
        let current: u64 = self.into();
        // Since EventVersion is guaranteed to be >= 0, and we're adding 1,
        // the result will always be valid (barring overflow)
        Self::try_new(current + 1).expect("next version should always be valid")
    }
}

/// A timestamp for when an event occurred.
///
/// This wrapper ensures consistent timestamp handling throughout the system
/// and enables future enhancements like custom serialization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// Creates a new timestamp from a UTC `DateTime`.
    pub const fn new(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }

    /// Creates a timestamp representing the current moment.
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Returns the underlying `DateTime`.
    pub const fn as_datetime(&self) -> &DateTime<Utc> {
        &self.0
    }

    /// Converts the timestamp into the underlying `DateTime`.
    pub const fn into_datetime(self) -> DateTime<Utc> {
        self.0
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(datetime: DateTime<Utc>) -> Self {
        Self::new(datetime)
    }
}

impl From<Timestamp> for DateTime<Utc> {
    fn from(timestamp: Timestamp) -> Self {
        timestamp.into_datetime()
    }
}

impl AsRef<DateTime<Utc>> for Timestamp {
    fn as_ref(&self) -> &DateTime<Utc> {
        self.as_datetime()
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // StreamId property tests
    proptest! {
        #[test]
        fn stream_id_accepts_valid_strings(s in "[a-zA-Z0-9_-]{1,255}") {
            let result = StreamId::try_new(s.clone());
            prop_assert!(result.is_ok());
            let stream_id = result.unwrap();
            prop_assert_eq!(stream_id.as_ref(), &s);
        }

        #[test]
        fn stream_id_trims_whitespace(s in " {0,10}[a-zA-Z0-9_-]{1,240} {0,10}") {
            let result = StreamId::try_new(s.clone());
            prop_assert!(result.is_ok());
            let stream_id = result.unwrap();
            prop_assert_eq!(stream_id.as_ref(), s.trim());
        }

        #[test]
        fn stream_id_rejects_empty_strings(s in " {0,50}") {
            let result = StreamId::try_new(s);
            prop_assert!(result.is_err());
        }

        #[test]
        fn stream_id_rejects_strings_over_255_chars(s in "[a-zA-Z0-9]{256,500}") {
            let result = StreamId::try_new(s);
            prop_assert!(result.is_err());
        }

        #[test]
        fn stream_id_roundtrip_serialization(s in "[a-zA-Z0-9_-]{1,255}") {
            let stream_id = StreamId::try_new(s).unwrap();
            let json = serde_json::to_string(&stream_id).unwrap();
            let deserialized: StreamId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(stream_id, deserialized);
        }
    }

    // EventId property tests
    proptest! {
        #[test]
        fn event_id_accepts_valid_uuid_v7(uuid_bytes in any::<[u8; 16]>()) {
            // Create a valid v7 UUID by setting the correct version and variant bits
            let mut bytes = uuid_bytes;
            // Set version to 7 (0111) in the high nibble of the 7th byte
            bytes[6] = (bytes[6] & 0x0F) | 0x70;
            // Set variant to RFC4122 (10) in the high bits of the 9th byte
            bytes[8] = (bytes[8] & 0x3F) | 0x80;

            let uuid = Uuid::from_bytes(bytes);
            let result = EventId::try_new(uuid);
            prop_assert!(result.is_ok());
            prop_assert_eq!(*result.unwrap().as_ref(), uuid);
        }

        #[test]
        fn event_id_rejects_non_v7_uuids(uuid_bytes in any::<[u8; 16]>(), version in 0u8..=6u8) {
            // Create UUIDs with versions other than 7
            let mut bytes = uuid_bytes;
            bytes[6] = (bytes[6] & 0x0F) | (version << 4);
            bytes[8] = (bytes[8] & 0x3F) | 0x80;

            let uuid = Uuid::from_bytes(bytes);
            let result = EventId::try_new(uuid);
            prop_assert!(result.is_err());
        }


        #[test]
        fn event_id_ordering_is_consistent(uuid_bytes1 in any::<[u8; 16]>(), uuid_bytes2 in any::<[u8; 16]>()) {
            // Create two valid v7 UUIDs
            let mut bytes1 = uuid_bytes1;
            bytes1[6] = (bytes1[6] & 0x0F) | 0x70;
            bytes1[8] = (bytes1[8] & 0x3F) | 0x80;

            let mut bytes2 = uuid_bytes2;
            bytes2[6] = (bytes2[6] & 0x0F) | 0x70;
            bytes2[8] = (bytes2[8] & 0x3F) | 0x80;

            let id1 = EventId::try_new(Uuid::from_bytes(bytes1)).unwrap();
            let id2 = EventId::try_new(Uuid::from_bytes(bytes2)).unwrap();

            // Verify ordering is transitive
            if id1 < id2 {
                prop_assert!(id2 >= id1);
            }
            if id1 == id2 {
                prop_assert!(id1 >= id2 && id2 >= id1);
            }
        }

        #[test]
        fn event_id_roundtrip_serialization(_: ()) {
            let event_id = EventId::new();
            let json = serde_json::to_string(&event_id).unwrap();
            let deserialized: EventId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(event_id, deserialized);
        }
    }

    // EventVersion property tests
    proptest! {
        #[test]
        fn event_version_accepts_non_negative_values(v in 0u64..=u64::MAX) {
            let result = EventVersion::try_new(v);
            prop_assert!(result.is_ok());
            let version = result.unwrap();
            let value: u64 = version.into();
            prop_assert_eq!(value, v);
        }

        #[test]
        fn event_version_next_increments_by_one(v in 0u64..u64::MAX) {
            let version = EventVersion::try_new(v).unwrap();
            let next = version.next();
            let next_value: u64 = next.into();
            prop_assert_eq!(next_value, v + 1);
        }

        #[test]
        fn event_version_ordering_is_consistent(v1 in 0u64..=u64::MAX, v2 in 0u64..=u64::MAX) {
            let version1 = EventVersion::try_new(v1).unwrap();
            let version2 = EventVersion::try_new(v2).unwrap();

            prop_assert_eq!(version1 < version2, v1 < v2);
            prop_assert_eq!(version1 == version2, v1 == v2);
            prop_assert_eq!(version1 > version2, v1 > v2);
        }

        #[test]
        fn event_version_roundtrip_serialization(v in 0u64..=u64::MAX) {
            let version = EventVersion::try_new(v).unwrap();
            let json = serde_json::to_string(&version).unwrap();
            let deserialized: EventVersion = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(version, deserialized);
        }
    }

    // Timestamp property tests
    proptest! {
        #[test]
        fn timestamp_from_datetime_preserves_value(
            secs in i64::MIN/1000..i64::MAX/1000,
            nanos in 0u32..1_000_000_000u32
        ) {
            use chrono::TimeZone;

            if let Some(dt) = Utc.timestamp_opt(secs, nanos).single() {
                let timestamp = Timestamp::new(dt);
                prop_assert_eq!(timestamp.as_datetime(), &dt);
                prop_assert_eq!(timestamp.into_datetime(), dt);
            }
        }

        #[test]
        fn timestamp_ordering_matches_datetime_ordering(
            secs1 in i64::MIN/1000..i64::MAX/1000,
            nanos1 in 0u32..1_000_000_000u32,
            secs2 in i64::MIN/1000..i64::MAX/1000,
            nanos2 in 0u32..1_000_000_000u32
        ) {
            use chrono::TimeZone;

            if let (Some(dt1), Some(dt2)) = (
                Utc.timestamp_opt(secs1, nanos1).single(),
                Utc.timestamp_opt(secs2, nanos2).single()
            ) {
                let ts1 = Timestamp::new(dt1);
                let ts2 = Timestamp::new(dt2);

                prop_assert_eq!(ts1 < ts2, dt1 < dt2);
                prop_assert_eq!(ts1 == ts2, dt1 == dt2);
                prop_assert_eq!(ts1 > ts2, dt1 > dt2);
            }
        }

        #[test]
        fn timestamp_roundtrip_serialization(
            secs in i64::MIN/1000..i64::MAX/1000,
            nanos in 0u32..1_000_000_000u32
        ) {
            use chrono::TimeZone;

            if let Some(dt) = Utc.timestamp_opt(secs, nanos).single() {
                let timestamp = Timestamp::new(dt);
                let json = serde_json::to_string(&timestamp).unwrap();
                let deserialized: Timestamp = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(timestamp, deserialized);
            }
        }
    }

    // Additional unit tests for specific cases
    #[test]
    fn event_version_initial_is_zero() {
        let initial = EventVersion::initial();
        let value: u64 = initial.into();
        assert_eq!(value, 0);
    }

    #[test]
    fn event_id_new_creates_valid_v7() {
        let event_id = EventId::new();
        assert_eq!(
            event_id.as_ref().get_version(),
            Some(uuid::Version::SortRand)
        );
    }

    #[test]
    fn event_id_default_creates_new() {
        let id1 = EventId::default();
        let id2 = EventId::default();
        // They should be different (extremely high probability)
        assert_ne!(id1, id2);
    }

    #[test]
    fn timestamp_now_creates_current_time() {
        let before = Utc::now();
        let timestamp = Timestamp::now();
        let after = Utc::now();

        assert!(timestamp.as_datetime() >= &before);
        assert!(timestamp.as_datetime() <= &after);
    }

    // Tests to verify smart constructors reject invalid inputs
    #[test]
    fn stream_id_rejects_specific_invalid_cases() {
        // Empty string
        assert!(StreamId::try_new("").is_err());

        // Only whitespace
        assert!(StreamId::try_new("   ").is_err());
        assert!(StreamId::try_new("\t\n\r").is_err());

        // String that's too long (256 chars)
        let long_string = "a".repeat(256);
        assert!(StreamId::try_new(long_string).is_err());

        // Valid edge case: exactly 255 chars
        let max_string = "a".repeat(255);
        assert!(StreamId::try_new(max_string).is_ok());
    }

    #[test]
    fn event_id_rejects_specific_invalid_uuids() {
        // Create a v4 UUID manually by setting version bits
        let mut bytes = [0u8; 16];
        bytes[6] = (bytes[6] & 0x0F) | 0x40; // Set version to 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // Set variant
        let v4_uuid = Uuid::from_bytes(bytes);
        assert!(EventId::try_new(v4_uuid).is_err());

        // Nil UUID
        let nil_uuid = Uuid::nil();
        assert!(EventId::try_new(nil_uuid).is_err());

        // Max UUID (all bits set, which won't be v7)
        let max_uuid = Uuid::max();
        assert!(EventId::try_new(max_uuid).is_err());
    }

    // Helper functions for trait assertions
    fn assert_stream_id_traits<
        T: std::fmt::Debug
            + Clone
            + PartialEq
            + Eq
            + std::hash::Hash
            + AsRef<str>
            + std::fmt::Display
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>,
    >() {
    }

    fn assert_event_id_traits<
        T: std::fmt::Debug
            + Clone
            + PartialEq
            + Eq
            + PartialOrd
            + Ord
            + std::hash::Hash
            + AsRef<Uuid>
            + std::fmt::Display
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>,
    >() {
    }

    fn assert_event_version_traits<
        T: std::fmt::Debug
            + Clone
            + Copy
            + PartialEq
            + Eq
            + PartialOrd
            + Ord
            + std::hash::Hash
            + std::fmt::Display
            + Into<u64>
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>,
    >() {
    }

    fn assert_timestamp_traits<
        T: std::fmt::Debug
            + Clone
            + Copy
            + PartialEq
            + Eq
            + PartialOrd
            + Ord
            + std::hash::Hash
            + std::fmt::Display
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>,
    >() {
    }

    #[test]
    fn all_types_implement_expected_traits() {
        assert_stream_id_traits::<StreamId>();
        assert_event_id_traits::<EventId>();
        assert_event_version_traits::<EventVersion>();
        assert_timestamp_traits::<Timestamp>();
    }
}
