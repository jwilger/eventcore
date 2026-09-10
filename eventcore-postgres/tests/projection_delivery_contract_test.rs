//! Public PostgreSQL contract tests for the strict, lossless projection source.

mod common;
#[path = "common/projection_delivery.rs"]
mod projection_delivery;

use std::collections::HashSet;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::time::Duration;

use eventcore_postgres::{PostgresEventStore, PostgresProjectionSource};
use eventcore_types::{
    BatchSize, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, EventTypeName,
    PersistedEventId, ProjectionSelection, ProjectionSelectionId, ProjectionSource,
    ProjectionStreamFilter, StreamId, StreamPattern, StreamPrefix, StreamVersion,
};
use futures::FutureExt;
use sqlx::{Executor, Postgres, query, query_scalar};
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

const DATABASE_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const PROJECTED_EVENT_TYPE: &str = "invoice-issued";
const SECOND_PROJECTED_EVENT_TYPE: &str = "invoice-adjusted";
const UNSELECTED_EVENT_TYPE: &str = "invoice-voided";

fn source_id() -> DeliverySourceId {
    DeliverySourceId::try_new("postgres-primary").expect("fixture source ID should be valid")
}

fn selection(
    id: &str,
    stream_filter: ProjectionStreamFilter,
    event_types: &[&str],
) -> ProjectionSelection {
    ProjectionSelection::try_new(
        ProjectionSelectionId::try_new(id).expect("fixture selection ID should be valid"),
        stream_filter,
        event_types
            .iter()
            .map(|event_type| {
                EventTypeName::try_new(*event_type).expect("fixture event type should be valid")
            })
            .collect(),
    )
    .expect("fixture selection should have distinct event types")
}

fn all_projected() -> ProjectionSelection {
    selection(
        "all-invoices-v1",
        ProjectionStreamFilter::All,
        &[PROJECTED_EVENT_TYPE],
    )
}

fn delivery_position(value: u64) -> DeliveryPosition {
    DeliveryPosition::new(NonZeroU64::new(value).expect("fixture position should be positive"))
}

async fn event_store_pool() -> projection_delivery::IsolatedTestDatabase {
    let _ = common::create_test_store;
    let pool = projection_delivery::create_isolated_test_pool().await;
    PostgresEventStore::from_pool(pool.clone_pool())
        .migrate()
        .await;
    pool
}

async fn migrated_source() -> (
    projection_delivery::IsolatedTestDatabase,
    PostgresProjectionSource,
) {
    let pool = event_store_pool().await;
    let source = PostgresProjectionSource::from_pool(pool.clone_pool(), source_id());
    source
        .migrate()
        .await
        .expect("projection source migration should succeed");
    (pool, source)
}

async fn insert_event<'e, E>(
    executor: E,
    event_id: Uuid,
    stream_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) where
    E: Executor<'e, Database = Postgres>,
{
    let _ = query(
        "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event_id)
    .bind(stream_id)
    .bind(event_type)
    .bind(payload)
    .bind(serde_json::json!({}))
    .execute(executor)
    .await
    .expect("fixture event insert should succeed");
}

async fn insert_event_with_raw_json(
    pool: &sqlx::Pool<Postgres>,
    event_id: Uuid,
    stream_id: &str,
    event_type: &str,
    payload: &str,
    metadata: &str,
) {
    let _ = query(
        "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata) \
         VALUES ($1, $2, $3, CAST($4 AS JSONB), CAST($5 AS JSONB))",
    )
    .bind(event_id)
    .bind(stream_id)
    .bind(event_type)
    .bind(payload)
    .bind(metadata)
    .execute(pool)
    .await
    .expect("fixture raw JSON event insert should succeed");
}

async fn bounded_cleanup(
    pool: &projection_delivery::IsolatedTestDatabase,
) -> Result<(), &'static str> {
    match timeout(
        DATABASE_OPERATION_TIMEOUT,
        AssertUnwindSafe(pool.cleanup()).catch_unwind(),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err("cleanup panicked"),
        Err(_) => Err("cleanup timed out"),
    }
}

// Break caught: defaulting, swapping, or omitting any persisted envelope field would give a
// projector an event that no longer identifies the exact source record it must apply.
#[tokio::test]
async fn delivered_envelopes_preserve_every_public_persisted_field() {
    let pool = timeout(
        DATABASE_OPERATION_TIMEOUT,
        projection_delivery::create_isolated_test_pool(),
    )
    .await
    .expect("isolated database allocation should remain bounded");
    let source = PostgresProjectionSource::from_pool(pool.clone_pool(), source_id());

    // Given a migrated source and two persisted events in the same stream.
    let setup = timeout(
        DATABASE_OPERATION_TIMEOUT,
        AssertUnwindSafe(async {
            PostgresEventStore::from_pool(pool.clone_pool())
                .migrate()
                .await;
            source
                .migrate()
                .await
                .expect("projection source migration should succeed");
        })
        .catch_unwind(),
    )
    .await;
    match setup {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => {
            let cleanup = bounded_cleanup(&pool).await;
            eprintln!("cleanup after setup panic: {cleanup:?}");
            resume_unwind(payload);
        }
        Err(_) => {
            let cleanup = bounded_cleanup(&pool).await;
            panic!("envelope contract setup timed out; cleanup: {cleanup:?}");
        }
    }

    let first_event_id =
        Uuid::parse_str("00000000-0000-7000-8000-000000000021").expect("fixture UUID should parse");
    let second_event_id =
        Uuid::parse_str("00000000-0000-7000-8000-000000000022").expect("fixture UUID should parse");
    let operation = timeout(
        DATABASE_OPERATION_TIMEOUT,
        AssertUnwindSafe(async {
            insert_event_with_raw_json(
                pool.pool(),
                first_event_id,
                "invoice::envelope-contract",
                PROJECTED_EVENT_TYPE,
                r#"{"amount_cents": 4100}"#,
                r#"{"trace_id": "trace-21"}"#,
            )
            .await;
            insert_event_with_raw_json(
                pool.pool(),
                second_event_id,
                "invoice::envelope-contract",
                SECOND_PROJECTED_EVENT_TYPE,
                r#"{"amount_cents": 9900}"#,
                r#"{"trace_id": "trace-22"}"#,
            )
            .await;

            // When the public source reads both events.
            let selected = selection(
                "envelope-contract-v1",
                ProjectionStreamFilter::All,
                &[PROJECTED_EVENT_TYPE, SECOND_PROJECTED_EVENT_TYPE],
            );
            let page = source
                .read_envelopes(
                    &selected,
                    None,
                    DeliveryUpperBound::Unbounded,
                    BatchSize::new(2),
                )
                .await
                .expect("persisted envelopes should be readable");

            // Then every persisted field exposed by the public envelope API is exact.
            assert_eq!(page.len(), 2);

            assert_eq!(page[0].source_id(), &source_id());
            assert_eq!(page[0].position(), delivery_position(1));
            assert_eq!(page[0].event_id(), PersistedEventId::new(first_event_id));
            assert_eq!(
                page[0].stream_id(),
                &StreamId::try_new("invoice::envelope-contract")
                    .expect("fixture stream ID should be valid")
            );
            assert_eq!(page[0].stream_version(), StreamVersion::new(1));
            assert_eq!(
                page[0].event_type(),
                &EventTypeName::try_new(PROJECTED_EVENT_TYPE)
                    .expect("fixture event type should be valid")
            );
            assert_eq!(page[0].payload().get(), r#"{"amount_cents": 4100}"#);
            assert_eq!(page[0].metadata().get(), r#"{"trace_id": "trace-21"}"#);

            assert_eq!(page[1].source_id(), &source_id());
            assert_eq!(page[1].position(), delivery_position(2));
            assert_eq!(page[1].event_id(), PersistedEventId::new(second_event_id));
            assert_eq!(
                page[1].stream_id(),
                &StreamId::try_new("invoice::envelope-contract")
                    .expect("fixture stream ID should be valid")
            );
            assert_eq!(page[1].stream_version(), StreamVersion::new(2));
            assert_eq!(
                page[1].event_type(),
                &EventTypeName::try_new(SECOND_PROJECTED_EVENT_TYPE)
                    .expect("fixture event type should be valid")
            );
            assert_eq!(page[1].payload().get(), r#"{"amount_cents": 9900}"#);
            assert_eq!(page[1].metadata().get(), r#"{"trace_id": "trace-22"}"#);
        })
        .catch_unwind(),
    )
    .await;

    let cleanup = bounded_cleanup(&pool).await;
    match operation {
        Ok(Ok(())) => cleanup.expect("test schema cleanup should succeed"),
        Ok(Err(payload)) => {
            eprintln!("cleanup after envelope contract panic: {cleanup:?}");
            resume_unwind(payload);
        }
        Err(_) => panic!("envelope contract body timed out; cleanup: {cleanup:?}"),
    }
}

// Break caught: treating an empty source as having a synthetic checkpoint would make a first
// batch look nonempty and could advance projection progress without a persisted event.
#[tokio::test]
async fn empty_source_has_no_watermark_and_returns_an_empty_page() {
    let (pool, source) = migrated_source().await;

    assert_eq!(
        source
            .high_watermark()
            .await
            .expect("empty source watermark should be readable"),
        None
    );
    assert!(
        source
            .read_envelopes(
                &all_projected(),
                None,
                DeliveryUpperBound::Unbounded,
                BatchSize::new(4),
            )
            .await
            .expect("empty source page should be readable")
            .is_empty()
    );

    pool.cleanup().await;
}

// Break caught: using a sequence/`nextval` or committing a frontier advance outside the append
// transaction would expose an uncommitted position, leave a rollback hole, or fail to reuse
// position one for the next committed event.
#[tokio::test]
async fn rolled_back_frontier_allocation_is_invisible_and_the_position_is_reused() {
    let (pool, source) = migrated_source().await;
    let mut transaction_a = pool.begin().await.expect("transaction A should begin");

    insert_event(
        &mut *transaction_a,
        Uuid::parse_str("00000000-0000-7000-8000-000000000010").expect("fixture UUID should parse"),
        "invoice::rollback::a",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"transaction": "a"}),
    )
    .await;

    assert_eq!(
        source
            .high_watermark()
            .await
            .expect("separate connection should read the committed watermark"),
        None
    );
    assert!(
        source
            .read_envelopes(
                &all_projected(),
                None,
                DeliveryUpperBound::Unbounded,
                BatchSize::new(2),
            )
            .await
            .expect("separate connection should not observe transaction A's envelope")
            .is_empty()
    );

    transaction_a
        .rollback()
        .await
        .expect("transaction A should roll back its frontier allocation");
    let committed_event_id =
        Uuid::parse_str("00000000-0000-7000-8000-000000000011").expect("fixture UUID should parse");
    insert_event(
        pool.pool(),
        committed_event_id,
        "invoice::rollback::b",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"transaction": "b"}),
    )
    .await;

    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(2),
        )
        .await
        .expect("committed replacement event should be readable");
    assert_eq!(
        page[0].event_id(),
        PersistedEventId::new(committed_event_id)
    );
    assert_eq!(page[0].position(), delivery_position(1));
    assert_eq!(
        source
            .high_watermark()
            .await
            .expect("committed watermark should be readable"),
        Some(delivery_position(1))
    );

    pool.cleanup().await;
}

// Break caught: making either cursor boundary inclusive or ignoring the explicit position-two
// upper bound would duplicate an already checkpointed event or admit position three into a
// finite batch frontier.
#[tokio::test]
async fn pages_are_strictly_after_the_cursor_and_inclusively_bounded_through_the_frontier() {
    let (pool, source) = migrated_source().await;
    let first_id = Uuid::now_v7();
    let second_id = Uuid::now_v7();
    let third_id = Uuid::now_v7();

    insert_event(
        pool.pool(),
        first_id,
        "invoice::bounded::one",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"number": 1}),
    )
    .await;
    insert_event(
        pool.pool(),
        second_id,
        "invoice::bounded::two",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"number": 2}),
    )
    .await;
    insert_event(
        pool.pool(),
        third_id,
        "invoice::bounded::three",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"number": 3}),
    )
    .await;

    let first_page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("first page should be readable");
    let second_page = source
        .read_envelopes(
            &all_projected(),
            Some(first_page[0].position()),
            DeliveryUpperBound::Inclusive(delivery_position(2)),
            BatchSize::new(4),
        )
        .await
        .expect("bounded second page should be readable");

    assert_eq!(first_page[0].position(), delivery_position(1));
    assert_eq!(first_page[0].event_id(), PersistedEventId::new(first_id));
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].position(), delivery_position(2));
    assert_eq!(second_page[0].event_id(), PersistedEventId::new(second_id));
    assert_ne!(second_page[0].event_id(), PersistedEventId::new(third_id));

    pool.cleanup().await;
}

// Break caught: returning only one batch even after callers resume would strand later selected
// events forever behind a valid checkpoint.
#[tokio::test]
async fn selected_events_larger_than_one_batch_are_completely_drainable() {
    let (pool, source) = migrated_source().await;
    let event_ids = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
    for (index, event_id) in event_ids.iter().enumerate() {
        insert_event(
            pool.pool(),
            *event_id,
            &format!("invoice::drain::{index}"),
            PROJECTED_EVENT_TYPE,
            serde_json::json!({"number": index}),
        )
        .await;
    }

    let first_page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(2),
        )
        .await
        .expect("first drain page should be readable");
    let second_page = source
        .read_envelopes(
            &all_projected(),
            Some(first_page[1].position()),
            DeliveryUpperBound::Unbounded,
            BatchSize::new(2),
        )
        .await
        .expect("second drain page should be readable");

    assert_eq!(
        first_page
            .iter()
            .map(|envelope| envelope.event_id())
            .collect::<Vec<_>>(),
        vec![
            PersistedEventId::new(event_ids[0]),
            PersistedEventId::new(event_ids[1]),
        ]
    );
    assert_eq!(
        second_page
            .iter()
            .map(|envelope| envelope.event_id())
            .collect::<Vec<_>>(),
        vec![PersistedEventId::new(event_ids[2])]
    );

    pool.cleanup().await;
}

// Break caught: applying stream or event-type predicates after LIMIT would let unselected rows
// consume a page slot and delay a selected event despite available batch capacity.
#[tokio::test]
async fn prefix_pattern_and_event_type_filters_apply_before_page_limit() {
    let (pool, source) = migrated_source().await;
    let ignored_type_id = Uuid::now_v7();
    let matching_prefix_id = Uuid::now_v7();
    let matching_pattern_id = Uuid::now_v7();
    let wrong_stream_id = Uuid::now_v7();

    insert_event(
        pool.pool(),
        ignored_type_id,
        "orders::north::ignored",
        UNSELECTED_EVENT_TYPE,
        serde_json::json!({"ignored": "type"}),
    )
    .await;
    insert_event(
        pool.pool(),
        matching_prefix_id,
        "orders::north::one",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"selected": "prefix"}),
    )
    .await;
    insert_event(
        pool.pool(),
        matching_pattern_id,
        "orders::south::two",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"selected": "pattern"}),
    )
    .await;
    insert_event(
        pool.pool(),
        wrong_stream_id,
        "payments::north::three",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"ignored": "stream"}),
    )
    .await;

    let prefix_selection = selection(
        "orders-prefix-v1",
        ProjectionStreamFilter::Prefix(
            StreamPrefix::try_new("orders::north::").expect("fixture prefix should be valid"),
        ),
        &[PROJECTED_EVENT_TYPE],
    );
    let pattern_selection = selection(
        "orders-pattern-v1",
        ProjectionStreamFilter::Pattern(
            StreamPattern::try_new("orders::south::*").expect("fixture pattern should be valid"),
        ),
        &[PROJECTED_EVENT_TYPE],
    );

    let prefix_page = source
        .read_envelopes(
            &prefix_selection,
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("prefix page should be readable");
    let pattern_page = source
        .read_envelopes(
            &pattern_selection,
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("pattern page should be readable");

    assert_eq!(
        prefix_page[0].event_id(),
        PersistedEventId::new(matching_prefix_id)
    );
    assert_eq!(
        pattern_page[0].event_id(),
        PersistedEventId::new(matching_pattern_id)
    );

    pool.cleanup().await;
}

// Break caught: using an unescaped SQL LIKE predicate makes StreamPrefix metacharacters select
// non-prefix streams, so unrelated events can consume a selected page before the literal match.
#[tokio::test]
async fn prefix_filter_treats_underscore_percent_and_backslash_as_literal_characters() {
    let (pool, source) = migrated_source().await;
    let underscore_wildcard_id = Uuid::now_v7();
    let percent_wildcard_id = Uuid::now_v7();
    let backslash_escape_id = Uuid::now_v7();
    let literal_prefix_id = Uuid::now_v7();

    insert_event(
        pool.pool(),
        underscore_wildcard_id,
        "invoiceX%\\underscore-wildcard",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "underscore wildcard only"}),
    )
    .await;
    insert_event(
        pool.pool(),
        percent_wildcard_id,
        "invoice_abc\\percent-wildcard",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "percent wildcard only"}),
    )
    .await;
    insert_event(
        pool.pool(),
        backslash_escape_id,
        "invoice_%xbackslash-escape",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "backslash escape only"}),
    )
    .await;
    insert_event(
        pool.pool(),
        literal_prefix_id,
        "invoice_%\\literal-prefix",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "literal prefix"}),
    )
    .await;

    let literal_selection = selection(
        "literal-prefix-v1",
        ProjectionStreamFilter::Prefix(
            StreamPrefix::try_new("invoice_%\\").expect("fixture prefix should be valid"),
        ),
        &[PROJECTED_EVENT_TYPE],
    );
    let page = source
        .read_envelopes(
            &literal_selection,
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("literal-prefix page should be readable");

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].event_id(), PersistedEventId::new(literal_prefix_id));

    pool.cleanup().await;
}

// Break caught: translating a leading `^` in a glob character class directly into a PostgreSQL
// regex class changes it from a literal glob member into regex negation and selects the wrong stream.
#[tokio::test]
async fn pattern_filter_preserves_literal_caret_character_class_semantics() {
    let (pool, source) = migrated_source().await;
    let regex_negation_only_id = Uuid::now_v7();
    let literal_caret_id = Uuid::now_v7();
    let pattern = StreamPattern::try_new("invoice::[^x]").expect("fixture pattern should be valid");

    assert!(pattern.matches("invoice::^"));
    assert!(!pattern.matches("invoice::a"));
    assert!(pattern.matches("invoice::x"));

    insert_event(
        pool.pool(),
        regex_negation_only_id,
        "invoice::a",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "regex negation only"}),
    )
    .await;
    insert_event(
        pool.pool(),
        literal_caret_id,
        "invoice::^",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"match": "literal caret"}),
    )
    .await;

    let pattern_selection = selection(
        "literal-caret-pattern-v1",
        ProjectionStreamFilter::Pattern(pattern),
        &[PROJECTED_EVENT_TYPE],
    );
    let page = source
        .read_envelopes(
            &pattern_selection,
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("literal-caret pattern page should be readable");

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].event_id(), PersistedEventId::new(literal_caret_id));

    pool.cleanup().await;
}

// Break caught: requiring a selected row at the global frontier would make a finite batch fail
// to certify catch-up whenever its final persisted event is intentionally unselected.
#[tokio::test]
async fn unselected_trailing_frontier_still_allows_selected_catch_up() {
    let (pool, source) = migrated_source().await;
    let selected_id = Uuid::now_v7();

    insert_event(
        pool.pool(),
        selected_id,
        "invoice::trailing::selected",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"selected": true}),
    )
    .await;
    insert_event(
        pool.pool(),
        Uuid::now_v7(),
        "invoice::trailing::unselected",
        UNSELECTED_EVENT_TYPE,
        serde_json::json!({"selected": false}),
    )
    .await;

    let high_watermark = source
        .high_watermark()
        .await
        .expect("watermark should be readable")
        .expect("inserted events should create a watermark");
    let selected_page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Inclusive(high_watermark),
            BatchSize::new(4),
        )
        .await
        .expect("selected page should be readable");
    let empty_tail = source
        .read_envelopes(
            &all_projected(),
            Some(selected_page[0].position()),
            DeliveryUpperBound::Inclusive(high_watermark),
            BatchSize::new(4),
        )
        .await
        .expect("selected tail should be readable");

    assert_eq!(
        selected_page[0].event_id(),
        PersistedEventId::new(selected_id)
    );
    assert!(empty_tail.is_empty());

    pool.cleanup().await;
}

// Break caught: treating a no-match filter as a storage failure would prevent a valid projector
// from completing an empty bounded catch-up cycle.
#[tokio::test]
async fn a_selection_with_no_matching_events_returns_an_empty_page() {
    let (pool, source) = migrated_source().await;
    insert_event(
        pool.pool(),
        Uuid::now_v7(),
        "invoice::no-match",
        UNSELECTED_EVENT_TYPE,
        serde_json::json!({"ignored": true}),
    )
    .await;

    assert!(
        source
            .read_envelopes(
                &all_projected(),
                None,
                DeliveryUpperBound::Unbounded,
                BatchSize::new(1),
            )
            .await
            .expect("no-match page should be readable")
            .is_empty()
    );

    pool.cleanup().await;
}

// Break caught: decoding or filter-mapping selected payloads in the source would silently drop a
// storage-valid envelope that a projection's application decoder must instead observe and stop on.
#[tokio::test]
async fn storage_valid_but_application_malformed_payload_remains_visible_as_raw_json() {
    let (pool, source) = migrated_source().await;
    let event_id = Uuid::now_v7();
    insert_event(
        pool.pool(),
        event_id,
        "invoice::malformed",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"missing_expected_invoice_fields": true}),
    )
    .await;

    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("raw envelope should be readable");

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].event_id(), PersistedEventId::new(event_id));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(page[0].payload().get())
            .expect("payload should remain structurally valid JSON"),
        serde_json::json!({"missing_expected_invoice_fields": true})
    );

    pool.cleanup().await;
}

// Break caught: decoding JSONB through serde_json::Value rounds values outside its default
// integer representation before the source gives the selected envelope to application code.
#[tokio::test]
async fn source_preserves_precise_jsonb_numbers_in_payload_and_metadata() {
    let (pool, source) = migrated_source().await;
    let event_id = Uuid::now_v7();
    insert_event_with_raw_json(
        pool.pool(),
        event_id,
        "invoice::precise-json",
        PROJECTED_EVENT_TYPE,
        r#"{"payload_number": 18446744073709551617}"#,
        r#"{"metadata_number": 18446744073709551617}"#,
    )
    .await;

    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("precise raw envelope should be readable");

    assert_eq!(page[0].event_id(), PersistedEventId::new(event_id));
    assert_eq!(
        page[0].payload().get(),
        r#"{"payload_number": 18446744073709551617}"#
    );
    assert_eq!(
        page[0].metadata().get(),
        r#"{"metadata_number": 18446744073709551617}"#
    );

    pool.cleanup().await;
}

// Break caught: deriving a fresh or database-incidental identity on each construction would make
// one physical source appear as distinct durable progress namespaces across restarts.
#[tokio::test]
async fn source_exposes_its_configured_stable_identity() {
    let (pool, source) = migrated_source().await;

    assert_eq!(source.source_id(), &source_id());

    pool.cleanup().await;
}

// Break caught: backfilling in insertion or UUID order rather than the documented
// `(stream_id, stream_version, event_id)` order would make historical replay nondeterministic.
#[tokio::test]
async fn migration_backfills_events_committed_before_the_projection_source_existed() {
    let pool = event_store_pool().await;
    let account_a_version_one =
        Uuid::parse_str("00000000-0000-7000-8000-000000000003").expect("fixture UUID should parse");
    let account_a_version_two =
        Uuid::parse_str("00000000-0000-7000-8000-000000000001").expect("fixture UUID should parse");
    let account_b_version_one =
        Uuid::parse_str("00000000-0000-7000-8000-000000000002").expect("fixture UUID should parse");
    insert_event(
        pool.pool(),
        account_b_version_one,
        "account::b",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"stream": "b", "version": 1}),
    )
    .await;
    insert_event(
        pool.pool(),
        account_a_version_one,
        "account::a",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"stream": "a", "version": 1}),
    )
    .await;
    insert_event(
        pool.pool(),
        account_a_version_two,
        "account::a",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"stream": "a", "version": 2}),
    )
    .await;

    let source = PostgresProjectionSource::from_pool(pool.clone_pool(), source_id());
    source
        .migrate()
        .await
        .expect("projection source migration should backfill history");
    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(4),
        )
        .await
        .expect("backfilled event should be readable");

    assert_eq!(
        page.iter()
            .map(|envelope| envelope.event_id())
            .collect::<Vec<_>>(),
        vec![
            PersistedEventId::new(account_a_version_one),
            PersistedEventId::new(account_a_version_two),
            PersistedEventId::new(account_b_version_one),
        ]
    );
    assert_eq!(
        page.iter()
            .map(|envelope| envelope.position())
            .collect::<Vec<_>>(),
        vec![
            delivery_position(1),
            delivery_position(2),
            delivery_position(3),
        ]
    );
    assert_eq!(
        source
            .high_watermark()
            .await
            .expect("backfilled watermark should be readable"),
        Some(delivery_position(3))
    );

    pool.cleanup().await;
}

// Break caught: limiting delivery mappings to new adapter writes would omit events inserted by a
// still-running legacy client after the projection migration has been applied.
#[tokio::test]
async fn direct_legacy_client_writes_are_delivered_after_source_migration() {
    let (pool, source) = migrated_source().await;
    let event_id = Uuid::now_v7();

    insert_event(
        pool.pool(),
        event_id,
        "invoice::legacy-client",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"legacy": true}),
    )
    .await;

    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("legacy client event should be readable");

    assert_eq!(page[0].event_id(), PersistedEventId::new(event_id));

    pool.cleanup().await;
}

// Break caught: a trigger function that resolves delivery tables through the legacy writer's
// search path fails after source migration owns those tables in a different schema.
#[tokio::test]
async fn legacy_writer_with_a_different_search_path_is_delivered_after_source_migration() {
    let database = projection_delivery::create_split_search_path_test_database().await;
    PostgresEventStore::from_pool(database.legacy_pool().clone())
        .migrate()
        .await;
    let source = PostgresProjectionSource::from_pool(database.source_pool(), source_id());
    source
        .migrate()
        .await
        .expect("source migration should install delivery tables in the source schema");
    let event_id = Uuid::now_v7();

    insert_event(
        database.legacy_pool(),
        event_id,
        "invoice::legacy-search-path",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"writer": "legacy search path"}),
    )
    .await;

    let page = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(1),
        )
        .await
        .expect("legacy writer event should be delivered through the source search path");

    assert_eq!(page[0].event_id(), PersistedEventId::new(event_id));

    database.cleanup().await;
}

// Break caught: recording the source migration in SQLx's shared ledger would make a 2.0.1 event
// store migrator reject an otherwise compatible database due to an unknown migration version.
#[tokio::test]
async fn projection_migration_keeps_the_legacy_event_store_migrator_compatible() {
    let (pool, _source) = migrated_source().await;

    PostgresEventStore::from_pool(pool.clone_pool())
        .migrate()
        .await;
    pool.cleanup().await;
}

// Break caught: resuming with a UUID-derived cursor would skip a preselected lower UUID inserted
// and committed after the higher UUID event has already been consumed by the projector.
#[tokio::test]
async fn lower_uuid_committed_after_a_consumed_higher_uuid_receives_a_later_delivery_position() {
    let (pool, source) = migrated_source().await;
    let one = BatchSize::new(1);
    let lower_uuid_event_id = Uuid::parse_str("00000000-0000-7000-8000-000000000001")
        .expect("fixture lower UUID should parse");
    let higher_uuid_event_id = Uuid::parse_str("ffffffff-ffff-7fff-bfff-ffffffffffff")
        .expect("fixture higher UUID should parse");

    insert_event(
        pool.pool(),
        higher_uuid_event_id,
        "invoice::uuid-order::higher",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"uuid": "higher"}),
    )
    .await;

    let first_page = source
        .read_envelopes(&all_projected(), None, DeliveryUpperBound::Unbounded, one)
        .await
        .expect("first UUID page should be readable");
    insert_event(
        pool.pool(),
        lower_uuid_event_id,
        "invoice::uuid-order::lower",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"uuid": "lower"}),
    )
    .await;
    let second_page = source
        .read_envelopes(
            &all_projected(),
            Some(first_page[0].position()),
            DeliveryUpperBound::Unbounded,
            one,
        )
        .await
        .expect("second UUID page should be readable");

    assert_eq!(
        first_page[0].event_id(),
        PersistedEventId::new(higher_uuid_event_id)
    );
    assert_eq!(
        second_page[0].event_id(),
        PersistedEventId::new(lower_uuid_event_id)
    );
    assert!(second_page[0].position() > first_page[0].position());

    pool.cleanup().await;
}

async fn waits_on_transaction_a_frontier(
    observer: &projection_delivery::IsolatedTestDatabase,
    transaction_a_backend_pid: i32,
    transaction_b_backend_pid: i32,
) -> Result<bool, sqlx::Error> {
    let observation = timeout(Duration::from_secs(2), async {
        loop {
            let waiting_on_transaction_a: bool = query_scalar(
                "SELECT EXISTS (\
                     SELECT 1 \
                     FROM pg_stat_activity AS waiter \
                     JOIN pg_locks AS waiting_lock ON waiting_lock.pid = waiter.pid \
                     JOIN pg_locks AS held_lock \
                       ON held_lock.locktype = waiting_lock.locktype \
                      AND held_lock.transactionid = waiting_lock.transactionid \
                     WHERE waiter.pid = $1 \
                       AND waiter.wait_event_type = 'Lock' \
                       AND waiting_lock.granted = FALSE \
                       AND waiting_lock.locktype = 'transactionid' \
                       AND held_lock.pid = $2 \
                       AND held_lock.granted = TRUE\
                 )",
            )
            .bind(transaction_b_backend_pid)
            .bind(transaction_a_backend_pid)
            .fetch_one(&**observer)
            .await?;

            if waiting_on_transaction_a {
                return Ok(());
            }

            sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    match observation {
        Ok(Ok(())) => Ok(true),
        Ok(Err(error)) => Err(error),
        Err(_) => Ok(false),
    }
}

async fn abort_and_join_transaction_b(transaction_b: &mut tokio::task::JoinHandle<()>) {
    transaction_b.abort();
    let _ = timeout(Duration::from_secs(2), transaction_b).await;
}

// Break caught: allocating delivery positions without transactionally serializing the frontier
// lets concurrent commits create a hole that a resumable projector can never read later.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_transactions_deliver_every_committed_event_without_waiting_for_blocked_insert()
{
    let (pool, source) = migrated_source().await;
    let transaction_a_event_id = Uuid::now_v7();
    let transaction_b_event_id = Uuid::now_v7();
    let mut transaction_a = pool.begin().await.expect("transaction A should begin");

    // Transaction A allocates the trigger frontier and deliberately keeps that lock uncommitted.
    insert_event(
        &mut *transaction_a,
        transaction_a_event_id,
        "invoice::concurrency::a",
        PROJECTED_EVENT_TYPE,
        serde_json::json!({"transaction": "a"}),
    )
    .await;
    let transaction_a_backend_pid: i32 = query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction_a)
        .await
        .expect("transaction A backend PID should be readable");

    let (transaction_b_started, transaction_b_started_receiver) = oneshot::channel();
    let transaction_b_pool = pool.clone();
    let mut transaction_b = tokio::spawn(async move {
        let mut transaction = transaction_b_pool
            .begin()
            .await
            .expect("transaction B should begin");
        let transaction_b_backend_pid: i32 = query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await
            .expect("transaction B backend PID should be readable");
        transaction_b_started
            .send(transaction_b_backend_pid)
            .expect("test should still observe transaction B start");
        insert_event(
            &mut *transaction,
            transaction_b_event_id,
            "invoice::concurrency::b",
            PROJECTED_EVENT_TYPE,
            serde_json::json!({"transaction": "b"}),
        )
        .await;
        transaction
            .commit()
            .await
            .expect("transaction B should commit once A releases the frontier");
    });

    let transaction_b_backend_pid =
        match timeout(Duration::from_secs(2), transaction_b_started_receiver).await {
            Ok(Ok(pid)) => pid,
            Ok(Err(_)) | Err(_) => {
                let _ = transaction_a.rollback().await;
                abort_and_join_transaction_b(&mut transaction_b).await;
                pool.cleanup().await;
                panic!(
                    "transaction B did not expose its backend PID within the bounded start window"
                );
            }
        };
    let b_is_waiting = match waits_on_transaction_a_frontier(
        &pool,
        transaction_a_backend_pid,
        transaction_b_backend_pid,
    )
    .await
    {
        Ok(waiting) => waiting,
        Err(error) => {
            let _ = transaction_a.rollback().await;
            abort_and_join_transaction_b(&mut transaction_b).await;
            pool.cleanup().await;
            panic!("observer SQL error while proving B's frontier lock wait: {error}");
        }
    };
    if !b_is_waiting {
        let _ = transaction_a.rollback().await;
        abort_and_join_transaction_b(&mut transaction_b).await;
        pool.cleanup().await;
        panic!(
            "transaction B never reached a PostgreSQL transaction-lock wait attributable to A's frontier"
        );
    }

    transaction_a
        .commit()
        .await
        .expect("transaction A should release the delivery frontier");
    match timeout(Duration::from_secs(2), &mut transaction_b).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            pool.cleanup().await;
            panic!("transaction B task failed after A released the frontier: {error}");
        }
        Err(_) => {
            abort_and_join_transaction_b(&mut transaction_b).await;
            pool.cleanup().await;
            panic!("transaction B did not finish within the bounded completion window");
        }
    }

    let delivered = source
        .read_envelopes(
            &all_projected(),
            None,
            DeliveryUpperBound::Unbounded,
            BatchSize::new(4),
        )
        .await
        .expect("committed concurrent events should be readable");
    let delivered_ids = delivered
        .iter()
        .map(|envelope| envelope.event_id())
        .collect::<HashSet<_>>();

    assert_eq!(delivered.len(), 2, "no committed event may be duplicated");
    assert_eq!(
        delivered_ids,
        HashSet::from([
            PersistedEventId::new(transaction_a_event_id),
            PersistedEventId::new(transaction_b_event_id),
        ])
    );
    assert_eq!(
        delivered
            .iter()
            .map(|envelope| envelope.position())
            .collect::<Vec<_>>(),
        vec![delivery_position(1), delivery_position(2)]
    );

    pool.cleanup().await;
}
