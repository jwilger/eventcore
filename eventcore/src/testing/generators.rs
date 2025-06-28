//! Property test generators for domain types.
//!
//! This module provides `proptest` generators for all domain types in the eventcore library.
//! Each generator respects the validation rules of its corresponding type.

use crate::metadata::{CausationId, CorrelationId, EventMetadata, EventMetadataBuilder, UserId};
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;

/// Generates valid `StreamId` values.
///
/// `StreamIds` are guaranteed to be:
/// - Non-empty after trimming
/// - At most 255 characters
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
    "[a-zA-Z0-9][a-zA-Z0-9._-]{0,254}"
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

/// Generates valid `EventVersion` values.
///
/// Versions are non-negative integers.
pub fn arb_event_version() -> impl Strategy<Value = EventVersion> {
    (0u64..=1000u64).prop_filter_map("Invalid EventVersion", |v| EventVersion::try_new(v).ok())
}

/// Generates small `EventVersion` values suitable for testing.
///
/// Limited to 0-10 for more predictable test scenarios.
pub fn arb_small_event_version() -> impl Strategy<Value = EventVersion> {
    (0u64..=10u64).prop_filter_map("Invalid EventVersion", |v| EventVersion::try_new(v).ok())
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
