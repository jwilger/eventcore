//! Public reset-and-replay contracts for transactional PostgreSQL projections.

use std::env;
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use eventcore_postgres::{
    NoopAfterCommit, PostgresCheckpointStore, PostgresProjectionConfig, PostgresProjectionReset,
    PostgresProjectionSource, PostgresProjectionStore, PostgresProjector,
    ProjectionResetAndReplayError, ProjectionResetError, ProjectionRunOutcome,
    TransactionalProjectionError, reset_and_replay_transactional_projection,
    reset_transactional_projection, run_transactional_projection,
};
use eventcore_testing::{
    ProjectionFailureObservation, ProjectionProgressObservation, ProjectionResetAttemptObservation,
    ProjectionResetBehavior, ProjectionResetFailureObservation,
    ProjectionResetReplayLeadershipObservation, ProjectionResetStateObservation,
    ProjectionRunOutcome as ContractRunOutcome, TransactionalProjectionResetFixture,
    legacy_checkpoint_reset_replay_contract, reset_and_replay_reconstructs_model_contract,
    reset_and_replay_retains_leadership_contract, reset_busy_contract,
    reset_callback_failure_rolls_back_contract, reset_progress_failure_rolls_back_contract,
    reset_selection_identity_validation_contract, reset_source_identity_validation_contract,
    successful_reset_is_atomic_contract,
};
use eventcore_types::{
    BatchSize, CheckpointStore, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, Event,
    EventStore, EventTypeName, ProjectionSelection, ProjectionSelectionId, ProjectionSource,
    ProjectionStreamFilter, ProjectorName, StreamId, StreamPosition, StreamVersion, StreamWrites,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Row, Transaction, postgres::PgPoolOptions, query, query_scalar};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

const RUN_TIMEOUT: Duration = Duration::from_secs(3);

// Break caught: omitting any public reset operation, callback trait, or typed error makes the
// coordinated recovery API unavailable to downstream applications.
#[test]
fn coordinated_reset_api_is_public() {
    fn reset_trait<R: PostgresProjectionReset>() {}
    let _ = reset_trait::<ApiOnlyReset>;
    let _ = reset_transactional_projection::<ApiOnlyReset>;
    let _: Option<ProjectionResetError> = None;
    let _: Option<ProjectionResetAndReplayError> = None;
}

struct ApiOnlyReset;

impl PostgresProjectionReset for ApiOnlyReset {
    type Error = std::convert::Infallible;

    async fn reset<'a, 'c>(
        &'a mut self,
        _tx: &'a mut Transaction<'c, Postgres>,
    ) -> Result<(), Self::Error>
    where
        'c: 'a,
    {
        Ok(())
    }
}

struct IsolatedTestDatabase {
    pool: Pool<Postgres>,
    schema: String,
    connection_string: String,
}

impl IsolatedTestDatabase {
    async fn new() -> Result<Self, sqlx::Error> {
        let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_owned());
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_owned());
        let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await?;
        let schema = format!("eventcore_reset_test_{}", Uuid::now_v7().simple());
        let _ = query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await?;
        admin.close().await;

        let pool_schema = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .after_connect(move |connection, _| {
                let schema = pool_schema.clone();
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
        Ok(Self {
            pool,
            schema,
            connection_string,
        })
    }

    async fn cleanup(self) -> Result<(), sqlx::Error> {
        self.pool.close().await;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await?;
        let _ = query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&admin)
            .await?;
        admin.close().await;
        Ok(())
    }
}

#[derive(Debug, Error)]
enum FixtureError {
    #[error("event store operation failed")]
    EventStore(#[from] eventcore_types::EventStoreError),
    #[error("postgres operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("projection source operation failed")]
    Source(#[from] eventcore_postgres::PostgresProjectionSourceError),
    #[error("projection runner failed")]
    Runner(#[from] TransactionalProjectionError),
    #[error("projection reset failed")]
    Reset(#[from] ProjectionResetError),
    #[error("projection reset and replay failed")]
    ResetAndReplay(#[from] ProjectionResetAndReplayError),
    #[error("legacy checkpoint operation failed")]
    LegacyCheckpoint(#[from] eventcore_postgres::PostgresCheckpointError),
    #[error("fixture task failed")]
    Task(#[from] tokio::task::JoinError),
    #[error("fixture operation timed out")]
    TimedOut,
    #[error("fixture observation was unavailable: {0}")]
    Observation(&'static str),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ValueAdded {
    stream_id: StreamId,
    amount: i64,
}

impl Event for ValueAdded {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "reset-value-added"
    }
}

#[derive(Clone)]
struct ApplyGate {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

struct ValueProjector {
    name: ProjectorName,
    gate: Option<ApplyGate>,
    leadership_probe: Option<LeadershipProbe>,
}

impl PostgresProjector for ValueProjector {
    type Event = ValueAdded;
    type Error = sqlx::Error;
    type AfterCommit = NoopAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    async fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        _position: DeliveryPosition,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> Result<Self::AfterCommit, Self::Error>
    where
        'c: 'a,
    {
        if let Some(gate) = self.gate.take() {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        if let Some(probe) = &self.leadership_probe {
            let pid: i32 = query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut **tx)
                .await?;
            *probe.replay_pid.lock().await = Some(pid);
        }
        let _ = query("UPDATE reset_projection_model SET total = total + $1")
            .bind(event.amount)
            .execute(&mut **tx)
            .await?;
        Ok(NoopAfterCommit)
    }
}

#[derive(Debug, Error)]
enum ResetCallbackError {
    #[error("fixture reset callback failed after mutation")]
    Mutation,
    #[error("fixture leadership probe failed: {0}")]
    Probe(String),
}

type LockWaiterTask = JoinHandle<Result<(), sqlx::Error>>;

#[derive(Clone)]
struct LeadershipProbe {
    pool: Pool<Postgres>,
    reset_pid: Arc<AsyncMutex<Option<i32>>>,
    replay_pid: Arc<AsyncMutex<Option<i32>>>,
    waiter_pid: Arc<AsyncMutex<Option<i32>>>,
    waiter_task: Arc<AsyncMutex<Option<LockWaiterTask>>>,
    waiter_release: Arc<Notify>,
    queued_before_reset_commit: Arc<AtomicBool>,
}

impl LeadershipProbe {
    fn new(pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            reset_pid: Arc::new(AsyncMutex::new(None)),
            replay_pid: Arc::new(AsyncMutex::new(None)),
            waiter_pid: Arc::new(AsyncMutex::new(None)),
            waiter_task: Arc::new(AsyncMutex::new(None)),
            waiter_release: Arc::new(Notify::new()),
            queued_before_reset_commit: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn start_exact_lock_waiter(&self, leader_pid: i32) -> Result<(), ResetCallbackError> {
        let lock = query(
            "SELECT classid::bigint AS classid, objid::bigint AS objid \
             FROM pg_locks WHERE locktype = 'advisory' AND pid = $1 \
             AND granted AND objsubid = 1",
        )
        .bind(leader_pid)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        let classid: i64 = lock
            .try_get("classid")
            .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        let objid: i64 = lock
            .try_get("objid")
            .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        let key = (((classid as u64) << 32) | objid as u64) as i64;

        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        connection.close_on_drop();
        let waiter_pid: i32 = query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        *self.waiter_pid.lock().await = Some(waiter_pid);
        let release = self.waiter_release.clone();
        let task = tokio::spawn(async move {
            let _ = query("SELECT pg_advisory_lock($1)")
                .bind(key)
                .execute(&mut *connection)
                .await?;
            release.notified().await;
            let _: bool = query_scalar("SELECT pg_advisory_unlock($1)")
                .bind(key)
                .fetch_one(&mut *connection)
                .await?;
            connection.close().await
        });
        *self.waiter_task.lock().await = Some(task);

        let queued = timeout(RUN_TIMEOUT, async {
            loop {
                if waiter_is_queued(&self.pool, waiter_pid)
                    .await
                    .map_err(|error| ResetCallbackError::Probe(error.to_string()))?
                {
                    return Ok::<(), ResetCallbackError>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| ResetCallbackError::Probe("waiter was not queued in time".to_owned()))?;
        queued?;
        self.queued_before_reset_commit
            .store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn waiter_queued(&self) -> Result<bool, sqlx::Error> {
        let Some(pid) = *self.waiter_pid.lock().await else {
            return Ok(false);
        };
        waiter_is_queued(&self.pool, pid).await
    }

    async fn cleanup_waiter(&self) {
        self.waiter_release.notify_one();
        let Some(mut task) = self.waiter_task.lock().await.take() else {
            return;
        };
        if timeout(RUN_TIMEOUT, &mut task).await.is_err() {
            task.abort();
            let _ = timeout(RUN_TIMEOUT, &mut task).await;
        }
    }
}

async fn waiter_is_queued(pool: &Pool<Postgres>, pid: i32) -> Result<bool, sqlx::Error> {
    query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_stat_activity \
         WHERE pid = $1 AND state = 'active' AND wait_event_type = 'Lock')",
    )
    .bind(pid)
    .fetch_one(pool)
    .await
}

struct ModelReset {
    behavior: ProjectionResetBehavior,
    attempts: Arc<AtomicU64>,
    leadership_probe: Option<LeadershipProbe>,
}

impl PostgresProjectionReset for ModelReset {
    type Error = ResetCallbackError;

    async fn reset<'a, 'c>(
        &'a mut self,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> Result<(), Self::Error>
    where
        'c: 'a,
    {
        let _ = self.attempts.fetch_add(1, Ordering::SeqCst);
        let total = match self.behavior {
            ProjectionResetBehavior::Succeed => 0_i64,
            ProjectionResetBehavior::MutateThenFail => -9_001_i64,
        };
        let _ = query("UPDATE reset_projection_model SET total = $1")
            .bind(total)
            .execute(&mut **tx)
            .await
            .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
        if self.behavior == ProjectionResetBehavior::MutateThenFail {
            return Err(ResetCallbackError::Mutation);
        }
        if let Some(probe) = &self.leadership_probe {
            let pid: i32 = query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut **tx)
                .await
                .map_err(|error| ResetCallbackError::Probe(error.to_string()))?;
            *probe.reset_pid.lock().await = Some(pid);
            probe.start_exact_lock_waiter(pid).await?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct GatedSource {
    inner: PostgresProjectionSource,
    gate_once: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl ProjectionSource for GatedSource {
    type Error = eventcore_postgres::PostgresProjectionSourceError;

    fn source_id(&self) -> &DeliverySourceId {
        self.inner.source_id()
    }

    async fn high_watermark(&self) -> Result<Option<DeliveryPosition>, Self::Error> {
        if self
            .gate_once
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.inner.high_watermark().await
    }

    async fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> Result<Vec<eventcore_types::PersistedEventEnvelope>, Self::Error> {
        self.inner
            .read_envelopes(selection, after, through, limit)
            .await
    }
}

struct PostgresResetFixture {
    database: IsolatedTestDatabase,
    event_store: eventcore_postgres::PostgresEventStore,
    source: PostgresProjectionSource,
    store: PostgresProjectionStore,
    selection: ProjectionSelection,
    source_id: DeliverySourceId,
    projector_name: ProjectorName,
    callback_attempts: Arc<AtomicU64>,
    legacy_position: StreamPosition,
}

impl PostgresResetFixture {
    async fn new() -> Result<Self, FixtureError> {
        let database = IsolatedTestDatabase::new().await?;
        let event_store = eventcore_postgres::PostgresEventStore::from_pool(database.pool.clone());
        event_store.migrate().await;
        let source_id = DeliverySourceId::try_new("reset-primary-source")
            .expect("literal source ID should be valid");
        let source = PostgresProjectionSource::from_pool(database.pool.clone(), source_id.clone());
        source.migrate().await?;
        let store = PostgresProjectionStore::from_pool(database.pool.clone());
        store.migrate().await?;
        let _ = query("CREATE TABLE reset_projection_model (total BIGINT NOT NULL)")
            .execute(&database.pool)
            .await?;
        let _ = query("INSERT INTO reset_projection_model (total) VALUES (0)")
            .execute(&database.pool)
            .await?;
        let selection = ProjectionSelection::try_new(
            ProjectionSelectionId::try_new("reset-values-v1")
                .expect("literal selection ID should be valid"),
            ProjectionStreamFilter::All,
            vec![
                EventTypeName::try_new("reset-value-added")
                    .expect("literal event type should be valid"),
            ],
        )
        .expect("fixture selection should be valid");
        let projector_name = ProjectorName::try_new(format!("reset-model-{}", database.schema))
            .expect("schema-derived projector name should be valid");
        Ok(Self {
            database,
            event_store,
            source,
            store,
            selection,
            source_id,
            projector_name,
            callback_attempts: Arc::new(AtomicU64::new(0)),
            legacy_position: StreamPosition::new(Uuid::from_u128(
                0x0199_1111_2222_7333_8444_5555_6666_7777,
            )),
        })
    }

    fn projector(&self, gate: Option<ApplyGate>) -> ValueProjector {
        ValueProjector {
            name: self.projector_name.clone(),
            gate,
            leadership_probe: None,
        }
    }

    fn probed_projector(&self, probe: LeadershipProbe) -> ValueProjector {
        ValueProjector {
            name: self.projector_name.clone(),
            gate: None,
            leadership_probe: Some(probe),
        }
    }

    fn resetter(&self, behavior: ProjectionResetBehavior) -> ModelReset {
        ModelReset {
            behavior,
            attempts: self.callback_attempts.clone(),
            leadership_probe: None,
        }
    }

    fn probed_resetter(&self, probe: LeadershipProbe) -> ModelReset {
        ModelReset {
            behavior: ProjectionResetBehavior::Succeed,
            attempts: self.callback_attempts.clone(),
            leadership_probe: Some(probe),
        }
    }

    fn config(&self) -> PostgresProjectionConfig {
        PostgresProjectionConfig::new(self.selection.clone())
    }

    async fn cleanup(self) -> Result<(), FixtureError> {
        let Self {
            database,
            event_store,
            source,
            store,
            ..
        } = self;
        drop(event_store);
        drop(source);
        drop(store);
        database.cleanup().await?;
        Ok(())
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

fn classify_reset(error: ProjectionResetError) -> ProjectionResetFailureObservation {
    match error {
        ProjectionResetError::Busy => ProjectionResetFailureObservation::Busy,
        ProjectionResetError::SourceIdentityMismatch {
            projector,
            persisted,
            configured,
        } => ProjectionResetFailureObservation::SourceIdentityMismatch {
            projector,
            persisted,
            configured,
        },
        ProjectionResetError::SelectionIdentityMismatch {
            projector,
            persisted,
            configured,
        } => ProjectionResetFailureObservation::SelectionIdentityMismatch {
            projector,
            persisted,
            configured,
        },
        ProjectionResetError::Callback { source } => ProjectionResetFailureObservation::Callback {
            source: source.to_string(),
        },
        ProjectionResetError::CommitIndeterminate { .. } => {
            ProjectionResetFailureObservation::CommitIndeterminate
        }
        ProjectionResetError::Progress { .. } => ProjectionResetFailureObservation::Progress,
        ProjectionResetError::LeadershipLost { .. } => ProjectionResetFailureObservation::Other,
    }
}

fn classify_runner(error: TransactionalProjectionError) -> ProjectionFailureObservation {
    match error {
        TransactionalProjectionError::LeadershipBusy => {
            ProjectionFailureObservation::LeadershipBusy
        }
        TransactionalProjectionError::LeadershipLost { .. } => {
            ProjectionFailureObservation::LeadershipLost
        }
        _ => ProjectionFailureObservation::Other,
    }
}

async fn abort_and_join<T>(task: &mut JoinHandle<T>) {
    task.abort();
    let _ = timeout(RUN_TIMEOUT, task).await;
}

async fn await_spawned<T>(task: &mut JoinHandle<T>, joined: &mut bool) -> Result<T, FixtureError> {
    let join_result = timeout(RUN_TIMEOUT, task)
        .await
        .map_err(|_| FixtureError::TimedOut)?;
    // `JoinHandle` was consumed by the completed poll even when its output is `Err` or it panicked.
    // Mark it joined before propagating the nested JoinError so cleanup never polls it again.
    *joined = true;
    join_result.map_err(FixtureError::from)
}

enum CleanupOutcome {
    Complete,
    Failed(String),
    TimedOut,
    Panicked(Box<dyn std::any::Any + Send>),
}

async fn bounded_cleanup<F, E>(duration: Duration, cleanup: F) -> CleanupOutcome
where
    F: Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    match AssertUnwindSafe(timeout(duration, cleanup))
        .catch_unwind()
        .await
    {
        Ok(Ok(Ok(()))) => CleanupOutcome::Complete,
        Ok(Ok(Err(error))) => CleanupOutcome::Failed(error.to_string()),
        Ok(Err(_)) => CleanupOutcome::TimedOut,
        Err(payload) => CleanupOutcome::Panicked(payload),
    }
}

impl TransactionalProjectionResetFixture for PostgresResetFixture {
    type Error = FixtureError;

    async fn append_reset_values(
        &mut self,
        values: &[i64],
    ) -> Result<Vec<DeliveryPosition>, Self::Error> {
        let after = self.source.high_watermark().await?;
        let stream_id = StreamId::try_new(format!("reset::{}", Uuid::now_v7()))
            .expect("fixture stream ID should be valid");
        let mut writes =
            StreamWrites::new().register_stream(stream_id.clone(), StreamVersion::new(0))?;
        for amount in values {
            writes = writes.append(ValueAdded {
                stream_id: stream_id.clone(),
                amount: *amount,
            })?;
        }
        let _ = self.event_store.append_events(writes).await?;
        let through = self
            .source
            .high_watermark()
            .await?
            .expect("append should establish a global frontier");
        Ok(self
            .source
            .read_envelopes(
                &self.selection,
                after,
                DeliveryUpperBound::Inclusive(through),
                BatchSize::new(values.len()),
            )
            .await?
            .into_iter()
            .map(|envelope| envelope.position())
            .collect())
    }

    async fn run_reset_fixture_batch(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        Ok(convert_outcome(
            timeout(
                RUN_TIMEOUT,
                run_transactional_projection(
                    self.projector(None),
                    &self.source,
                    &self.store,
                    self.config(),
                ),
            )
            .await
            .map_err(|_| FixtureError::TimedOut)??,
        ))
    }

    async fn reset_attempt(
        &mut self,
        behavior: ProjectionResetBehavior,
    ) -> Result<ProjectionResetAttemptObservation, Self::Error> {
        let mut reset = self.resetter(behavior);
        Ok(
            match timeout(
                RUN_TIMEOUT,
                reset_transactional_projection(
                    &mut reset,
                    &self.projector_name,
                    &self.source_id,
                    self.selection.id(),
                    &self.store,
                ),
            )
            .await
            .map_err(|_| FixtureError::TimedOut)?
            {
                Ok(()) => ProjectionResetAttemptObservation::Completed,
                Err(error) => ProjectionResetAttemptObservation::Failed(classify_reset(error)),
            },
        )
    }

    async fn inject_reset_progress_deletion_failure(&mut self) -> Result<(), Self::Error> {
        let _ = query(
            "CREATE FUNCTION fixture_reject_progress_delete() RETURNS trigger LANGUAGE plpgsql \
             AS $$ BEGIN RAISE EXCEPTION 'fixture progress delete failure'; END; $$",
        )
        .execute(&self.database.pool)
        .await?;
        let _ = query(
            "CREATE TRIGGER fixture_reject_progress_delete BEFORE DELETE \
             ON eventcore_projection_progress FOR EACH ROW \
             EXECUTE FUNCTION fixture_reject_progress_delete()",
        )
        .execute(&self.database.pool)
        .await?;
        Ok(())
    }

    async fn observe_reset_while_runner_owns_leadership(
        &mut self,
    ) -> Result<ProjectionResetAttemptObservation, Self::Error> {
        let _ = self.append_reset_values(&[1]).await?;
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let projector = self.projector(Some(ApplyGate {
            entered: entered.clone(),
            release: release.clone(),
        }));
        let source = self.source.clone();
        let store = self.store.clone();
        let config = self.config();
        let mut runner = tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        });
        let operation = AssertUnwindSafe(async {
            timeout(RUN_TIMEOUT, entered.notified())
                .await
                .map_err(|_| FixtureError::TimedOut)?;
            self.reset_attempt(ProjectionResetBehavior::Succeed).await
        })
        .catch_unwind()
        .await;
        release.notify_one();
        let runner_cleanup = match timeout(RUN_TIMEOUT, &mut runner).await {
            Ok(result) => result
                .map_err(FixtureError::from)
                .and_then(|result| result.map(|_| ()).map_err(FixtureError::from)),
            Err(_) => {
                abort_and_join(&mut runner).await;
                Err(FixtureError::TimedOut)
            }
        };
        match operation {
            Ok(result) => {
                runner_cleanup?;
                result
            }
            Err(payload) => {
                let _ = runner_cleanup;
                resume_unwind(payload)
            }
        }
    }

    async fn reset_and_replay(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        let mut reset = self.resetter(ProjectionResetBehavior::Succeed);
        Ok(convert_outcome(
            timeout(
                RUN_TIMEOUT,
                reset_and_replay_transactional_projection(
                    self.projector(None),
                    &mut reset,
                    &self.source,
                    &self.store,
                    self.config(),
                ),
            )
            .await
            .map_err(|_| FixtureError::TimedOut)??,
        ))
    }

    async fn observe_reset_replay_leadership(
        &mut self,
    ) -> Result<ProjectionResetReplayLeadershipObservation, Self::Error> {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let source = GatedSource {
            inner: self.source.clone(),
            gate_once: Arc::new(AtomicBool::new(true)),
            entered: entered.clone(),
            release: release.clone(),
        };
        let store = self.store.clone();
        let config = self.config();
        let probe = LeadershipProbe::new(self.database.pool.clone());
        let projector = self.probed_projector(probe.clone());
        let mut reset = self.probed_resetter(probe.clone());
        let mut orchestrator = tokio::spawn(async move {
            reset_and_replay_transactional_projection(
                projector, &mut reset, &source, &store, config,
            )
            .await
        });
        let mut joined = false;
        let operation = AssertUnwindSafe(async {
            timeout(RUN_TIMEOUT, entered.notified())
                .await
                .map_err(|_| FixtureError::TimedOut)?;
            let state_while_gated = timeout(RUN_TIMEOUT, self.reset_state())
                .await
                .map_err(|_| FixtureError::TimedOut)??;
            let waiter_queued_during_replay = timeout(RUN_TIMEOUT, probe.waiter_queued())
                .await
                .map_err(|_| FixtureError::TimedOut)??;
            let competitor = timeout(
                RUN_TIMEOUT,
                run_transactional_projection(
                    self.projector(None),
                    &self.source,
                    &self.store,
                    self.config(),
                ),
            )
            .await
            .map_err(|_| FixtureError::TimedOut)?;
            let competing_failure = match competitor {
                Ok(_) => ProjectionFailureObservation::Other,
                Err(error) => classify_runner(error),
            };
            release.notify_one();
            let replay = await_spawned(&mut orchestrator, &mut joined).await??;
            let reset_pid = probe
                .reset_pid
                .lock()
                .await
                .ok_or(FixtureError::Observation("reset callback backend PID"))?;
            let replay_pid = probe
                .replay_pid
                .lock()
                .await
                .ok_or(FixtureError::Observation("replay apply backend PID"))?;
            Ok(ProjectionResetReplayLeadershipObservation {
                state_while_gated,
                waiter_queued_before_reset_commit: probe
                    .queued_before_reset_commit
                    .load(Ordering::SeqCst),
                waiter_queued_during_replay,
                reset_session_token: reset_pid.to_string(),
                replay_session_token: replay_pid.to_string(),
                competing_failure,
                replay_outcome: convert_outcome(replay),
            })
        })
        .catch_unwind()
        .await;

        release.notify_one();
        if !joined {
            abort_and_join(&mut orchestrator).await;
        }
        probe.cleanup_waiter().await;
        match operation {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    async fn reset_state(&self) -> Result<ProjectionResetStateObservation, Self::Error> {
        let total = query_scalar::<_, i64>("SELECT total FROM reset_projection_model")
            .fetch_one(&self.database.pool)
            .await?;
        let progress = self.store.progress(&self.projector_name).await?;
        Ok(ProjectionResetStateObservation {
            model_total: total,
            progress: progress.map(|progress| ProjectionProgressObservation {
                source_id: progress.source_id().clone(),
                selection_id: progress.selection_id().clone(),
                position: progress.position(),
            }),
        })
    }

    async fn reset_callback_attempt_count(&self) -> Result<u64, Self::Error> {
        Ok(self.callback_attempts.load(Ordering::SeqCst))
    }

    async fn seed_reset_progress_identity(
        &mut self,
        source_id: DeliverySourceId,
        selection_id: ProjectionSelectionId,
        position: DeliveryPosition,
    ) -> Result<(), Self::Error> {
        let position = i64::try_from(position.get()).expect("fixture position should fit BIGINT");
        let _ = query(
            "INSERT INTO eventcore_projection_progress \
             (projector_name, source_id, selection_id, last_position) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (projector_name) DO UPDATE SET source_id = EXCLUDED.source_id, \
             selection_id = EXCLUDED.selection_id, last_position = EXCLUDED.last_position",
        )
        .bind(self.projector_name.as_ref())
        .bind(source_id.as_ref())
        .bind(selection_id.as_ref())
        .bind(position)
        .execute(&self.database.pool)
        .await?;
        Ok(())
    }

    async fn seed_legacy_checkpoint(&mut self) -> Result<(), Self::Error> {
        let legacy = PostgresCheckpointStore::from_pool(self.database.pool.clone());
        legacy
            .save(self.projector_name.as_ref(), self.legacy_position)
            .await?;
        Ok(())
    }

    async fn seed_stale_legacy_model(&mut self, total: i64) -> Result<(), Self::Error> {
        let _ = query("UPDATE reset_projection_model SET total = $1")
            .bind(total)
            .execute(&self.database.pool)
            .await?;
        Ok(())
    }

    async fn legacy_checkpoint_is_unchanged(&self) -> Result<bool, Self::Error> {
        let legacy = PostgresCheckpointStore::from_pool(self.database.pool.clone());
        Ok(legacy.load(self.projector_name.as_ref()).await? == Some(self.legacy_position))
    }

    fn reset_source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    fn reset_selection_id(&self) -> &ProjectionSelectionId {
        self.selection.id()
    }

    fn reset_projector_name(&self) -> &ProjectorName {
        &self.projector_name
    }
}

// Break caught: applying a nested task error before marking the completed JoinHandle consumed
// would make unconditional cleanup poll that handle a second time and panic.
#[tokio::test]
async fn completed_error_and_panicked_handles_are_marked_joined_before_propagation() {
    let mut error_task = tokio::spawn(async { Err::<(), &'static str>("task output failure") });
    let mut error_joined = false;
    assert_eq!(
        await_spawned(&mut error_task, &mut error_joined)
            .await
            .expect("task itself should join"),
        Err("task output failure"),
    );
    assert!(error_joined);

    let mut panic_task = tokio::spawn(async { panic!("fixture task panic") });
    let mut panic_joined = false;
    assert!(matches!(
        await_spawned(&mut panic_task, &mut panic_joined).await,
        Err(FixtureError::Task(_)),
    ));
    assert!(panic_joined);
}

// Break caught: awaiting pool/schema cleanup without a bound can hang the entire contract suite
// after a leaked connection or blocked task.
#[tokio::test]
async fn cleanup_timeout_is_a_deterministic_outcome() {
    let outcome = bounded_cleanup(
        Duration::from_millis(1),
        std::future::pending::<Result<(), std::io::Error>>(),
    )
    .await;
    assert!(matches!(outcome, CleanupOutcome::TimedOut));
}

// Break caught: aborting the orchestrator without waking and joining its queued-lock waiter can
// leave a live backend that prevents pool and schema cleanup.
#[tokio::test]
async fn waiter_cleanup_wakes_and_joins_the_owned_task() {
    let database = IsolatedTestDatabase::new()
        .await
        .expect("isolated database should initialize");
    let probe = LeadershipProbe::new(database.pool.clone());
    let completed = Arc::new(AtomicBool::new(false));
    let completed_by_task = completed.clone();
    let release = probe.waiter_release.clone();
    *probe.waiter_task.lock().await = Some(tokio::spawn(async move {
        release.notified().await;
        completed_by_task.store(true, Ordering::SeqCst);
        Ok(())
    }));

    probe.cleanup_waiter().await;
    assert!(completed.load(Ordering::SeqCst));
    assert!(probe.waiter_task.lock().await.is_none());
    assert!(matches!(
        bounded_cleanup(RUN_TIMEOUT, database.cleanup()).await,
        CleanupOutcome::Complete,
    ));
}

macro_rules! reset_contract_test {
    ($name:ident, $contract:path) => {
        // Every contract uses a unique projector/schema and guarantees bounded cleanup even when
        // an assertion panics; this prevents advisory locks or schemas leaking into later tests.
        #[tokio::test]
        async fn $name() {
            let mut fixture = PostgresResetFixture::new()
                .await
                .expect("reset fixture should initialize");
            let result = AssertUnwindSafe($contract(&mut fixture))
                .catch_unwind()
                .await;
            let cleanup = bounded_cleanup(RUN_TIMEOUT, fixture.cleanup()).await;
            match (result, cleanup) {
                (Ok(Ok(())), CleanupOutcome::Complete) => {}
                (Ok(Err(error)), CleanupOutcome::Complete) => {
                    panic!("reset contract should complete: {error}")
                }
                (Ok(Ok(())), CleanupOutcome::Failed(cleanup)) => {
                    panic!("reset contract passed but cleanup failed: {cleanup}")
                }
                (Ok(Err(error)), CleanupOutcome::Failed(cleanup)) => {
                    panic!("reset contract failed: {error}; cleanup also failed: {cleanup}")
                }
                (Ok(Ok(())), CleanupOutcome::TimedOut) => {
                    panic!("reset contract passed but cleanup timed out")
                }
                (Ok(Err(error)), CleanupOutcome::TimedOut) => {
                    panic!("reset contract failed: {error}; cleanup also timed out")
                }
                (Ok(_), CleanupOutcome::Panicked(payload)) => resume_unwind(payload),
                (Err(payload), CleanupOutcome::Complete) => resume_unwind(payload),
                (Err(payload), CleanupOutcome::Failed(cleanup)) => {
                    eprintln!("cleanup also failed while preserving contract panic: {cleanup}");
                    resume_unwind(payload);
                }
                (Err(payload), CleanupOutcome::TimedOut) => {
                    eprintln!("cleanup also timed out while preserving contract panic");
                    resume_unwind(payload);
                }
                (Err(payload), CleanupOutcome::Panicked(_cleanup_panic)) => {
                    eprintln!("cleanup also panicked while preserving contract panic");
                    resume_unwind(payload);
                }
            }
        }
    };
}

// Break caught: reset that steals or waits for the runner's named lock would violate the
// non-blocking recovery contract and could race a live writer.
reset_contract_test!(
    reset_is_busy_while_runner_owns_leadership,
    reset_busy_contract
);

// Break caught: callback mutation or progress deletion outside one transaction would expose a
// half-reset model when application reset code fails.
reset_contract_test!(
    failed_reset_rolls_back_model_and_progress,
    reset_callback_failure_rolls_back_contract
);

// Break caught: committing model reset and progress deletion separately could expose mismatched
// recovery state after either statement commits alone.
reset_contract_test!(
    successful_reset_commits_model_and_progress_deletion,
    successful_reset_is_atomic_contract
);

// Break caught: committing a successful callback mutation before deleting progress would expose
// a cleared model paired with the old checkpoint when the progress DELETE fails.
reset_contract_test!(
    progress_delete_failure_rolls_back_successful_callback,
    reset_progress_failure_rolls_back_contract
);

// Break caught: invoking the callback before validating durable source identity can destroy the
// wrong projection before reporting the mismatch.
reset_contract_test!(
    reset_validates_source_identity_before_callback,
    reset_source_identity_validation_contract
);

// Break caught: invoking the callback before validating durable selection identity can silently
// rebuild a model under changed projection semantics.
reset_contract_test!(
    reset_validates_selection_identity_before_callback,
    reset_selection_identity_validation_contract
);

// Break caught: replaying from old progress, skipping an event, or applying one twice produces a
// different exact aggregate than rebuilding all selected events once.
reset_contract_test!(
    reset_and_replay_reconstructs_exact_model,
    reset_and_replay_reconstructs_model_contract
);

// Break caught: releasing leadership between reset commit and the first replay source read lets a
// competing writer observe and race the empty projection.
reset_contract_test!(
    reset_and_replay_retains_leadership_between_phases,
    reset_and_replay_retains_leadership_contract
);

// Break caught: translating an opaque legacy UUID checkpoint into the global sequence can skip
// selected history; reset/replay must rebuild from the new delivery contract instead.
reset_contract_test!(
    legacy_uuid_checkpoint_is_adopted_through_reset_replay,
    legacy_checkpoint_reset_replay_contract
);
