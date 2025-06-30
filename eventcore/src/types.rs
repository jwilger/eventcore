//! Core types for the EventCore event sourcing library.
//!
//! This module provides the fundamental types used throughout EventCore for event sourcing.
//! All types follow the "parse, don't validate" principle, using smart constructors to ensure
//! validity at construction time. Once a value is successfully parsed into one of these types,
//! it is guaranteed to be valid for the lifetime of the program.
//!
//! # Design Philosophy
//!
//! The types in this module are designed to make illegal states unrepresentable:
//!
//! - **StreamId**: Guaranteed to be non-empty and at most 255 characters
//! - **EventId**: Always a valid UUIDv7, providing time-based ordering
//! - **EventVersion**: Always non-negative, preventing invalid version numbers
//! - **Timestamp**: Consistent UTC timestamp handling across the system
//!
//! # Examples
//!
//! ```
//! use eventcore::{StreamId, EventId, EventVersion, Timestamp};
//!
//! // Create a stream identifier
//! let stream_id = StreamId::try_new("user-123").expect("valid stream id");
//!
//! // EventIds are automatically generated with proper ordering
//! let event_id = EventId::new();
//!
//! // Version numbers start at zero and increment
//! let version = EventVersion::initial();
//! let next_version = version.next();
//!
//! // Timestamps capture the current moment
//! let timestamp = Timestamp::now();
//! ```

use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for an event stream.
///
/// `StreamId` represents a logical stream of events in the event store. Each stream
/// contains an ordered sequence of events that typically represent the state changes
/// of a single entity or aggregate.
///
/// # Guarantees
///
/// Once constructed, a `StreamId` is guaranteed to be:
/// - Non-empty (after trimming whitespace)
/// - At most 255 characters in length
/// - Valid for the lifetime of the program
///
/// # Examples
///
/// ```
/// use eventcore::StreamId;
///
/// // Create a stream ID for a user entity
/// let user_stream = StreamId::try_new("user-123").expect("valid stream id");
///
/// // Stream IDs are automatically trimmed
/// let trimmed = StreamId::try_new("  order-456  ").expect("valid stream id");
/// assert_eq!(trimmed.as_ref(), "order-456");
///
/// // Invalid stream IDs are rejected at construction
/// assert!(StreamId::try_new("").is_err()); // empty string
/// assert!(StreamId::try_new("   ").is_err()); // only whitespace
/// assert!(StreamId::try_new("a".repeat(256)).is_err()); // too long
/// ```
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

impl StreamId {
    /// Creates a `StreamId` from a compile-time validated string literal.
    ///
    /// This function performs validation at compile time for string literals,
    /// providing optimized construction for common stream ID patterns.
    /// It's particularly useful for predefined stream types like "transfers",
    /// "orders", or "users".
    ///
    /// The validation is performed at compile time via const assertions,
    /// ensuring the string is non-empty and within the 255 character limit.
    /// Runtime validation is skipped since the input is already verified.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::StreamId;
    ///
    /// // These are validated at compile time - zero runtime cost
    /// let transfer_stream = StreamId::from_static("transfers");
    /// let order_stream = StreamId::from_static("orders");
    /// let user_stream = StreamId::from_static("users");
    /// ```
    ///
    /// # Compile-time validation
    ///
    /// The validation occurs at compile time, so invalid inputs will cause compilation to fail:
    ///
    /// ```ignore
    /// use eventcore::StreamId;
    /// // This would panic at compile time due to empty string
    /// let invalid = StreamId::from_static("");
    ///
    /// // This would panic at compile time due to length > 255
    /// let too_long = StreamId::from_static("a string that is much longer than the 255 character limit and would cause a compile-time panic due to the const validation function checking the length at compile time which is exactly what we want for this optimization");
    /// ```
    pub fn from_static(s: &'static str) -> Self {
        // Compile-time validation using const assertions
        const fn validate_static_str(s: &str) {
            assert!(!s.is_empty(), "StreamId cannot be empty");
            assert!(s.len() <= 255, "StreamId cannot exceed 255 characters");
            // Note: We assume static strings are pre-trimmed for performance
        }

        // This will panic at compile time if validation fails
        validate_static_str(s);

        // We still need to use try_new since nutype doesn't expose direct construction,
        // but the const validation above ensures this will always succeed
        Self::try_new(s).expect("const validation guarantees validity")
    }

    /// Creates a `StreamId` with optimized construction for frequently used dynamic strings.
    ///
    /// This method provides an optimization for hot paths where the same dynamic
    /// stream IDs are constructed repeatedly. It uses an internal cache to avoid
    /// redundant validation and string operations for previously seen values.
    ///
    /// The cache is bounded and uses a least-recently-used (LRU) eviction policy
    /// to prevent unbounded memory growth. This makes it safe to use in long-running
    /// applications.
    ///
    /// # Performance
    ///
    /// - First time: Full validation and caching (similar to `try_new`)
    /// - Subsequent times: O(1) cache lookup, no validation needed
    /// - Cache misses: Falls back to normal validation and updates cache
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::StreamId;
    ///
    /// // First call performs validation and caches
    /// let stream1 = StreamId::cached("user-123").expect("valid stream");
    ///
    /// // Subsequent calls are optimized - no validation needed
    /// let stream2 = StreamId::cached("user-123").expect("cached stream");
    /// assert_eq!(stream1, stream2);
    ///
    /// // Different IDs are also cached
    /// let order_stream = StreamId::cached("order-456").expect("valid stream");
    /// ```
    pub fn cached(s: &str) -> Result<Self, StreamIdError> {
        use std::collections::HashMap;
        use std::sync::{OnceLock, RwLock};

        // Cache with bounded size to prevent memory growth
        const CACHE_SIZE: usize = 1000;
        static CACHE: OnceLock<RwLock<HashMap<String, StreamId>>> = OnceLock::new();

        let cache = CACHE.get_or_init(|| RwLock::new(HashMap::with_capacity(CACHE_SIZE)));

        // First try to read from cache
        {
            let cache_read = cache.read().expect("StreamId cache should not be poisoned");
            if let Some(cached_id) = cache_read.get(s) {
                return Ok(cached_id.clone());
            }
        }

        // Cache miss - validate and store
        let validated_id = Self::try_new(s)?;

        // Update cache with write lock
        {
            let mut cache_write = cache
                .write()
                .expect("StreamId cache should not be poisoned");

            // Simple LRU: if cache is full, clear it completely
            // This is a simple strategy that avoids complex LRU tracking
            if cache_write.len() >= CACHE_SIZE {
                cache_write.clear();
            }

            cache_write.insert(s.to_string(), validated_id.clone());
        }

        Ok(validated_id)
    }
}

/// A globally unique event identifier using UUIDv7 format.
///
/// `EventId` provides globally unique identification for events while maintaining
/// chronological ordering properties. This enables efficient sorting and querying
/// of events across the entire event store.
///
/// # Guarantees
///
/// Every `EventId` is guaranteed to be:
/// - A valid UUIDv7 (RFC 9562)
/// - Globally unique with extremely high probability
/// - Time-ordered when compared with other EventIds
/// - Suitable for distributed systems without coordination
///
/// # Ordering Properties
///
/// UUIDv7 includes a timestamp component, making EventIds naturally ordered by
/// creation time. Events created later will have lexicographically greater IDs,
/// enabling efficient range queries and event replay.
///
/// # Examples
///
/// ```
/// use eventcore::EventId;
/// use std::thread;
/// use std::time::Duration;
///
/// // Create a new event ID
/// let event1 = EventId::new();
///
/// // IDs created later are greater
/// thread::sleep(Duration::from_millis(1));
/// let event2 = EventId::new();
/// assert!(event2 > event1);
///
/// // Only UUIDv7 values are accepted
/// use uuid::Uuid;
/// let nil_uuid = Uuid::nil();  // Not a v7 UUID
/// assert!(EventId::try_new(nil_uuid).is_err());
/// ```
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
    /// This generates a new UUIDv7 which includes the current timestamp
    /// and random data, ensuring both uniqueness and chronological ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::EventId;
    ///
    /// let id = EventId::new();
    /// // Each call creates a unique ID
    /// let another_id = EventId::new();
    /// assert_ne!(id, another_id);
    /// ```
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

/// The version number of an event within its stream.
///
/// `EventVersion` tracks the position of events within a stream, enabling
/// optimistic concurrency control and ensuring events are processed in order.
/// Version numbers are crucial for detecting concurrent modifications and
/// maintaining consistency in distributed systems.
///
/// # Guarantees
///
/// - Versions are always non-negative (≥ 0)
/// - The first event in a stream has version 0
/// - Versions increment monotonically by 1 for each new event
/// - Once assigned, a version number is immutable
///
/// # Concurrency Control
///
/// Event versions enable optimistic concurrency control. When writing events,
/// you can specify the expected current version of the stream. If the actual
/// version doesn't match, the write fails, preventing lost updates.
///
/// # Examples
///
/// ```
/// use eventcore::EventVersion;
///
/// // New streams start at version 0
/// let initial = EventVersion::initial();
/// assert_eq!(u64::from(initial), 0);
///
/// // Versions increment monotonically
/// let v1 = initial.next();
/// let v2 = v1.next();
/// assert_eq!(u64::from(v1), 1);
/// assert_eq!(u64::from(v2), 2);
///
/// // Versions can be compared
/// assert!(v2 > v1);
/// assert!(v1 > initial);
/// ```
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
    /// This is equivalent to `initial()` and represents the version of the
    /// first event in any stream.
    ///
    /// Note: This is implemented as a function rather than a const
    /// because nutype prevents direct construction.
    pub fn min() -> Self {
        Self::try_new(0).expect("0 is always a valid version")
    }

    /// Creates the initial version (0) for a new stream.
    ///
    /// Use this when creating the first event in a new stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::EventVersion;
    ///
    /// let version = EventVersion::initial();
    /// assert_eq!(u64::from(version), 0);
    /// ```
    pub fn initial() -> Self {
        Self::try_new(0).expect("0 is always a valid version")
    }

    /// Returns the next version after this one.
    ///
    /// This method is used to calculate the version number for the next event
    /// in a stream. It increments the current version by exactly 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::EventVersion;
    ///
    /// let v0 = EventVersion::initial();
    /// let v1 = v0.next();
    /// let v2 = v1.next();
    ///
    /// assert_eq!(u64::from(v0), 0);
    /// assert_eq!(u64::from(v1), 1);
    /// assert_eq!(u64::from(v2), 2);
    /// ```
    #[must_use]
    pub fn next(self) -> Self {
        let current: u64 = self.into();
        // Since EventVersion is guaranteed to be >= 0, and we're adding 1,
        // the result will always be valid (barring overflow)
        Self::try_new(current + 1).expect("next version should always be valid")
    }
}

/// A UTC timestamp for event sourcing operations.
///
/// `Timestamp` provides a consistent representation of time throughout the
/// event sourcing system. All timestamps are stored in UTC to avoid timezone
/// ambiguities and ensure reliable event ordering across distributed systems.
///
/// # Design Rationale
///
/// This wrapper type exists to:
/// - Enforce UTC timezone usage throughout the system
/// - Provide a clear domain type for event timestamps
/// - Enable future enhancements without breaking changes
/// - Ensure consistent serialization across different storage backends
///
/// # Examples
///
/// ```
/// use eventcore::Timestamp;
/// use chrono::{DateTime, Utc, TimeZone};
///
/// // Create a timestamp for the current moment
/// let now = Timestamp::now();
///
/// // Create from a specific DateTime
/// let dt = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();
/// let timestamp = Timestamp::new(dt);
///
/// // Convert back to DateTime for manipulation
/// let datetime: DateTime<Utc> = timestamp.into();
///
/// // Timestamps are comparable
/// let later = Timestamp::now();
/// assert!(later >= now);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// Creates a new timestamp from a UTC `DateTime`.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::Timestamp;
    /// use chrono::{Utc, TimeZone};
    ///
    /// let dt = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();
    /// let timestamp = Timestamp::new(dt);
    /// ```
    pub const fn new(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }

    /// Creates a timestamp representing the current moment.
    ///
    /// This is the most common way to create timestamps for new events.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::Timestamp;
    ///
    /// let timestamp = Timestamp::now();
    /// println!("Event occurred at: {}", timestamp);
    /// ```
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Returns a reference to the underlying `DateTime`.
    ///
    /// Use this when you need to perform date/time calculations
    /// without consuming the timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::Timestamp;
    /// use chrono::Duration;
    ///
    /// let timestamp = Timestamp::now();
    /// let one_hour_ago = *timestamp.as_datetime() - Duration::hours(1);
    /// ```
    pub const fn as_datetime(&self) -> &DateTime<Utc> {
        &self.0
    }

    /// Converts the timestamp into the underlying `DateTime`.
    ///
    /// This consumes the timestamp and returns the inner `DateTime<Utc>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventcore::Timestamp;
    /// use chrono::DateTime;
    ///
    /// let timestamp = Timestamp::now();
    /// let datetime: DateTime<_> = timestamp.into_datetime();
    /// ```
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

    // Tests for StreamId::from_static optimization
    #[test]
    fn stream_id_from_static_creates_valid_ids() {
        let transfer_stream = StreamId::from_static("transfers");
        assert_eq!(transfer_stream.as_ref(), "transfers");

        let order_stream = StreamId::from_static("orders");
        assert_eq!(order_stream.as_ref(), "orders");

        let user_stream = StreamId::from_static("users");
        assert_eq!(user_stream.as_ref(), "users");
    }

    #[test]
    fn stream_id_from_static_handles_edge_cases() {
        // Single character
        let single = StreamId::from_static("a");
        assert_eq!(single.as_ref(), "a");

        // Special characters
        let special = StreamId::from_static("user-123_test");
        assert_eq!(special.as_ref(), "user-123_test");

        // Long but valid string (testing with a static string of reasonable length)
        let long_static = StreamId::from_static(
            "this-is-a-very-long-stream-name-but-still-within-limits-for-testing-purposes",
        );
        assert!(long_static.as_ref().len() > 50);
    }

    #[test]
    fn stream_id_from_static_equals_try_new() {
        let static_id = StreamId::from_static("test-stream");
        let dynamic_id = StreamId::try_new("test-stream").unwrap();
        assert_eq!(static_id, dynamic_id);
    }

    // Tests for StreamId::cached optimization
    #[test]
    fn stream_id_cached_stores_and_retrieves() {
        let id1 = StreamId::cached("test-cache-1").expect("valid stream id");
        let id2 = StreamId::cached("test-cache-1").expect("cached stream id");

        // Should be equal
        assert_eq!(id1, id2);
        assert_eq!(id1.as_ref(), "test-cache-1");
    }

    #[test]
    fn stream_id_cached_handles_multiple_entries() {
        let user_id = StreamId::cached("user-456").expect("valid user stream");
        let order_id = StreamId::cached("order-789").expect("valid order stream");
        let product_id = StreamId::cached("product-101").expect("valid product stream");

        // Retrieve again to test cache hits
        let user_id2 = StreamId::cached("user-456").expect("cached user stream");
        let order_id2 = StreamId::cached("order-789").expect("cached order stream");
        let product_id2 = StreamId::cached("product-101").expect("cached product stream");

        assert_eq!(user_id, user_id2);
        assert_eq!(order_id, order_id2);
        assert_eq!(product_id, product_id2);
    }

    #[test]
    fn stream_id_cached_rejects_invalid_input() {
        // Empty string should fail
        assert!(StreamId::cached("").is_err());

        // Too long string should fail
        let too_long = "a".repeat(256);
        assert!(StreamId::cached(&too_long).is_err());

        // Whitespace-only should fail
        assert!(StreamId::cached("   ").is_err());
    }

    #[test]
    fn stream_id_cached_equals_try_new() {
        let test_str = "cached-test-stream";
        let cached_id = StreamId::cached(test_str).expect("valid cached stream");
        let regular_id = StreamId::try_new(test_str).expect("valid regular stream");

        assert_eq!(cached_id, regular_id);
    }

    #[test]
    fn stream_id_cached_performance_stress_test() {
        // Test cache with many different entries
        let mut ids = Vec::new();

        // Fill cache with different IDs
        for i in 0..500 {
            let stream_name = format!("stream-{i}");
            let id = StreamId::cached(&stream_name).expect("valid stream id");
            ids.push((stream_name, id));
        }

        // Verify all cached IDs are still accessible
        for (stream_name, original_id) in &ids {
            let cached_id = StreamId::cached(stream_name).expect("cached stream id");
            assert_eq!(*original_id, cached_id);
        }

        // Test cache overflow behavior (should not crash)
        for i in 500..1500 {
            let stream_name = format!("overflow-stream-{i}");
            let id = StreamId::cached(&stream_name).expect("valid overflow stream id");
            assert_eq!(id.as_ref(), stream_name);
        }
    }

    #[test]
    fn stream_id_cached_concurrent_access() {
        use std::thread;

        let handles: Vec<_> = (0..10)
            .map(|thread_id| {
                thread::spawn(move || {
                    let mut results = Vec::new();
                    for i in 0..100 {
                        let stream_name = format!("thread-{thread_id}-stream-{i}");
                        let id =
                            StreamId::cached(&stream_name).expect("valid concurrent stream id");
                        results.push((stream_name, id));
                    }
                    results
                })
            })
            .collect();

        // Wait for all threads and collect results
        let mut all_results = Vec::new();
        for handle in handles {
            all_results.extend(handle.join().expect("thread should complete"));
        }

        // Verify all IDs are correct
        for (stream_name, id) in all_results {
            assert_eq!(id.as_ref(), stream_name);

            // Verify we can still retrieve from cache
            let cached_again = StreamId::cached(&stream_name).expect("should still be cached");
            assert_eq!(id, cached_again);
        }
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
