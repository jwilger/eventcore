use crate::{
    BatchSize, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, EventTypeName,
    PersistedEventEnvelope, PersistedEventId, ProjectionSelection, ProjectionSelectionId,
    ProjectionSource, ProjectionStreamFilter, ProjectorName, StreamId, StreamVersion,
};
use proptest::prelude::*;
use serde_json::value::RawValue;
use std::convert::Infallible;
use std::num::NonZeroU64;
use uuid::Uuid;

fn raw_json(json: &str) -> Box<RawValue> {
    RawValue::from_string(json.to_owned()).expect("fixture JSON is valid")
}

fn source_id() -> DeliverySourceId {
    DeliverySourceId::try_new("event-store-primary").expect("valid source ID")
}

fn selection() -> ProjectionSelection {
    ProjectionSelection::try_new(
        ProjectionSelectionId::try_new("accounting-v1").expect("valid selection ID"),
        ProjectionStreamFilter::All,
        vec![EventTypeName::try_new("invoice-issued").expect("valid event type")],
    )
    .expect("non-empty unique selection")
}

fn envelope(position: u64) -> PersistedEventEnvelope {
    PersistedEventEnvelope::new(
        source_id(),
        DeliveryPosition::new(NonZeroU64::new(position).expect("positive fixture position")),
        PersistedEventId::new(
            Uuid::parse_str("018e8c5e-8c5e-7000-8000-000000000001").expect("valid UUID"),
        ),
        StreamId::try_new("account-123").expect("valid stream ID"),
        StreamVersion::new(4),
        EventTypeName::try_new("invoice-issued").expect("valid event type"),
        raw_json(r#"{ "amount": 42, "currency": "USD" }"#),
        raw_json(r#"{ "correlationId": "c-7" }"#),
    )
}

// Break caught: removing trim-and-reject validation would allow a blank source identity
// or expose a differently scoped source key to durable projection progress.
#[test]
fn delivery_identities_trim_valid_values_and_reject_blank_values() {
    let source = DeliverySourceId::try_new("  orders-primary  ").expect("non-blank source ID");
    let projector =
        ProjectorName::try_new("  orders-read-model  ").expect("non-blank projector name");
    let selection =
        ProjectionSelectionId::try_new("  orders-v2  ").expect("non-blank selection ID");
    let event_type = EventTypeName::try_new("  order-confirmed  ").expect("non-blank event type");

    assert_eq!(source.as_ref(), "orders-primary");
    assert_eq!(projector.as_ref(), "orders-read-model");
    assert_eq!(selection.as_ref(), "orders-v2");
    assert_eq!(event_type.as_ref(), "order-confirmed");

    assert!(DeliverySourceId::try_new(" \t\n ").is_err());
    assert!(ProjectorName::try_new(" \t\n ").is_err());
    assert!(ProjectionSelectionId::try_new(" \t\n ").is_err());
    assert!(EventTypeName::try_new(" \t\n ").is_err());
}

// Break caught: accepting whitespace-only source IDs would make distinct or unusable
// delivery sources share a progress namespace.
proptest! {
    #[test]
    fn delivery_source_identity_accepts_exactly_non_blank_values(value in ".*") {
        let expected = !value.trim().is_empty();
        prop_assert_eq!(DeliverySourceId::try_new(value).is_ok(), expected);
    }
}

// Break caught: storing a zero, truncated, or otherwise altered global position would make
// resume boundaries ambiguous and can skip committed events.
proptest! {
    #[test]
    fn delivery_position_round_trips(value in 1_u64..=u64::MAX) {
        let position = DeliveryPosition::new(NonZeroU64::new(value).expect("strategy is positive"));
        prop_assert_eq!(position.get(), value);
    }
}

// Break caught: allowing an empty or duplicated event-type selection would either create an
// undefined projection contract or silently duplicate delivery work.
#[test]
fn selection_requires_at_least_one_unique_event_type() {
    let selection_id = ProjectionSelectionId::try_new("orders-v1").expect("valid selection ID");
    assert!(
        ProjectionSelection::try_new(
            selection_id.clone(),
            ProjectionStreamFilter::All,
            Vec::new(),
        )
        .is_err()
    );

    let event_type = EventTypeName::try_new("order-confirmed").expect("valid event type");
    assert!(
        ProjectionSelection::try_new(
            selection_id,
            ProjectionStreamFilter::All,
            vec![event_type.clone(), event_type],
        )
        .is_err()
    );
}

// Break caught: returning a changed selection ID, selector, or event-type list would let a
// source read a different durable projection contract than the runner persisted.
#[test]
fn selection_exposes_the_exact_declared_contract() {
    let selection_id = ProjectionSelectionId::try_new("orders-v2").expect("valid selection ID");
    let stream_filter = ProjectionStreamFilter::All;
    let event_types = vec![
        EventTypeName::try_new("order-confirmed").expect("valid event type"),
        EventTypeName::try_new("order-cancelled").expect("valid event type"),
    ];

    let selection = ProjectionSelection::try_new(
        selection_id.clone(),
        stream_filter.clone(),
        event_types.clone(),
    )
    .expect("non-empty unique selection");

    assert_eq!(selection.id(), &selection_id);
    assert_eq!(selection.stream_filter(), &stream_filter);
    assert_eq!(selection.event_types(), event_types.as_slice());
}

// Break caught: parsing and reserializing payload or metadata would change the bytes that the
// application decoder receives, despite the source contract requiring opaque JSON preservation.
#[test]
fn persisted_envelope_exposes_exact_fields_and_preserves_raw_json() {
    let envelope = envelope(9);

    let position: DeliveryPosition = envelope.position();
    let event_id: PersistedEventId = envelope.event_id();
    let stream_version: StreamVersion = envelope.stream_version();
    assert_copy(position);
    assert_copy(event_id);
    assert_copy(stream_version);

    assert_eq!(envelope.source_id().as_ref(), "event-store-primary");
    assert_eq!(position.get(), 9);
    assert_eq!(
        event_id.get(),
        Uuid::parse_str("018e8c5e-8c5e-7000-8000-000000000001").expect("valid UUID"),
    );
    assert_eq!(envelope.stream_id().as_ref(), "account-123");
    assert_eq!(stream_version, StreamVersion::new(4));
    assert_eq!(envelope.event_type().as_ref(), "invoice-issued");
    assert_eq!(
        envelope.payload().get(),
        r#"{ "amount": 42, "currency": "USD" }"#
    );
    assert_eq!(envelope.metadata().get(), r#"{ "correlationId": "c-7" }"#);
}

fn assert_copy<T: Copy>(_: T) {}

fn assert_projection_source<T: ProjectionSource>(_: T) {}

struct FixedSource {
    id: DeliverySourceId,
}

impl FixedSource {
    fn new() -> Self {
        Self { id: source_id() }
    }
}

impl ProjectionSource for FixedSource {
    type Error = Infallible;

    fn source_id(&self) -> &DeliverySourceId {
        &self.id
    }

    async fn high_watermark(&self) -> Result<Option<DeliveryPosition>, Self::Error> {
        Ok(Some(DeliveryPosition::new(
            NonZeroU64::new(12).expect("positive position"),
        )))
    }

    async fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> Result<Vec<PersistedEventEnvelope>, Self::Error> {
        let requested_expected_page = selection.id().as_ref() == "accounting-v1"
            && selection.stream_filter() == &ProjectionStreamFilter::All
            && selection.event_types()
                == [EventTypeName::try_new("invoice-issued").expect("valid event type")]
            && after.map(DeliveryPosition::get) == Some(8)
            && through
                == DeliveryUpperBound::Inclusive(DeliveryPosition::new(
                    NonZeroU64::new(12).expect("positive position"),
                ))
            && usize::from(limit) == 25;

        if requested_expected_page {
            Ok(vec![envelope(9)])
        } else {
            Ok(Vec::new())
        }
    }
}

// Break caught: omitting or weakening the blanket ProjectionSource implementation for &T would
// prevent runners from borrowing a source while preserving its complete delivery behavior.
#[tokio::test]
async fn borrowed_projection_source_forwards_identity_watermark_and_page_requests() {
    let source = FixedSource::new();
    let borrowed = &source;
    assert_projection_source::<&FixedSource>(borrowed);

    assert_eq!(borrowed.source_id().as_ref(), "event-store-primary");
    assert_eq!(
        borrowed
            .high_watermark()
            .await
            .expect("fixed source is infallible")
            .expect("fixed source has a watermark")
            .get(),
        12,
    );

    let envelopes = borrowed
        .read_envelopes(
            &selection(),
            Some(DeliveryPosition::new(
                NonZeroU64::new(8).expect("positive position"),
            )),
            DeliveryUpperBound::Inclusive(DeliveryPosition::new(
                NonZeroU64::new(12).expect("positive position"),
            )),
            BatchSize::new(25),
        )
        .await
        .expect("fixed source is infallible");

    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].position().get(), 9);
}
