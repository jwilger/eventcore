//! Public reset-and-replay contracts for transactional PostgreSQL projections.

#[path = "common/fixture_lifecycle.rs"]
mod fixture_lifecycle;

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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

const RUN_TIMEOUT: Duration = Duration::from_secs(3);
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

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

struct DatabasePlan {
    schema: String,
    connection_string: String,
}

impl DatabasePlan {
    fn new() -> Self {
        let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_owned());
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_owned());
        Self {
            schema: format!("eventcore_reset_test_{}", Uuid::now_v7().simple()),
            connection_string: format!("postgres://postgres:postgres@{host}:{port}/postgres"),
        }
    }

    async fn cleanup(&self) -> Result<(), sqlx::Error> {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await?;
        let _ = query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&admin)
            .await?;
        admin.close().await;
        Ok(())
    }
}

impl IsolatedTestDatabase {
    async fn new(plan: &DatabasePlan) -> Result<Self, sqlx::Error> {
        let connection_string = plan.connection_string.clone();
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await?;
        let schema = plan.schema.clone();
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
    #[error("fixture transport operation failed")]
    Io(#[from] std::io::Error),
    #[error("reset commit acknowledgement proxy failed: {0}")]
    CommitAcknowledgementProxy(String),
}

struct ResetCommitObservation {
    connection_string: String,
    schema: String,
    projector_name: ProjectorName,
}

struct AbortOnDrop<T> {
    task: Option<JoinHandle<T>>,
}

impl<T> AbortOnDrop<T> {
    fn new(task: JoinHandle<T>) -> Self {
        Self { task: Some(task) }
    }

    fn task_mut(&mut self) -> &mut JoinHandle<T> {
        self.task.as_mut().expect("owned task should be present")
    }

    async fn abort_and_join(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        task.abort();
        let _ = timeout(RUN_TIMEOUT, task).await;
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

struct ResetCommitAcknowledgementProxy {
    store: PostgresProjectionStore,
    confirmation: AsyncMutex<Option<oneshot::Receiver<Result<(), String>>>>,
    task: AbortOnDrop<()>,
}

impl ResetCommitAcknowledgementProxy {
    async fn start(
        database: &IsolatedTestDatabase,
        projector_name: ProjectorName,
    ) -> Result<Self, FixtureError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy_address = listener.local_addr()?;
        let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_owned());
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_owned());
        let target = format!("{host}:{port}");
        let observation = ResetCommitObservation {
            connection_string: database.connection_string.clone(),
            schema: database.schema.clone(),
            projector_name,
        };
        let (confirmation_sender, confirmation_receiver) = oneshot::channel();
        let mut task = AbortOnDrop::new(tokio::spawn(async move {
            let result =
                run_reset_commit_acknowledgement_proxy(listener, target, observation).await;
            let _ = confirmation_sender.send(result);
        }));

        let schema = database.schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |connection, _| {
                let schema = schema.clone();
                Box::pin(async move {
                    let _ = query("SELECT set_config('search_path', $1, false)")
                        .bind(schema)
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&format!(
                "postgres://postgres:postgres@{proxy_address}/postgres?sslmode=disable"
            ))
            .await;
        let pool = match pool {
            Ok(pool) => pool,
            Err(error) => {
                task.abort_and_join().await;
                return Err(FixtureError::Sql(error));
            }
        };

        Ok(Self {
            store: PostgresProjectionStore::from_pool(pool),
            confirmation: AsyncMutex::new(Some(confirmation_receiver)),
            task,
        })
    }

    async fn await_committed_observation(&self) -> Result<(), FixtureError> {
        let receiver = self.confirmation.lock().await.take().ok_or_else(|| {
            FixtureError::CommitAcknowledgementProxy(
                "commit acknowledgement observation was awaited more than once".to_owned(),
            )
        })?;
        let result = timeout(RUN_TIMEOUT, receiver)
            .await
            .map_err(|_| FixtureError::TimedOut)?
            .map_err(|_| {
                FixtureError::CommitAcknowledgementProxy(
                    "proxy exited without a commit acknowledgement observation".to_owned(),
                )
            })?;
        result.map_err(FixtureError::CommitAcknowledgementProxy)
    }

    async fn shutdown(mut self) {
        self.task.abort_and_join().await;
    }
}

async fn run_reset_commit_acknowledgement_proxy(
    listener: TcpListener,
    target: String,
    observation: ResetCommitObservation,
) -> Result<(), String> {
    let (client, _) = listener
        .accept()
        .await
        .map_err(|error| format!("proxy did not accept reset connection: {error}"))?;
    let server = TcpStream::connect(target)
        .await
        .map_err(|error| format!("proxy did not connect to PostgreSQL: {error}"))?;
    let (client_reader, client_writer) = client.into_split();
    let (server_reader, server_writer) = server.into_split();
    let (commit_sender, mut commit_receiver) = oneshot::channel();
    let mut frontend = AbortOnDrop::new(tokio::spawn(forward_reset_postgres_frontend(
        client_reader,
        server_writer,
        commit_sender,
    )));

    let result = forward_reset_postgres_backend(
        server_reader,
        client_writer,
        &mut commit_receiver,
        observation,
    )
    .await;
    frontend.abort_and_join().await;
    result
}

async fn forward_reset_postgres_frontend(
    mut client: tokio::net::tcp::OwnedReadHalf,
    mut server: tokio::net::tcp::OwnedWriteHalf,
    commit_sender: oneshot::Sender<()>,
) -> Result<(), String> {
    let startup_length = client
        .read_u32()
        .await
        .map_err(|error| format!("proxy could not read startup length: {error}"))?;
    let mut startup = vec![0; checked_postgres_payload_length(startup_length)?];
    let _ = client
        .read_exact(&mut startup)
        .await
        .map_err(|error| format!("proxy could not read startup payload: {error}"))?;
    server
        .write_u32(startup_length)
        .await
        .map_err(|error| format!("proxy could not forward startup length: {error}"))?;
    server
        .write_all(&startup)
        .await
        .map_err(|error| format!("proxy could not forward startup payload: {error}"))?;
    server
        .flush()
        .await
        .map_err(|error| format!("proxy could not flush startup packet: {error}"))?;

    let mut commit_sender = Some(commit_sender);
    loop {
        let tag = client
            .read_u8()
            .await
            .map_err(|error| format!("proxy could not read frontend tag: {error}"))?;
        let length = client
            .read_u32()
            .await
            .map_err(|error| format!("proxy could not read frontend length: {error}"))?;
        let mut payload = vec![0; checked_postgres_payload_length(length)?];
        let _ = client
            .read_exact(&mut payload)
            .await
            .map_err(|error| format!("proxy could not read frontend payload: {error}"))?;
        server
            .write_u8(tag)
            .await
            .map_err(|error| format!("proxy could not forward frontend tag: {error}"))?;
        server
            .write_u32(length)
            .await
            .map_err(|error| format!("proxy could not forward frontend length: {error}"))?;
        server
            .write_all(&payload)
            .await
            .map_err(|error| format!("proxy could not forward frontend payload: {error}"))?;
        server
            .flush()
            .await
            .map_err(|error| format!("proxy could not flush frontend packet: {error}"))?;
        if tag == b'Q' && payload == b"COMMIT\0" {
            let _ = commit_sender
                .take()
                .expect("commit sender should be present")
                .send(());
        }
    }
}

async fn forward_reset_postgres_backend(
    mut server: tokio::net::tcp::OwnedReadHalf,
    mut client: tokio::net::tcp::OwnedWriteHalf,
    commit_receiver: &mut oneshot::Receiver<()>,
    observation: ResetCommitObservation,
) -> Result<(), String> {
    loop {
        let (tag, length, payload) = read_postgres_message(&mut server).await?;
        if tag == b'C' && payload == b"COMMIT\0" {
            timeout(RUN_TIMEOUT, &mut *commit_receiver)
                .await
                .map_err(|_| {
                    "proxy did not observe forwarded COMMIT before its completion".to_owned()
                })?
                .map_err(|_| "proxy frontend ended before forwarding COMMIT".to_owned())?;
            let _ = timeout(RUN_TIMEOUT, observe_fresh_reset_state(observation))
                .await
                .map_err(|_| "fresh reset commit observer timed out".to_owned())??;
            // Withhold CommandComplete and close the client connection only after a separate
            // PostgreSQL session has proved that the reset transaction committed atomically.
            return Ok(());
        }
        client
            .write_u8(tag)
            .await
            .map_err(|error| format!("proxy could not forward backend tag: {error}"))?;
        client
            .write_u32(length)
            .await
            .map_err(|error| format!("proxy could not forward backend length: {error}"))?;
        client
            .write_all(&payload)
            .await
            .map_err(|error| format!("proxy could not forward backend payload: {error}"))?;
        client
            .flush()
            .await
            .map_err(|error| format!("proxy could not flush backend packet: {error}"))?;
    }
}

async fn read_postgres_message(
    server: &mut tokio::net::tcp::OwnedReadHalf,
) -> Result<(u8, u32, Vec<u8>), String> {
    let tag = server
        .read_u8()
        .await
        .map_err(|error| format!("proxy could not read backend tag: {error}"))?;
    let length = server
        .read_u32()
        .await
        .map_err(|error| format!("proxy could not read backend length: {error}"))?;
    let mut payload = vec![0; checked_postgres_payload_length(length)?];
    let _ = server
        .read_exact(&mut payload)
        .await
        .map_err(|error| format!("proxy could not read backend payload: {error}"))?;
    Ok((tag, length, payload))
}

fn checked_postgres_payload_length(length: u32) -> Result<usize, String> {
    let length = length
        .checked_sub(4)
        .ok_or_else(|| "PostgreSQL protocol frame length was shorter than its header".to_owned())?;
    usize::try_from(length)
        .map_err(|_| "PostgreSQL protocol frame length did not fit usize".to_owned())
}

async fn observe_fresh_reset_state(
    observation: ResetCommitObservation,
) -> Result<ProjectionResetStateObservation, String> {
    let schema = observation.schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let schema = schema.clone();
            Box::pin(async move {
                let _ = query("SELECT set_config('search_path', $1, false)")
                    .bind(schema)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&observation.connection_string)
        .await
        .map_err(|error| format!("fresh reset observer could not connect: {error}"))?;
    let result = async {
        let model_total = query_scalar::<_, i64>("SELECT total FROM reset_projection_model")
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("fresh reset observer could not read model: {error}"))?;
        let progress_exists: bool = query_scalar(
            "SELECT EXISTS(SELECT 1 FROM eventcore_projection_progress \
             WHERE projector_name = $1)",
        )
        .bind(observation.projector_name.as_ref())
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("fresh reset observer could not read progress: {error}"))?;
        if progress_exists {
            return Err(
                "COMMIT acknowledgement was withheld while reset progress still existed".to_owned(),
            );
        }
        if model_total != 0 {
            return Err(format!(
                "COMMIT acknowledgement was withheld while reset model total was {model_total}"
            ));
        }
        Ok(ProjectionResetStateObservation {
            model_total,
            progress: None,
        })
    }
    .await;
    pool.close().await;
    result
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

type LockWaiterTask = AbortOnDrop<Result<(), sqlx::Error>>;

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
        let task = AbortOnDrop::new(tokio::spawn(async move {
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
        }));
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
        if timeout(RUN_TIMEOUT, task.task_mut()).await.is_err() {
            task.abort_and_join().await;
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
    reset_commit_acknowledgement_proxy: Option<ResetCommitAcknowledgementProxy>,
}

impl PostgresResetFixture {
    async fn new(plan: &DatabasePlan) -> Result<Self, FixtureError> {
        let database = IsolatedTestDatabase::new(plan).await?;
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
            reset_commit_acknowledgement_proxy: None,
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
            reset_commit_acknowledgement_proxy,
            ..
        } = self;
        if let Some(proxy) = reset_commit_acknowledgement_proxy {
            proxy.shutdown().await;
        }
        drop(event_store);
        drop(source);
        drop(store);
        database.cleanup().await?;
        Ok(())
    }

    async fn inject_reset_commit_acknowledgement_loss(&mut self) -> Result<(), FixtureError> {
        let proxy =
            ResetCommitAcknowledgementProxy::start(&self.database, self.projector_name.clone())
                .await?;
        self.reset_commit_acknowledgement_proxy = Some(proxy);
        Ok(())
    }

    async fn reset_with_commit_acknowledgement_loss(
        &mut self,
    ) -> Result<Result<(), ProjectionResetError>, FixtureError> {
        let proxy =
            self.reset_commit_acknowledgement_proxy
                .as_ref()
                .ok_or(FixtureError::Observation(
                    "reset commit acknowledgement proxy",
                ))?;
        let mut reset = self.resetter(ProjectionResetBehavior::Succeed);
        let result = timeout(
            RUN_TIMEOUT,
            reset_transactional_projection(
                &mut reset,
                &self.projector_name,
                &self.source_id,
                self.selection.id(),
                &proxy.store,
            ),
        )
        .await
        .map_err(|_| FixtureError::TimedOut)?;
        proxy.await_committed_observation().await?;
        Ok(result)
    }

    async fn fresh_reset_state(&self) -> Result<ProjectionResetStateObservation, FixtureError> {
        observe_fresh_reset_state(ResetCommitObservation {
            connection_string: self.database.connection_string.clone(),
            schema: self.database.schema.clone(),
            projector_name: self.projector_name.clone(),
        })
        .await
        .map_err(FixtureError::CommitAcknowledgementProxy)
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
        unknown => panic!("unsupported projection run outcome: {unknown:?}"),
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
        _ => ProjectionResetFailureObservation::Other,
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

async fn abort_and_join<T>(task: &mut AbortOnDrop<T>) {
    task.abort_and_join().await;
}

async fn await_spawned<T>(task: &mut AbortOnDrop<T>, joined: &mut bool) -> Result<T, FixtureError> {
    let join_result = timeout(RUN_TIMEOUT, task.task_mut())
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

impl std::fmt::Debug for CleanupOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete => formatter.write_str("Complete"),
            Self::Failed(error) => formatter.debug_tuple("Failed").field(error).finish(),
            Self::TimedOut => formatter.write_str("TimedOut"),
            Self::Panicked(payload) => formatter
                .debug_tuple("Panicked")
                .field(&(**payload).type_id())
                .finish(),
        }
    }
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
        let mut runner = AbortOnDrop::new(tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        }));
        let operation = AssertUnwindSafe(async {
            timeout(RUN_TIMEOUT, entered.notified())
                .await
                .map_err(|_| FixtureError::TimedOut)?;
            self.reset_attempt(ProjectionResetBehavior::Succeed).await
        })
        .catch_unwind()
        .await;
        release.notify_one();
        let runner_cleanup = match timeout(RUN_TIMEOUT, runner.task_mut()).await {
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
        let mut orchestrator = AbortOnDrop::new(tokio::spawn(async move {
            reset_and_replay_transactional_projection(
                projector, &mut reset, &source, &store, config,
            )
            .await
        }));
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

type ResetFixtureFuture<'a> = fixture_lifecycle::FixtureFuture<'a, Result<(), FixtureError>>;

struct ResetFixtureOwner {
    plan: DatabasePlan,
    fixture: Option<PostgresResetFixture>,
}

impl ResetFixtureOwner {
    fn plan() -> Self {
        Self {
            plan: DatabasePlan::new(),
            fixture: None,
        }
    }

    async fn initialize(&mut self) -> Result<(), FixtureError> {
        self.fixture = Some(PostgresResetFixture::new(&self.plan).await?);
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), FixtureError> {
        let fixture_cleanup = match self.fixture.take() {
            Some(fixture) => fixture.cleanup().await,
            None => Ok(()),
        };
        let schema_cleanup = self.plan.cleanup().await.map_err(FixtureError::from);
        fixture_cleanup?;
        schema_cleanup
    }
}

fn initialize_reset_fixture(owner: &mut ResetFixtureOwner) -> ResetFixtureFuture<'_> {
    Box::pin(async move { owner.initialize().await })
}

fn cleanup_reset_fixture(owner: &mut ResetFixtureOwner) -> ResetFixtureFuture<'_> {
    Box::pin(async move { owner.cleanup().await })
}

async fn assert_reset_fixture(
    contract: impl for<'a> FnOnce(&'a mut PostgresResetFixture) -> ResetFixtureFuture<'a> + 'static,
) {
    let result = fixture_lifecycle::run_fixture(
        ResetFixtureOwner::plan(),
        fixture_lifecycle::FixtureTimeouts::new(TEST_TIMEOUT),
        initialize_reset_fixture,
        |owner| {
            Box::pin(async move {
                contract(
                    owner
                        .fixture
                        .as_mut()
                        .expect("reset fixture should be initialized"),
                )
                .await
            })
        },
        cleanup_reset_fixture,
    )
    .await;
    if let Err(error) = result {
        match error {
            fixture_lifecycle::FixtureLifecycleError::InitializationPanicked(payload)
            | fixture_lifecycle::FixtureLifecycleError::BodyPanicked(payload)
            | fixture_lifecycle::FixtureLifecycleError::CleanupPanicked(payload) => {
                resume_unwind(payload)
            }
            other => panic!("reset fixture lifecycle failed: {other:?}"),
        }
    }
}

// Break caught: applying a nested task error before marking the completed JoinHandle consumed
// would make unconditional cleanup poll that handle a second time and panic.
#[tokio::test]
async fn completed_error_and_panicked_handles_are_marked_joined_before_propagation() {
    let mut error_task = AbortOnDrop::new(tokio::spawn(async {
        Err::<(), &'static str>("task output failure")
    }));
    let mut error_joined = false;
    assert_eq!(
        await_spawned(&mut error_task, &mut error_joined)
            .await
            .expect("task itself should join"),
        Err("task output failure"),
    );
    assert!(error_joined);

    let mut panic_task = AbortOnDrop::new(tokio::spawn(async { panic!("fixture task panic") }));
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
    let plan = DatabasePlan::new();
    let database = IsolatedTestDatabase::new(&plan)
        .await
        .expect("isolated database should initialize");
    let probe = LeadershipProbe::new(database.pool.clone());
    let completed = Arc::new(AtomicBool::new(false));
    let completed_by_task = completed.clone();
    let release = probe.waiter_release.clone();
    *probe.waiter_task.lock().await = Some(AbortOnDrop::new(tokio::spawn(async move {
        release.notified().await;
        completed_by_task.store(true, Ordering::SeqCst);
        Ok(())
    })));

    probe.cleanup_waiter().await;
    assert!(completed.load(Ordering::SeqCst));
    assert!(probe.waiter_task.lock().await.is_none());
    assert!(matches!(
        bounded_cleanup(RUN_TIMEOUT, database.cleanup()).await,
        CleanupOutcome::Complete,
    ));
}

// Break caught: keeping a raw JoinHandle across the startup connection await lets cancellation
// detach the proxy task before the completed fixture can own and clean it up.
#[tokio::test]
async fn dropping_startup_task_guard_cancels_owned_task() {
    struct DropProbe(Option<oneshot::Sender<()>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    let started = Arc::new(Notify::new());
    let task_started = started.clone();
    let (dropped_sender, dropped_receiver) = oneshot::channel();
    let guard = AbortOnDrop::new(tokio::spawn(async move {
        let _probe = DropProbe(Some(dropped_sender));
        task_started.notify_one();
        std::future::pending::<()>().await;
    }));

    timeout(RUN_TIMEOUT, started.notified())
        .await
        .expect("owned startup task should start before its timeout");
    drop(guard);
    timeout(RUN_TIMEOUT, dropped_receiver)
        .await
        .expect("startup cancellation should stop the owned task before its timeout")
        .expect("startup task cancellation should drop its future");
}

// Break caught: dropping a raw lock-waiter handle detaches the task and retains its checked-out
// PostgreSQL connection, preventing bounded fixture cleanup from reacquiring the pool.
#[tokio::test]
async fn dropping_lock_waiter_owner_terminates_task_and_releases_database_connection() {
    assert_reset_fixture(|fixture| {
        Box::pin(async move {
            let single_connection_pool = timeout(
                RUN_TIMEOUT,
                PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&fixture.database.connection_string),
            )
            .await
            .map_err(|_| FixtureError::TimedOut)??;
            let (acquired_sender, acquired_receiver) = oneshot::channel();
            let (terminated_sender, terminated_receiver) = oneshot::channel();
            let task_pool = single_connection_pool.clone();
            let waiter = LockWaiterTask::new(tokio::spawn(async move {
                struct TerminationProbe(Option<oneshot::Sender<()>>);
                impl Drop for TerminationProbe {
                    fn drop(&mut self) {
                        if let Some(sender) = self.0.take() {
                            let _ = sender.send(());
                        }
                    }
                }

                let _probe = TerminationProbe(Some(terminated_sender));
                let _connection = task_pool.acquire().await?;
                let _ = acquired_sender.send(());
                std::future::pending::<Result<(), sqlx::Error>>().await
            }));
            timeout(RUN_TIMEOUT, acquired_receiver)
                .await
                .expect("waiter should acquire the only connection before its timeout")
                .expect("waiter should report its acquisition");
            assert!(
                timeout(Duration::from_millis(20), single_connection_pool.acquire())
                    .await
                    .is_err(),
                "the live waiter must own the pool's only connection",
            );

            drop(waiter);

            timeout(RUN_TIMEOUT, terminated_receiver)
                .await
                .expect("waiter cancellation should complete before its timeout")
                .expect("waiter cancellation should drop its future");
            let connection = timeout(RUN_TIMEOUT, single_connection_pool.acquire())
                .await
                .expect("released connection should be reacquired before its timeout")
                .expect("cancelled waiter must release its database connection");
            drop(connection);
            timeout(RUN_TIMEOUT, single_connection_pool.close())
                .await
                .map_err(|_| FixtureError::TimedOut)?;
            Ok(())
        })
    })
    .await;
}

// Break caught: keeping the frontend child as a raw JoinHandle lets cancellation of its proxy
// parent detach that child with its PostgreSQL-side socket still open.
#[tokio::test]
async fn cancelling_proxy_task_closes_frontend_connection() {
    let proxy_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy listener should bind");
    let proxy_address = proxy_listener
        .local_addr()
        .expect("proxy listener should have an address");
    let target_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("target listener should bind");
    let target_address = target_listener
        .local_addr()
        .expect("target listener should have an address");
    let mut proxy_task = AbortOnDrop::new(tokio::spawn(run_reset_commit_acknowledgement_proxy(
        proxy_listener,
        target_address.to_string(),
        ResetCommitObservation {
            connection_string: String::new(),
            schema: String::new(),
            projector_name: ProjectorName::try_new("cancellation-probe")
                .expect("literal projector name should be valid"),
        },
    )));
    let mut client = timeout(RUN_TIMEOUT, TcpStream::connect(proxy_address))
        .await
        .expect("client connection should complete before its timeout")
        .expect("client should connect to proxy");
    let (mut target, _) = timeout(RUN_TIMEOUT, target_listener.accept())
        .await
        .expect("target accept should complete before its timeout")
        .expect("proxy should connect to target");

    client
        .write_u32(8)
        .await
        .expect("client should write startup length");
    client
        .write_all(b"test")
        .await
        .expect("client should write startup payload");
    client.flush().await.expect("client should flush startup");
    let mut startup = [0_u8; 8];
    let _ = timeout(RUN_TIMEOUT, target.read_exact(&mut startup))
        .await
        .expect("frontend forwarding should complete before its timeout")
        .expect("frontend should forward startup bytes");

    proxy_task.abort_and_join().await;
    let error = timeout(RUN_TIMEOUT, target.read_u8())
        .await
        .expect("target connection should close before its timeout")
        .expect_err("cancelled proxy must close its target connection");
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
}

// Break caught: mapping a lost reset COMMIT acknowledgement to rollback, success, or a generic
// leadership error would let recovery code misclassify an atomically committed reset.
#[tokio::test]
async fn committed_reset_with_lost_acknowledgement_is_indeterminate() {
    assert_reset_fixture(|fixture| {
        Box::pin(async move {
            let positions = fixture.append_reset_values(&[7, 11]).await?;
            let _ = fixture.run_reset_fixture_batch().await?;
            assert_eq!(
                fixture.reset_state().await?,
                ProjectionResetStateObservation {
                    model_total: 18,
                    progress: Some(ProjectionProgressObservation {
                        source_id: fixture.source_id.clone(),
                        selection_id: fixture.selection.id().clone(),
                        position: positions[1],
                    }),
                },
                "the fault must begin from independently observed non-empty projection state",
            );

            fixture.inject_reset_commit_acknowledgement_loss().await?;
            let result = fixture.reset_with_commit_acknowledgement_loss().await?;
            assert!(
                matches!(
                    result,
                    Err(ProjectionResetError::CommitIndeterminate { .. })
                ),
                "a lost reset COMMIT acknowledgement must be indeterminate, got {result:?}",
            );
            assert_eq!(
                fixture.callback_attempts.load(Ordering::SeqCst),
                1,
                "an indeterminate reset commit must not retry application reset code",
            );
            assert_eq!(
                timeout(RUN_TIMEOUT, fixture.fresh_reset_state())
                    .await
                    .map_err(|_| FixtureError::TimedOut)??,
                ProjectionResetStateObservation {
                    model_total: 0,
                    progress: None,
                },
                "a fresh PostgreSQL connection must observe the atomically committed reset",
            );
            Ok(())
        })
    })
    .await;
}

macro_rules! reset_contract_test {
    ($name:ident, $contract:path) => {
        // Every contract uses a unique projector/schema and guarantees bounded cleanup even when
        // an assertion panics; this prevents advisory locks or schemas leaking into later tests.
        #[tokio::test]
        async fn $name() {
            assert_reset_fixture(|fixture| Box::pin($contract(fixture))).await;
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
