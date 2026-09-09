//! Public contract tests for transactional PostgreSQL projections.

use std::env;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::time::Duration;

use eventcore_postgres::{
    NoopAfterCommit, PostgresProjectionConfig, PostgresProjectionMode, PostgresProjectionSource,
    PostgresProjectionSourceError, PostgresProjectionStore, PostgresProjector,
    ProjectionConfigurationError, ProjectionRetryPolicy, ProjectionRunOutcome,
    TransactionalProjectionError, run_transactional_projection,
};
use eventcore_testing::{
    ProjectionApplicationBehavior, ProjectionHookLogEntry, ProjectionProgressObservation,
    ProjectionRunOutcome as ContractRunOutcome, TransactionalProjectionFixture,
    transactional_projection_contract,
};
use eventcore_types::{
    BatchSize, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, Event, EventStore,
    EventTypeName, ProjectionSelection, ProjectionSelectionId, ProjectionSource,
    ProjectionStreamFilter, ProjectorName, StreamId, StreamVersion, StreamWrites,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Transaction, postgres::PgPoolOptions, query, query_scalar};
use thiserror::Error;
use tokio::sync::Barrier;
use tokio::time::timeout;
use uuid::Uuid;

const RUN_TIMEOUT: Duration = Duration::from_secs(2);

struct IsolatedTestDatabase {
    pool: Pool<Postgres>,
    schema: String,
    connection_string: String,
}

impl IsolatedTestDatabase {
    fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    fn clone_pool(&self) -> Pool<Postgres> {
        self.pool.clone()
    }

    async fn cleanup(self) -> Result<(), sqlx::Error> {
        self.pool.close().await;
        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await?;
        let _ = query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&cleanup_pool)
            .await?;
        cleanup_pool.close().await;
        Ok(())
    }
}

async fn isolated_database() -> Result<IsolatedTestDatabase, sqlx::Error> {
    let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_string());
    let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await?;
    let schema = format!("eventcore_transactional_test_{}", Uuid::now_v7().simple());
    let _ = query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;
    let schema_for_pool = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |connection, _metadata| {
            let schema = schema_for_pool.clone();
            Box::pin(async move {
                let _ = query("SELECT set_config('search_path', $1, false)")
                    .bind(schema)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&connection_string)
        .await?;
    Ok(IsolatedTestDatabase {
        pool,
        schema,
        connection_string,
    })
}

fn selection() -> ProjectionSelection {
    ProjectionSelection::try_new(
        ProjectionSelectionId::try_new("invoice-effects-v1")
            .expect("fixture selection identity should be valid"),
        ProjectionStreamFilter::All,
        vec![EventTypeName::try_new("invoice-issued").expect("fixture event type should be valid")],
    )
    .expect("fixture selection should be valid")
}

fn source_id() -> DeliverySourceId {
    DeliverySourceId::try_new("postgres-primary").expect("fixture source ID should be valid")
}

#[derive(Clone, Deserialize, Serialize)]
struct InvoiceIssued {
    stream_id: StreamId,
}

impl Event for InvoiceIssued {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "invoice-issued"
    }
}

#[derive(Debug, Error)]
enum FixtureError {
    #[error("event store operation failed")]
    EventStore(#[from] eventcore_types::EventStoreError),
    #[error("postgres operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("projection source operation failed")]
    Source(#[from] PostgresProjectionSourceError),
    #[error("transactional projection run failed")]
    Runner(#[from] TransactionalProjectionError),
    #[error("transactional projection run exceeded its fixture timeout")]
    TimedOut,
}

struct IncrementProjector {
    name: ProjectorName,
    behavior: ProjectionApplicationBehavior,
    apply_barrier: Option<Arc<Barrier>>,
}

impl PostgresProjector for IncrementProjector {
    type Event = InvoiceIssued;
    type Error = sqlx::Error;
    type AfterCommit = NoopAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    async fn apply<'a, 'c>(
        &'a mut self,
        _event: &'a Self::Event,
        _position: DeliveryPosition,
        transaction: &'a mut Transaction<'c, Postgres>,
    ) -> Result<Self::AfterCommit, Self::Error>
    where
        'c: 'a,
    {
        let _ = self.behavior;
        if let Some(barrier) = &self.apply_barrier {
            let _ = barrier.wait().await;
        }
        let _ = query("UPDATE invoice_projection_effect SET total = total + 1")
            .execute(&mut **transaction)
            .await?;
        Ok(NoopAfterCommit)
    }
}

struct PostgresAtomicFixture {
    database: IsolatedTestDatabase,
    event_store: eventcore_postgres::PostgresEventStore,
    source: PostgresProjectionSource,
    store: PostgresProjectionStore,
    source_id: DeliverySourceId,
    selection: ProjectionSelection,
    projector_name: ProjectorName,
    behavior: ProjectionApplicationBehavior,
    apply_barrier: Option<Arc<Barrier>>,
}

impl PostgresAtomicFixture {
    async fn new() -> Result<Self, FixtureError> {
        let database = isolated_database().await?;
        let event_store = eventcore_postgres::PostgresEventStore::from_pool(database.clone_pool());
        event_store.migrate().await;
        let source_id = source_id();
        let source = PostgresProjectionSource::from_pool(database.clone_pool(), source_id.clone());
        source.migrate().await?;
        let store = PostgresProjectionStore::from_pool(database.clone_pool());
        store.migrate().await?;
        let _ = query("CREATE TABLE invoice_projection_effect (total BIGINT NOT NULL)")
            .execute(database.pool())
            .await?;
        let _ = query("INSERT INTO invoice_projection_effect (total) VALUES (0)")
            .execute(database.pool())
            .await?;
        let projector_name = ProjectorName::try_new(format!("invoice-effect-{}", database.schema))
            .expect("schema-derived fixture projector name should be valid");
        Ok(Self {
            database,
            event_store,
            source,
            store,
            source_id,
            selection: selection(),
            projector_name,
            behavior: ProjectionApplicationBehavior::Apply,
            apply_barrier: None,
        })
    }

    async fn cleanup(self) -> Result<(), FixtureError> {
        self.database.cleanup().await?;
        Ok(())
    }

    fn with_apply_barrier(&mut self, barrier: Arc<Barrier>) {
        self.apply_barrier = Some(barrier);
    }

    fn clear_apply_barrier(&mut self) {
        self.apply_barrier = None;
    }

    async fn run_with_timeout(
        &self,
        config: PostgresProjectionConfig,
    ) -> Result<ProjectionRunOutcome, FixtureError> {
        let source = self.source.clone();
        let store = self.store.clone();
        let projector = IncrementProjector {
            name: self.projector_name.clone(),
            behavior: self.behavior,
            apply_barrier: self.apply_barrier.clone(),
        };
        match timeout(
            RUN_TIMEOUT,
            run_transactional_projection(projector, &source, &store, config),
        )
        .await
        {
            Ok(result) => Ok(result?),
            // Dropping the timed-out future also drops its transaction and leader connection;
            // both are configured for PostgreSQL rollback/close cleanup.
            Err(_) => Err(FixtureError::TimedOut),
        }
    }
}

impl TransactionalProjectionFixture for PostgresAtomicFixture {
    type Error = FixtureError;

    async fn append_values(
        &mut self,
        values: &[serde_json::Value],
    ) -> Result<Vec<DeliveryPosition>, Self::Error> {
        let stream_id = StreamId::try_new(format!("invoice::{}", Uuid::now_v7()))
            .expect("fixture stream ID should be valid");
        let mut writes =
            StreamWrites::new().register_stream(stream_id.clone(), StreamVersion::new(0))?;
        for value in values {
            assert!(
                value.is_object(),
                "fixture values must use object event payloads"
            );
            writes = writes.append(InvoiceIssued {
                stream_id: stream_id.clone(),
            })?;
        }
        let _ = self.event_store.append_events(writes).await?;
        let through = self
            .source
            .high_watermark()
            .await?
            .expect("public append should advance the public delivery frontier");
        let envelopes = self
            .source
            .read_envelopes(
                &self.selection,
                None,
                DeliveryUpperBound::Inclusive(through),
                BatchSize::new(values.len()),
            )
            .await?;
        for envelope in &envelopes {
            let _: InvoiceIssued = serde_json::from_str(envelope.payload().get()).expect(
                "EventCore public append payload must decode into the public projector event",
            );
        }
        Ok(envelopes
            .iter()
            .map(|envelope| envelope.position())
            .collect())
    }

    async fn append_malformed_input(&mut self, input: &str) -> Result<(), Self::Error> {
        let _ = query(
            "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata) \
             VALUES ($1, $2, $3, CAST($4 AS JSONB), $5)",
        )
        .bind(Uuid::now_v7())
        .bind(format!("invoice::malformed::{}", Uuid::now_v7()))
        .bind("invoice-issued")
        .bind(input)
        .bind(serde_json::json!({}))
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    fn select_application_behavior(&mut self, behavior: ProjectionApplicationBehavior) {
        self.behavior = behavior;
    }

    async fn run_batch(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        Ok(convert_outcome(
            self.run_with_timeout(PostgresProjectionConfig::new(self.selection.clone()))
                .await?,
        ))
    }

    async fn run_continuous(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        Ok(convert_outcome(
            self.run_with_timeout(
                PostgresProjectionConfig::new(self.selection.clone()).continuous(cancellation),
            )
            .await?,
        ))
    }

    async fn inject_progress_failure(&mut self) -> Result<(), Self::Error> {
        let _ = query(
            "CREATE OR REPLACE FUNCTION fixture_fail_projection_progress() RETURNS trigger \
             LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture progress failure'; END; $$; \
             CREATE TRIGGER fixture_fail_projection_progress BEFORE INSERT OR UPDATE ON \
             eventcore_projection_progress FOR EACH ROW EXECUTE FUNCTION fixture_fail_projection_progress();",
        )
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    async fn inject_connection_loss(&mut self) -> Result<(), Self::Error> {
        let _ = query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE false")
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    async fn effect_count(&self) -> Result<u64, Self::Error> {
        let count = query_scalar::<_, i64>("SELECT total FROM invoice_projection_effect")
            .fetch_one(self.database.pool())
            .await?;
        Ok(u64::try_from(count).expect("fixture effect count should be nonnegative"))
    }

    async fn progress(&self) -> Result<Option<ProjectionProgressObservation>, Self::Error> {
        let progress = self.store.progress(&self.projector_name).await?;
        Ok(progress.map(|progress| ProjectionProgressObservation {
            source_id: progress.source_id().clone(),
            selection_id: progress.selection_id().clone(),
            position: progress.position(),
        }))
    }

    async fn hook_log(&self) -> Result<Vec<ProjectionHookLogEntry>, Self::Error> {
        Ok(Vec::new())
    }

    async fn start_leadership_attempt(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn lose_leadership(&mut self) -> Result<(), Self::Error> {
        self.inject_connection_loss().await
    }

    async fn reset(&mut self) -> Result<(), Self::Error> {
        let _ = query("DELETE FROM invoice_projection_effect; INSERT INTO invoice_projection_effect (total) VALUES (0)")
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    fn selection_id(&self) -> &ProjectionSelectionId {
        self.selection.id()
    }
}

fn convert_outcome(outcome: ProjectionRunOutcome) -> ContractRunOutcome {
    match outcome {
        ProjectionRunOutcome::CaughtUp {
            processed,
            skipped,
            through,
        } => ContractRunOutcome::CaughtUp {
            processed,
            skipped,
            through,
        },
        ProjectionRunOutcome::Stopped {
            position,
            processed,
            skipped,
        } => ContractRunOutcome::Stopped {
            position,
            processed,
            skipped,
        },
        ProjectionRunOutcome::Cancelled { processed, skipped } => {
            ContractRunOutcome::Cancelled { processed, skipped }
        }
    }
}

// Break caught: changing documented defaults or the public mode/retry builders would make
// projection polling, batching, or backoff behavior unexpectedly incompatible.
#[test]
fn transactional_projection_config_validates_and_preserves_every_builder_value() {
    let defaults = PostgresProjectionConfig::new(selection());
    assert_eq!(usize::from(defaults.batch_size()), 100);
    assert!(matches!(defaults.mode(), PostgresProjectionMode::Batch));
    assert_eq!(defaults.retry_policy().max_retries(), 0);
    assert_eq!(
        defaults.retry_policy().initial_delay(),
        Duration::from_millis(100)
    );
    assert_eq!(defaults.retry_policy().multiplier(), 2.0);
    assert_eq!(
        defaults.retry_policy().maximum_delay(),
        Duration::from_secs(30)
    );
    assert_eq!(defaults.continuous_poll_interval(), Duration::from_secs(1));

    let retry =
        ProjectionRetryPolicy::new(3, Duration::from_millis(7), 3.0, Duration::from_secs(9))
            .expect("valid retry policy should construct");
    let cancellation = tokio_util::sync::CancellationToken::new();
    let config = PostgresProjectionConfig::new(selection())
        .with_batch_size(BatchSize::new(23))
        .with_retry_policy(retry.clone())
        .with_continuous_poll_interval(Duration::from_millis(11))
        .expect("positive poll interval should construct")
        .continuous(cancellation);

    assert_eq!(usize::from(config.batch_size()), 23);
    assert_eq!(config.selection(), &selection());
    assert_eq!(config.retry_policy(), &retry);
    assert_eq!(config.retry_policy().max_retries(), 3);
    assert_eq!(
        config.retry_policy().initial_delay(),
        Duration::from_millis(7)
    );
    assert_eq!(config.retry_policy().multiplier(), 3.0);
    assert_eq!(
        config.retry_policy().maximum_delay(),
        Duration::from_secs(9)
    );
    assert_eq!(config.continuous_poll_interval(), Duration::from_millis(11));
    assert!(matches!(
        config.mode(),
        PostgresProjectionMode::Continuous(_)
    ));
    assert!(matches!(
        PostgresProjectionConfig::new(selection()).with_continuous_poll_interval(Duration::ZERO),
        Err(ProjectionConfigurationError::ZeroContinuousPollInterval)
    ));
    assert_eq!(
        ProjectionRetryPolicy::new(0, Duration::ZERO, f64::INFINITY, Duration::ZERO),
        Err(ProjectionConfigurationError::InvalidRetryMultiplier),
    );
    assert_eq!(
        ProjectionRetryPolicy::new(0, Duration::ZERO, 0.5, Duration::ZERO),
        Err(ProjectionConfigurationError::InvalidRetryMultiplier),
    );
}

// Break caught: changing the component ledger identity can make destination migration state
// collide with another projection component or cause this migration to run unexpectedly.
#[tokio::test]
async fn destination_migration_records_its_exact_component_ledger_row() {
    let database = isolated_database()
        .await
        .expect("isolated database should be available");
    let store = PostgresProjectionStore::from_pool(database.clone_pool());
    store
        .migrate()
        .await
        .expect("destination migration should succeed");
    assert_eq!(
        query_scalar::<_, i64>(
            "SELECT version FROM eventcore_projection_schema_versions WHERE component = $1",
        )
        .bind("projection-destination")
        .fetch_one(database.pool())
        .await
        .expect("destination ledger row should be observable"),
        2,
    );
    database
        .cleanup()
        .await
        .expect("test schema cleanup should succeed");
}

// Break caught: reporting the global frontier as stopped without checking the selected page
// would block a projection whose source contains only unselected events.
#[tokio::test]
async fn batch_catches_up_when_the_frontier_contains_only_unselected_events() {
    let mut fixture = PostgresAtomicFixture::new()
        .await
        .expect("fixture should initialize");
    let stream_id = StreamId::try_new("invoice::unselected").expect("valid stream ID");
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("stream should register")
        .append(UnselectedEvent { stream_id })
        .expect("event should append to writes");
    let _ = fixture
        .event_store
        .append_events(writes)
        .await
        .expect("public append should work");
    let through = fixture
        .source
        .high_watermark()
        .await
        .expect("source should have a frontier");
    assert_eq!(
        fixture
            .run_batch()
            .await
            .expect("batch run should complete"),
        ContractRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through,
        },
    );
    fixture
        .cleanup()
        .await
        .expect("test schema cleanup should succeed");
}

#[derive(Clone, Deserialize, Serialize)]
struct UnselectedEvent {
    stream_id: StreamId,
}

impl Event for UnselectedEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "invoice-voided"
    }
}

// Break caught: committing only the application effect or only durable progress would either
// redeliver a non-idempotent increment or permanently lose an event after restart.
#[tokio::test]
async fn effect_and_progress_commit_atomically() {
    let mut fixture = PostgresAtomicFixture::new()
        .await
        .expect("fixture should initialize");
    let result = AssertUnwindSafe(transactional_projection_contract(&mut fixture))
        .catch_unwind()
        .await;
    let cleanup = fixture.cleanup().await;
    cleanup.expect("test schema cleanup should succeed even after an expected assertion failure");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("atomicity contract setup must not fail: {error}"),
        Err(payload) => resume_unwind(payload),
    }
}

// Break caught: deriving fixture projector names from a constant lets independent schemas collide
// on PostgreSQL's database-wide advisory-lock namespace under parallel nextest execution.
#[tokio::test]
async fn independent_schema_fixtures_acquire_distinct_leadership_and_retain_their_identity() {
    let (left, right) = tokio::join!(PostgresAtomicFixture::new(), PostgresAtomicFixture::new());
    let mut left = left.expect("left fixture should initialize");
    let mut right = right.expect("right fixture should initialize");
    let left_name = left.projector_name.clone();
    let right_name = right.projector_name.clone();
    assert_ne!(
        left_name, right_name,
        "fixture identities must be schema-specific"
    );

    let _ = left
        .append_values(&[serde_json::json!({})])
        .await
        .expect("left fixture event should append");
    let _ = right
        .append_values(&[serde_json::json!({})])
        .await
        .expect("right fixture event should append");
    let barrier = Arc::new(Barrier::new(2));
    left.with_apply_barrier(barrier.clone());
    right.with_apply_barrier(barrier);

    let (left_run, right_run) = tokio::join!(left.run_batch(), right.run_batch());
    assert!(matches!(
        left_run.expect("left runner should acquire leadership"),
        ContractRunOutcome::CaughtUp { processed: 1, .. }
    ),);
    assert!(matches!(
        right_run.expect("right runner should acquire leadership"),
        ContractRunOutcome::CaughtUp { processed: 1, .. }
    ),);

    left.clear_apply_barrier();
    assert!(matches!(
        left.run_batch().await.expect("repeat run should succeed"),
        ContractRunOutcome::CaughtUp { processed: 0, .. }
    ),);
    assert_eq!(
        left.projector_name, left_name,
        "fixture identity must persist across runs"
    );
    assert_eq!(
        right.projector_name, right_name,
        "fixture identity must persist across runs"
    );

    let (left_cleanup, right_cleanup) = tokio::join!(left.cleanup(), right.cleanup());
    left_cleanup.expect("left fixture cleanup should succeed");
    right_cleanup.expect("right fixture cleanup should succeed");
}
