//! Property test generators for domain types.
//!
//! This module provides `proptest` generators for all domain types in the eventcore library.
//! Each generator respects the validation rules of its corresponding type.

use crate::metadata::{CausationId, CorrelationId, EventMetadata, EventMetadataBuilder, UserId};
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;

/// Generates valid `StreamId` values with enhanced shrinking.
///
/// `StreamIds` are guaranteed to be:
/// - Non-empty after trimming
/// - At most 255 characters
///
/// The shrinking strategy prioritizes:
/// 1. Shorter strings (easier to debug)
/// 2. Alphanumeric characters (more readable)
/// 3. Common prefixes and patterns
///
/// # Example
/// ```rust,ignore
/// use proptest::prelude::*;
/// use eventcore::testing::generators::arb_stream_id;
///
/// proptest! {
///     #[test]
///     fn test_with_stream_id(stream_id in arb_stream_id()) {
///         assert!(!stream_id.as_ref().is_empty());
///     }
/// }
/// ```
pub fn arb_stream_id() -> impl Strategy<Value = StreamId> {
    // Enhanced shrinking strategy: Start with simple patterns and add complexity
    prop_oneof![
        // High probability: Simple patterns that shrink well
        20 => "[a-zA-Z][a-zA-Z0-9]{0,10}",
        // Medium probability: Common patterns with separators
        15 => "[a-zA-Z][a-zA-Z0-9]{0,10}-[a-zA-Z0-9]{1,10}",
        // Medium probability: UUID-like patterns
        10 => "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}",
        // Lower probability: Complex patterns
        5 => "[a-zA-Z0-9][a-zA-Z0-9._-]{10,50}",
        // Very low probability: Maximum complexity
        1 => "[a-zA-Z0-9][a-zA-Z0-9._-]{50,254}",
    ]
    .prop_filter_map("Invalid StreamId", |s| StreamId::try_new(s).ok())
}

/// Generates valid `StreamId` values with a specific prefix.
///
/// Useful for creating `StreamIds` that follow a naming convention.
pub fn arb_stream_id_with_prefix(prefix: &'static str) -> impl Strategy<Value = StreamId> {
    prop::strategy::LazyJust::new(move || 255_usize.saturating_sub(prefix.len()).saturating_sub(1))
        .prop_flat_map(move |max_len| {
            prop::string::string_regex(&format!("[a-zA-Z0-9._-]{{0,{max_len}}}"))
                .expect("valid regex")
                .prop_filter_map("Invalid StreamId", move |suffix| {
                    let full_id = format!("{prefix}-{suffix}");
                    StreamId::try_new(full_id).ok()
                })
        })
}

/// Generates valid `EventId` values (`UUIDv7`).
///
/// `EventIds` are guaranteed to be `UUIDv7` format for time-based ordering.
pub fn arb_event_id() -> impl Strategy<Value = EventId> {
    any::<()>().prop_map(|()| EventId::new())
}

/// Generates valid `EventVersion` values with enhanced shrinking.
///
/// Versions are non-negative integers. The shrinking strategy prioritizes:
/// 1. Smaller version numbers (easier to debug)
/// 2. Common values like 0, 1, 2 (typical edge cases)
/// 3. Powers of 2 and common boundaries
pub fn arb_event_version() -> impl Strategy<Value = EventVersion> {
    prop_oneof![
        // High probability: Common small values that shrink well
        40 => 0u64..=10u64,
        // Medium probability: Medium range values
        30 => 10u64..=100u64,
        // Medium probability: Boundary values and powers of 2 (within the 1000 limit)
        20 => prop_oneof![
            Just(0u64), Just(1u64), Just(2u64), Just(7u64), Just(8u64), Just(15u64), Just(16u64),
            Just(31u64), Just(32u64), Just(63u64), Just(64u64), Just(127u64), Just(128u64),
            Just(255u64), Just(256u64), Just(511u64), Just(512u64), Just(999u64), Just(1000u64)
        ],
        // Lower probability: Large values
        10 => 100u64..=1000u64,
    ]
    .prop_filter_map("Invalid EventVersion", |v| EventVersion::try_new(v).ok())
}

/// Generates small `EventVersion` values suitable for testing with enhanced shrinking.
///
/// Limited to 0-10 for more predictable test scenarios.
/// Shrinks toward 0 for simpler failure cases.
pub fn arb_small_event_version() -> impl Strategy<Value = EventVersion> {
    prop_oneof![
        // Very high probability for small values
        50 => 0u64..=3u64,
        // Medium probability for slightly larger
        30 => 3u64..=7u64,
        // Lower probability for upper range
        20 => 7u64..=10u64,
    ]
    .prop_filter_map("Invalid EventVersion", |v| EventVersion::try_new(v).ok())
}

/// Generates valid `Timestamp` values.
///
/// Timestamps are within a reasonable range to avoid overflow issues.
pub fn arb_timestamp() -> impl Strategy<Value = Timestamp> {
    (0i64..=253_402_300_799i64) // Up to year 9999
        .prop_filter_map("Invalid timestamp", |secs| {
            Utc.timestamp_opt(secs, 0).single().map(Timestamp::new)
        })
}

/// Generates recent `Timestamp` values (within the last year).
pub fn arb_recent_timestamp() -> impl Strategy<Value = Timestamp> {
    prop::strategy::LazyJust::new(|| {
        let now = Utc::now();
        let year_ago = now.timestamp() - 365 * 24 * 60 * 60;
        (year_ago, now.timestamp())
    })
    .prop_flat_map(|(year_ago, now_ts)| {
        (year_ago..=now_ts).prop_map(move |secs| {
            let dt = Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now);
            Timestamp::new(dt)
        })
    })
}

/// Generates valid `CorrelationId` values.
pub fn arb_correlation_id() -> impl Strategy<Value = CorrelationId> {
    any::<()>().prop_map(|()| CorrelationId::new())
}

/// Generates valid `CausationId` values.
///
/// Since `CausationId` is typically created from `EventId`, this generator
/// creates an `EventId` first and converts it.
pub fn arb_causation_id() -> impl Strategy<Value = CausationId> {
    arb_event_id().prop_map(CausationId::from)
}

/// Generates valid `UserId` values.
///
/// `UserIds` are guaranteed to be:
/// - Non-empty after trimming
/// - At most 255 characters
pub fn arb_user_id() -> impl Strategy<Value = UserId> {
    "[a-zA-Z0-9][a-zA-Z0-9._@-]{0,254}"
        .prop_filter_map("Invalid UserId", |s| UserId::try_new(s).ok())
}

/// Generates email-like `UserId` values.
pub fn arb_email_user_id() -> impl Strategy<Value = UserId> {
    "[a-z]{3,10}@[a-z]{3,10}\\.(com|org|net)"
        .prop_filter_map("Invalid email UserId", |s| UserId::try_new(s).ok())
}

/// Generates `EventMetadata` with all fields populated.
pub fn arb_event_metadata() -> impl Strategy<Value = EventMetadata> {
    (
        arb_timestamp(),
        arb_correlation_id(),
        prop::option::of(arb_causation_id()),
        prop::option::of(arb_user_id()),
    )
        .prop_map(|(timestamp, correlation_id, causation_id, user_id)| {
            let mut builder = EventMetadataBuilder::new()
                .timestamp(timestamp)
                .correlation_id(correlation_id);

            if let Some(causation_id) = causation_id {
                builder = builder.causation_id(causation_id);
            }

            if let Some(user_id) = user_id {
                builder = builder.user_id(user_id);
            }

            builder.build()
        })
}

/// Generates minimal `EventMetadata` (only required fields).
pub fn arb_minimal_event_metadata() -> impl Strategy<Value = EventMetadata> {
    any::<()>().prop_map(|()| EventMetadata::new())
}

/// Generates a custom value for `EventMetadata` custom fields.
pub fn arb_custom_metadata_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        any::<String>().prop_map(serde_json::Value::String),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
    ]
}

/// Generates a vector of valid `StreamId` values.
pub fn arb_stream_id_vec(
    size: impl Into<prop::collection::SizeRange>,
) -> impl Strategy<Value = Vec<StreamId>> {
    prop::collection::vec(arb_stream_id(), size)
}

/// Generates a vector of valid `EventId` values.
pub fn arb_event_id_vec(
    size: impl Into<prop::collection::SizeRange>,
) -> impl Strategy<Value = Vec<EventId>> {
    prop::collection::vec(arb_event_id(), size)
}

/// Generates ordered event versions starting from initial.
pub fn arb_ordered_versions(count: usize) -> impl Strategy<Value = Vec<EventVersion>> {
    Just(
        (0..count)
            .map(|i| EventVersion::try_new(i as u64).unwrap())
            .collect(),
    )
}

/// Generates collections optimized for concurrency testing.
///
/// These generators produce smaller, more manageable collections that lead to
/// better shrinking when concurrency issues are found.
/// Generates a small collection of stream IDs optimized for concurrency testing.
///
/// Prefers smaller collections and reuses stream IDs to increase collision probability.
pub fn arb_concurrent_stream_ids() -> impl Strategy<Value = Vec<StreamId>> {
    prop_oneof![
        // High probability: Very small collections with reuse
        30 => prop::collection::vec(
            prop_oneof![
                3 => Just("stream-a".to_string()),
                3 => Just("stream-b".to_string()),
                2 => Just("stream-c".to_string()),
                1 => "[a-z]{1,3}".prop_map(|s| s),
            ].prop_filter_map("Invalid StreamId", |s| StreamId::try_new(s).ok()),
            1..=3
        ),
        // Medium probability: Small collections
        25 => prop::collection::vec(arb_stream_id(), 2..=5),
        // Lower probability: Medium collections
        15 => prop::collection::vec(arb_stream_id(), 3..=8),
    ]
}

/// Generates small operation counts optimized for concurrency testing.
///
/// Prefers smaller counts that shrink toward 1 to find minimal failing cases.
pub fn arb_concurrent_operation_count() -> impl Strategy<Value = usize> {
    prop_oneof![
        // Very high probability: Small counts
        50 => 1usize..=3,
        // High probability: Medium counts
        30 => 2usize..=5,
        // Medium probability: Larger counts
        15 => 4usize..=8,
        // Low probability: Large counts
        5 => 6usize..=15,
    ]
}

/// Generates amounts optimized for transfer testing with enhanced shrinking.
///
/// Prefers small amounts and common values that tend to cause edge cases.
pub fn arb_transfer_amount() -> impl Strategy<Value = u64> {
    prop_oneof![
        // High probability: Very small amounts
        40 => 1u64..=10u64,
        // Medium probability: Small amounts
        25 => 5u64..=50u64,
        // Medium probability: Common boundary values
        20 => prop_oneof![
            Just(1u64), Just(10u64), Just(100u64), Just(1000u64),
            Just(255u64), Just(256u64), Just(1023u64), Just(1024u64),
        ],
        // Lower probability: Larger amounts
        15 => 50u64..=1000u64,
    ]
}

/// Generates small collections of amounts for batch operations.
pub fn arb_transfer_amounts() -> impl Strategy<Value = Vec<u64>> {
    prop::collection::vec(arb_transfer_amount(), 1..=5)
}

/// Generates strings optimized for concurrency testing.
///
/// Prefers shorter strings and reuses common values to increase collision probability.
pub fn arb_concurrent_string() -> impl Strategy<Value = String> {
    prop_oneof![
        // High probability: Very short strings with reuse
        40 => prop_oneof![
            Just("a".to_string()),
            Just("b".to_string()),
            Just("c".to_string()),
            Just("id1".to_string()),
            Just("id2".to_string()),
            Just("test".to_string()),
        ],
        // Medium probability: Short generated strings
        30 => "[a-z]{1,5}".prop_map(|s| s),
        // Lower probability: Medium strings
        20 => "[a-z0-9]{3,10}".prop_map(|s| s),
        // Very low probability: Longer strings
        10 => "[a-zA-Z0-9_-]{5,20}".prop_map(|s| s),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    proptest! {
        #[test]
        fn generated_stream_ids_are_valid(stream_id in arb_stream_id()) {
            // If we can generate it, it should be valid
            assert!(!stream_id.as_ref().is_empty());
            assert!(stream_id.as_ref().len() <= 255);
        }

        #[test]
        fn generated_stream_ids_with_prefix_are_valid(stream_id in arb_stream_id_with_prefix("test")) {
            assert!(stream_id.as_ref().starts_with("test-"));
            assert!(stream_id.as_ref().len() <= 255);
        }

        #[test]
        fn generated_event_ids_are_uuidv7(event_id in arb_event_id()) {
            assert_eq!(event_id.as_ref().get_version(), Some(uuid::Version::SortRand));
        }

        #[test]
        fn generated_event_versions_are_valid(version in arb_event_version()) {
            let value: u64 = version.into();
            assert!(value <= 1000);
        }

        #[test]
        fn generated_timestamps_are_valid(timestamp in arb_timestamp()) {
            use chrono::Datelike;
            let dt = timestamp.as_datetime();
            assert!(dt.year() <= 9999);
            assert!(dt.year() >= 1970);
        }

        #[test]
        fn generated_recent_timestamps_are_recent(timestamp in arb_recent_timestamp()) {
            let now = Utc::now();
            let year_ago = now - chrono::Duration::days(365);
            let dt = timestamp.as_datetime();
            assert!(dt >= &year_ago);
            assert!(dt <= &now);
        }

        #[test]
        fn generated_correlation_ids_are_uuidv7(correlation_id in arb_correlation_id()) {
            assert_eq!(correlation_id.as_ref().get_version(), Some(uuid::Version::SortRand));
        }

        #[test]
        fn generated_causation_ids_are_uuidv7(causation_id in arb_causation_id()) {
            assert_eq!(causation_id.as_ref().get_version(), Some(uuid::Version::SortRand));
        }

        #[test]
        fn generated_user_ids_are_valid(user_id in arb_user_id()) {
            assert!(!user_id.as_ref().is_empty());
            assert!(user_id.as_ref().len() <= 255);
        }

        #[test]
        fn generated_email_user_ids_look_like_emails(user_id in arb_email_user_id()) {
            assert!(user_id.as_ref().contains('@'));
            assert!(user_id.as_ref().contains('.'));
        }

        #[test]
        fn generated_event_metadata_has_required_fields(metadata in arb_event_metadata()) {
            use chrono::Datelike;
            // All metadata should have valid timestamp and correlation_id
            // Don't test timestamp against current time to avoid flaky timing issues
            assert!(metadata.timestamp.as_datetime().year() >= 1970);
            assert!(metadata.timestamp.as_datetime().year() <= 9999);
            assert_eq!(metadata.correlation_id.as_ref().get_version(), Some(uuid::Version::SortRand));
        }

        #[test]
        fn generated_minimal_metadata_has_defaults(metadata in arb_minimal_event_metadata()) {
            assert!(metadata.causation_id.is_none());
            assert!(metadata.user_id.is_none());
            assert!(metadata.custom.is_empty());
        }

        #[test]
        fn generated_stream_id_roundtrips(stream_id in arb_stream_id()) {
            let json = serde_json::to_string(&stream_id).unwrap();
            let deserialized: StreamId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(stream_id, deserialized);
        }

        #[test]
        fn generated_ordered_versions_are_sequential(_: ()) {
            let versions = arb_ordered_versions(5).prop_flat_map(Just).new_tree(&mut TestRunner::default()).unwrap().current();
            assert_eq!(versions.len(), 5);
            for (i, version) in versions.iter().enumerate() {
                let value: u64 = (*version).into();
                assert_eq!(value, i as u64);
            }
        }
    }

    #[test]
    fn specific_generator_tests() {
        // Test that generators produce expected values
        let mut runner = TestRunner::default();

        // Stream ID generator should produce valid values
        for _ in 0..10 {
            let stream_id = arb_stream_id().new_tree(&mut runner).unwrap().current();
            assert!(!stream_id.as_ref().is_empty());
        }

        // Event ID generator should produce unique values
        let id1 = arb_event_id().new_tree(&mut runner).unwrap().current();
        let id2 = arb_event_id().new_tree(&mut runner).unwrap().current();
        assert_ne!(id1, id2);

        // Ordered versions should be predictable
        let versions = arb_ordered_versions(3)
            .new_tree(&mut runner)
            .unwrap()
            .current();
        assert_eq!(versions[0], EventVersion::initial());
        assert_eq!(versions[1], EventVersion::try_new(1).unwrap());
        assert_eq!(versions[2], EventVersion::try_new(2).unwrap());
    }
}
