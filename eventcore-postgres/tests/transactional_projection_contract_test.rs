//! Public contract tests for transactional PostgreSQL projections.

use std::collections::VecDeque;
use std::env;
use std::future::Future;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eventcore_postgres::{
    AfterCommit, PostgresProjectionConfig, PostgresProjectionMode, PostgresProjectionSource,
    PostgresProjectionSourceError, PostgresProjectionStore, PostgresProjector,
    ProjectionConfigurationError, ProjectionFailureContext, ProjectionFailureDecision,
    ProjectionPollSleeper, ProjectionRetryPolicy, ProjectionRetrySleeper, ProjectionRunOutcome,
    TransactionalProjectionError, run_transactional_projection,
};
use eventcore_testing::{
    AFTER_COMMIT_FAILURE_SENTINEL, ProjectionApplicationBehavior, ProjectionAttemptObservation,
    ProjectionFailureObservation, ProjectionFixedHighWaterObservation, ProjectionHookLogEntry,
    ProjectionLeadershipLossObservation, ProjectionLeadershipObservation,
    ProjectionProgressObservation, ProjectionRunOutcome as ContractRunOutcome,
    TransactionalProjectionExecutionFixture, TransactionalProjectionFixture,
    after_commit_failure_contract, after_commit_ordering_and_rollback_contract,
    commit_acknowledgement_loss_contract, empty_source_identity_validation_contract,
    empty_source_selection_identity_validation_contract, explicit_skip_contract,
    exponential_retry_backoff_contract, fatal_leaves_position_pending_contract,
    finite_overflow_retry_backoff_contract, fixed_high_watermark_contract,
    initially_empty_batch_contract, leadership_loss_fencing_contract,
    malformed_selected_input_contract, multi_page_batch_drain_contract,
    mutation_failure_rolls_back_contract, no_match_batch_contract,
    no_match_identity_validation_contract, no_match_selection_identity_validation_contract,
    overlapping_leadership_contract, progress_failure_rolls_back_contract,
    restart_resumes_from_committed_position_contract, retry_exhaustion_contract,
    retry_reloads_progress_contract, selection_identity_mismatch_contract,
    source_identity_mismatch_contract, stop_leaves_position_pending_contract,
    trailing_unselected_frontier_contract, transactional_projection_contract,
    transient_retry_success_contract,
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Barrier, oneshot};
use tokio::task::JoinHandle;
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
    #[error("fixture transport operation failed")]
    Io(#[from] std::io::Error),
    #[error("projection source operation failed")]
    Source(#[from] PostgresProjectionSourceError),
    #[error("transactional projection run failed")]
    Runner(#[from] TransactionalProjectionError),
    #[error("transactional projection run exceeded its fixture timeout")]
    TimedOut,
    #[error("commit acknowledgement proxy failed: {0}")]
    CommitAcknowledgementProxy(String),
    #[error("fixture task failed")]
    Task(#[from] tokio::task::JoinError),
}

struct CommitObservation {
    pool: Pool<Postgres>,
    projector_name: ProjectorName,
    source_id: DeliverySourceId,
    selection_id: ProjectionSelectionId,
    position: DeliveryPosition,
}

struct CommitAcknowledgementProxy {
    store: PostgresProjectionStore,
    confirmation: tokio::sync::Mutex<Option<oneshot::Receiver<Result<(), String>>>>,
    task: JoinHandle<()>,
}

impl CommitAcknowledgementProxy {
    async fn start(
        database: &IsolatedTestDatabase,
        observation: CommitObservation,
    ) -> Result<Self, FixtureError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy_address = listener.local_addr()?;
        let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_string());
        let target = format!("{host}:{port}");
        let (confirmation_sender, confirmation_receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = run_commit_acknowledgement_proxy(listener, target, observation).await;
            let _ = confirmation_sender.send(result);
        });

        let schema = database.schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |connection, _metadata| {
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
            .await?;

        Ok(Self {
            store: PostgresProjectionStore::from_pool(pool),
            confirmation: tokio::sync::Mutex::new(Some(confirmation_receiver)),
            task,
        })
    }

    async fn await_committed_observation(&self) -> Result<(), FixtureError> {
        let receiver = self.confirmation.lock().await.take().ok_or_else(|| {
            FixtureError::CommitAcknowledgementProxy(
                "commit acknowledgement observation was awaited more than once".to_string(),
            )
        })?;
        let result = timeout(RUN_TIMEOUT, receiver)
            .await
            .map_err(|_| FixtureError::TimedOut)?
            .map_err(|_| {
                FixtureError::CommitAcknowledgementProxy(
                    "proxy exited without a commit acknowledgement observation".to_string(),
                )
            })?;
        result.map_err(FixtureError::CommitAcknowledgementProxy)
    }

    async fn shutdown(self) {
        self.task.abort();
        let _ = timeout(RUN_TIMEOUT, self.task).await;
    }
}

async fn run_commit_acknowledgement_proxy(
    listener: TcpListener,
    target: String,
    observation: CommitObservation,
) -> Result<(), String> {
    let (client, _) = listener
        .accept()
        .await
        .map_err(|error| format!("proxy did not accept runner connection: {error}"))?;
    let server = TcpStream::connect(target)
        .await
        .map_err(|error| format!("proxy did not connect to PostgreSQL: {error}"))?;
    let (client_reader, client_writer) = client.into_split();
    let (server_reader, server_writer) = server.into_split();
    let (commit_sender, mut commit_receiver) = oneshot::channel();
    let frontend = tokio::spawn(forward_postgres_frontend(
        client_reader,
        server_writer,
        commit_sender,
    ));

    let result = forward_postgres_backend(
        server_reader,
        client_writer,
        &mut commit_receiver,
        observation,
    )
    .await;
    frontend.abort();
    let _ = frontend.await;
    result
}

async fn forward_postgres_frontend(
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

async fn forward_postgres_backend(
    mut server: tokio::net::tcp::OwnedReadHalf,
    mut client: tokio::net::tcp::OwnedWriteHalf,
    commit_receiver: &mut oneshot::Receiver<()>,
    observation: CommitObservation,
) -> Result<(), String> {
    loop {
        // Do not place this read in `select!`: cancelling after a partial PostgreSQL frame
        // would desynchronize the transport before the next frame is decoded.
        let (tag, length, payload) = read_postgres_message(&mut server).await?;
        if tag == b'C' && payload == b"COMMIT\0" {
            timeout(RUN_TIMEOUT, &mut *commit_receiver)
                .await
                .map_err(|_| {
                    "proxy did not observe forwarded COMMIT before its completion".to_string()
                })?
                .map_err(|_| "proxy frontend ended before forwarding COMMIT".to_string())?;
            timeout(
                RUN_TIMEOUT,
                observe_committed_effect_and_progress(observation),
            )
            .await
            .map_err(|_| {
                "direct commit observer did not complete before its timeout".to_string()
            })??;
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
    let length = length.checked_sub(4).ok_or_else(|| {
        "PostgreSQL protocol frame length was shorter than its header".to_string()
    })?;
    usize::try_from(length)
        .map_err(|_| "PostgreSQL protocol frame length did not fit usize".to_string())
}

async fn observe_committed_effect_and_progress(
    observation: CommitObservation,
) -> Result<(), String> {
    let effect_count = query_scalar::<_, i64>("SELECT total FROM invoice_projection_effect")
        .fetch_one(&observation.pool)
        .await
        .map_err(|error| format!("direct observer could not read committed effect: {error}"))?;
    let position = i64::try_from(observation.position.get())
        .map_err(|error| format!("fixture position did not fit BIGINT: {error}"))?;
    let matching_progress: bool = query_scalar(
        "SELECT EXISTS(SELECT 1 FROM eventcore_projection_progress \
         WHERE projector_name = $1 AND source_id = $2 AND selection_id = $3 AND last_position = $4)",
    )
    .bind(observation.projector_name.as_ref())
    .bind(observation.source_id.as_ref())
    .bind(observation.selection_id.as_ref())
    .bind(position)
    .fetch_one(&observation.pool)
    .await
    .map_err(|error| format!("direct observer could not read committed progress: {error}"))?;
    if effect_count == 1 && matching_progress {
        Ok(())
    } else {
        Err(format!(
            "COMMIT acknowledgement was withheld without observing atomic committed state: effect_count={effect_count}, matching_progress={matching_progress}",
        ))
    }
}

struct IncrementProjector {
    name: ProjectorName,
    behaviors: VecDeque<ProjectionApplicationBehavior>,
    last_failure_decision: ProjectionFailureDecision,
    apply_gate: Option<ApplyGate>,
    application_attempts: Arc<AtomicU64>,
    application_attempt_transactions: Arc<Mutex<Vec<String>>>,
    destination_pool: Pool<Postgres>,
    source_id: DeliverySourceId,
    selection_id: ProjectionSelectionId,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
    hook_attempts: Arc<AtomicU64>,
    leader_pid_sender: Option<oneshot::Sender<i32>>,
}

#[derive(Clone)]
struct ApplyGate {
    entered: Arc<Barrier>,
    release: Option<Arc<Barrier>>,
}

struct RecordingAfterCommit {
    position: DeliveryPosition,
    projector_name: ProjectorName,
    destination_pool: Pool<Postgres>,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
    hook_attempts: Arc<AtomicU64>,
    should_fail: bool,
}

#[derive(Debug, Error)]
enum FixtureAfterCommitError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Sentinel(&'static str),
}

impl AfterCommit for RecordingAfterCommit {
    type Error = FixtureAfterCommitError;

    async fn execute(self) -> Result<(), Self::Error> {
        let _ = self.hook_attempts.fetch_add(1, Ordering::SeqCst);
        let position = i64::try_from(self.position.get())
            .expect("fixture position should fit PostgreSQL BIGINT");
        let committed: bool = query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projection_attempts WHERE position = $1) AND \
             EXISTS(SELECT 1 FROM eventcore_projection_progress \
             WHERE projector_name = $2 AND last_position = $1)",
        )
        .bind(position)
        .bind(self.projector_name.as_ref())
        .fetch_one(&self.destination_pool)
        .await?;
        if !committed {
            return Err(FixtureAfterCommitError::Database(sqlx::Error::Protocol(
                "after-commit action observed uncommitted effect or progress".to_owned(),
            )));
        }
        if self.should_fail {
            return Err(FixtureAfterCommitError::Sentinel(
                AFTER_COMMIT_FAILURE_SENTINEL,
            ));
        }
        self.hook_log
            .lock()
            .expect("fixture hook log mutex should not be poisoned")
            .push(ProjectionHookLogEntry::Committed(self.position));
        Ok(())
    }
}

impl PostgresProjector for IncrementProjector {
    type Event = InvoiceIssued;
    type Error = sqlx::Error;
    type AfterCommit = RecordingAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    async fn apply<'a, 'c>(
        &'a mut self,
        _event: &'a Self::Event,
        position: DeliveryPosition,
        transaction: &'a mut Transaction<'c, Postgres>,
    ) -> Result<Self::AfterCommit, Self::Error>
    where
        'c: 'a,
    {
        let _ = self.application_attempts.fetch_add(1, Ordering::SeqCst);
        if let Some(sender) = self.leader_pid_sender.take() {
            let pid = query_scalar::<_, i32>("SELECT pg_backend_pid()")
                .fetch_one(&mut **transaction)
                .await?;
            let _ = sender.send(pid);
        }
        let transaction_token = query_scalar::<_, String>("SELECT txid_current()::text")
            .fetch_one(&mut **transaction)
            .await?;
        self.application_attempt_transactions
            .lock()
            .expect("fixture transaction-token mutex should not be poisoned")
            .push(transaction_token);
        let behavior = self
            .behaviors
            .pop_front()
            .unwrap_or(ProjectionApplicationBehavior::Apply);
        if let Some(gate) = &self.apply_gate {
            let _ = gate.entered.wait().await;
            if let Some(release) = &gate.release {
                let _ = release.wait().await;
            }
        }
        if behavior == ProjectionApplicationBehavior::Fail {
            return Err(sqlx::Error::Protocol(
                "fixture application mutation failure".to_owned(),
            ));
        }
        let numeric_position =
            i64::try_from(position.get()).expect("fixture position should fit PostgreSQL BIGINT");
        let _ = query("INSERT INTO projection_attempts (position) VALUES ($1)")
            .bind(numeric_position)
            .execute(&mut **transaction)
            .await?;
        let _ = query("UPDATE invoice_projection_effect SET total = total + 1")
            .execute(&mut **transaction)
            .await?;
        if behavior == ProjectionApplicationBehavior::RetryWithExternallyCommittedProgress {
            let _ = query(
                "INSERT INTO eventcore_projection_progress \
                 (projector_name, source_id, selection_id, last_position) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (projector_name) DO UPDATE \
                 SET source_id = EXCLUDED.source_id, selection_id = EXCLUDED.selection_id, \
                 last_position = EXCLUDED.last_position, updated_at = NOW()",
            )
            .bind(self.name.as_ref())
            .bind(self.source_id.as_ref())
            .bind(self.selection_id.as_ref())
            .bind(numeric_position)
            .execute(&self.destination_pool)
            .await?;
        }
        self.last_failure_decision = match behavior {
            ProjectionApplicationBehavior::Retry
            | ProjectionApplicationBehavior::RetryThenApply
            | ProjectionApplicationBehavior::RetryWithExternallyCommittedProgress => {
                ProjectionFailureDecision::Retry
            }
            ProjectionApplicationBehavior::Skip => ProjectionFailureDecision::Skip,
            ProjectionApplicationBehavior::Stop => ProjectionFailureDecision::Stop,
            _ => ProjectionFailureDecision::Fatal,
        };
        if matches!(
            behavior,
            ProjectionApplicationBehavior::ApplyThenFail
                | ProjectionApplicationBehavior::Retry
                | ProjectionApplicationBehavior::RetryThenApply
                | ProjectionApplicationBehavior::RetryWithExternallyCommittedProgress
                | ProjectionApplicationBehavior::Skip
                | ProjectionApplicationBehavior::Stop
                | ProjectionApplicationBehavior::Fatal
        ) {
            return Err(sqlx::Error::Protocol(
                "fixture application mutation failed after applying its effect".to_owned(),
            ));
        }
        Ok(RecordingAfterCommit {
            position,
            projector_name: self.name.clone(),
            destination_pool: self.destination_pool.clone(),
            hook_log: self.hook_log.clone(),
            hook_attempts: self.hook_attempts.clone(),
            should_fail: behavior == ProjectionApplicationBehavior::AfterCommitFail,
        })
    }

    fn on_error(
        &mut self,
        _failure: ProjectionFailureContext<'_, Self::Error>,
    ) -> ProjectionFailureDecision {
        self.last_failure_decision
    }
}

#[derive(Debug, Clone)]
struct RecordingRetrySleeper {
    requests: Arc<Mutex<Vec<Duration>>>,
}

#[derive(Debug, Clone)]
struct RecordingPollSleeper {
    requests: Arc<Mutex<Vec<Duration>>>,
}

impl ProjectionPollSleeper for RecordingPollSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let requests = self.requests.clone();
        Box::pin(async move {
            requests
                .lock()
                .expect("fixture poll-sleep mutex should not be poisoned")
                .push(duration);
        })
    }
}

impl ProjectionRetrySleeper for RecordingRetrySleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let requests = self.requests.clone();
        Box::pin(async move {
            requests
                .lock()
                .expect("fixture retry-sleep mutex should not be poisoned")
                .push(duration);
        })
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
    behaviors: VecDeque<ProjectionApplicationBehavior>,
    retry_policy: ProjectionRetryPolicy,
    batch_size: BatchSize,
    apply_gate: Option<ApplyGate>,
    application_attempts: Arc<AtomicU64>,
    application_attempt_transactions: Arc<Mutex<Vec<String>>>,
    retry_sleep_requests: Arc<Mutex<Vec<Duration>>>,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
    hook_attempts: Arc<AtomicU64>,
    commit_acknowledgement_proxy: Option<CommitAcknowledgementProxy>,
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
        let _ = query("CREATE TABLE projection_attempts (position BIGINT NOT NULL)")
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
            behaviors: VecDeque::from([ProjectionApplicationBehavior::Apply]),
            retry_policy: ProjectionRetryPolicy::new(
                0,
                Duration::from_millis(100),
                2.0,
                Duration::from_secs(30),
            )
            .expect("default fixture retry policy should be valid"),
            batch_size: BatchSize::new(100),
            apply_gate: None,
            application_attempts: Arc::new(AtomicU64::new(0)),
            application_attempt_transactions: Arc::new(Mutex::new(Vec::new())),
            retry_sleep_requests: Arc::new(Mutex::new(Vec::new())),
            hook_log: Arc::new(Mutex::new(Vec::new())),
            hook_attempts: Arc::new(AtomicU64::new(0)),
            commit_acknowledgement_proxy: None,
        })
    }

    async fn cleanup(self) -> Result<(), FixtureError> {
        let Self {
            database,
            event_store,
            source,
            store,
            commit_acknowledgement_proxy,
            ..
        } = self;
        if let Some(proxy) = commit_acknowledgement_proxy {
            proxy.shutdown().await;
        }
        drop(event_store);
        drop(source);
        drop(store);
        database.cleanup().await?;
        Ok(())
    }

    fn with_apply_barrier(&mut self, barrier: Arc<Barrier>) {
        self.apply_gate = Some(ApplyGate {
            entered: barrier,
            release: None,
        });
    }

    fn clear_apply_barrier(&mut self) {
        self.apply_gate = None;
    }

    fn projector(
        &self,
        apply_gate: Option<ApplyGate>,
        leader_pid_sender: Option<oneshot::Sender<i32>>,
    ) -> IncrementProjector {
        IncrementProjector {
            name: self.projector_name.clone(),
            behaviors: self.behaviors.clone(),
            last_failure_decision: ProjectionFailureDecision::Fatal,
            apply_gate,
            application_attempts: self.application_attempts.clone(),
            application_attempt_transactions: self.application_attempt_transactions.clone(),
            destination_pool: self.database.clone_pool(),
            source_id: self.source_id.clone(),
            selection_id: self.selection.id().clone(),
            hook_log: self.hook_log.clone(),
            hook_attempts: self.hook_attempts.clone(),
            leader_pid_sender,
        }
    }

    async fn run_with_timeout(
        &self,
        config: PostgresProjectionConfig,
    ) -> Result<ProjectionRunOutcome, FixtureError> {
        let config = config.with_retry_sleeper(RecordingRetrySleeper {
            requests: self.retry_sleep_requests.clone(),
        });
        let source = self.source.clone();
        let store = self
            .commit_acknowledgement_proxy
            .as_ref()
            .map_or_else(|| self.store.clone(), |proxy| proxy.store.clone());
        let projector = self.projector(self.apply_gate.clone(), None);
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

    async fn run_batch_attempt_with_timeout(
        &self,
    ) -> Result<ProjectionAttemptObservation, FixtureError> {
        let mut config = PostgresProjectionConfig::new(self.selection.clone())
            .with_batch_size(self.batch_size)
            .with_retry_policy(self.retry_policy.clone());
        if self.commit_acknowledgement_proxy.is_some() {
            config = config.with_retry_policy(
                ProjectionRetryPolicy::new(
                    1,
                    Duration::from_millis(1),
                    1.0,
                    Duration::from_millis(1),
                )
                .expect("fixture retry policy should be valid"),
            );
        }
        let result = self.run_with_timeout(config).await;
        if let Some(proxy) = &self.commit_acknowledgement_proxy {
            proxy.await_committed_observation().await?;
        }
        let requested_behavior = self.behaviors.front().copied();
        match result {
            Ok(outcome) => Ok(ProjectionAttemptObservation::Completed(convert_outcome(
                outcome,
            ))),
            Err(FixtureError::Runner(error)) => Ok(ProjectionAttemptObservation::Failed(
                classify_runner_error(error, requested_behavior),
            )),
            Err(error) => Err(error),
        }
    }
}

fn classify_runner_error(
    error: TransactionalProjectionError,
    requested_behavior: Option<ProjectionApplicationBehavior>,
) -> ProjectionFailureObservation {
    match error {
        TransactionalProjectionError::Application { position, .. } => {
            if requested_behavior == Some(ProjectionApplicationBehavior::Fatal) {
                ProjectionFailureObservation::Application { position }
            } else {
                ProjectionFailureObservation::ApplicationFatal { position }
            }
        }
        TransactionalProjectionError::RetryExhausted {
            position, attempts, ..
        } => ProjectionFailureObservation::RetryExhausted { position, attempts },
        TransactionalProjectionError::Progress { position, .. } => {
            ProjectionFailureObservation::Progress { position }
        }
        TransactionalProjectionError::Decode { position, .. } => {
            ProjectionFailureObservation::Decode { position }
        }
        TransactionalProjectionError::CommitIndeterminate { position, .. } => {
            ProjectionFailureObservation::CommitIndeterminate { position }
        }
        TransactionalProjectionError::AfterCommitFailed {
            committed_position,
            source,
        } => ProjectionFailureObservation::AfterCommitFailed {
            committed_position,
            source: source.to_string(),
        },
        TransactionalProjectionError::SourceIdentityMismatch { .. } => {
            ProjectionFailureObservation::SourceIdentityMismatch
        }
        TransactionalProjectionError::SelectionIdentityMismatch { .. } => {
            ProjectionFailureObservation::SelectionIdentityMismatch
        }
        TransactionalProjectionError::LeadershipBusy => {
            ProjectionFailureObservation::LeadershipBusy
        }
        TransactionalProjectionError::LeadershipLost { .. } => {
            ProjectionFailureObservation::LeadershipLost
        }
        _ => ProjectionFailureObservation::Other,
    }
}

impl TransactionalProjectionFixture for PostgresAtomicFixture {
    type Error = FixtureError;

    async fn append_values(
        &mut self,
        values: &[serde_json::Value],
    ) -> Result<Vec<DeliveryPosition>, Self::Error> {
        let before = self.source.high_watermark().await?;
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
                before,
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

    async fn append_malformed_input(
        &mut self,
        input: &str,
    ) -> Result<DeliveryPosition, Self::Error> {
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
        self.source.high_watermark().await?.ok_or_else(|| {
            FixtureError::Sql(sqlx::Error::Protocol(
                "malformed input was not assigned a delivery position".to_owned(),
            ))
        })
    }

    fn select_application_behavior(&mut self, behavior: ProjectionApplicationBehavior) {
        self.behaviors = match behavior {
            ProjectionApplicationBehavior::Retry => VecDeque::from([
                ProjectionApplicationBehavior::Retry,
                ProjectionApplicationBehavior::Retry,
                ProjectionApplicationBehavior::Retry,
            ]),
            ProjectionApplicationBehavior::RetryThenApply => VecDeque::from([
                ProjectionApplicationBehavior::RetryThenApply,
                ProjectionApplicationBehavior::Apply,
            ]),
            behavior => VecDeque::from([behavior]),
        };
    }

    fn select_application_script(&mut self, behaviors: &[ProjectionApplicationBehavior]) {
        self.behaviors = behaviors.iter().copied().collect();
    }

    fn configure_retry_policy(
        &mut self,
        max_retries: u32,
        initial_delay: Duration,
        multiplier: f64,
        maximum_delay: Duration,
    ) {
        self.retry_policy =
            ProjectionRetryPolicy::new(max_retries, initial_delay, multiplier, maximum_delay)
                .expect("contract retry policy should be valid");
    }

    async fn run_batch(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        Ok(convert_outcome(
            self.run_with_timeout(
                PostgresProjectionConfig::new(self.selection.clone())
                    .with_batch_size(self.batch_size)
                    .with_retry_policy(self.retry_policy.clone()),
            )
            .await?,
        ))
    }

    async fn run_batch_attempt(&mut self) -> Result<ProjectionAttemptObservation, Self::Error> {
        self.run_batch_attempt_with_timeout().await
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
             LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture progress failure'; END; $$",
        )
        .execute(self.database.pool())
        .await?;
        let _ = query(
            "CREATE TRIGGER fixture_fail_projection_progress BEFORE INSERT OR UPDATE ON \
             eventcore_projection_progress FOR EACH ROW EXECUTE FUNCTION fixture_fail_projection_progress()",
        )
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    async fn inject_commit_acknowledgement_loss(&mut self) -> Result<(), Self::Error> {
        let position = self
            .source
            .high_watermark()
            .await?
            .expect("commit acknowledgement fault requires a pending event");
        let proxy = CommitAcknowledgementProxy::start(
            &self.database,
            CommitObservation {
                pool: self.database.clone_pool(),
                projector_name: self.projector_name.clone(),
                source_id: self.source_id.clone(),
                selection_id: self.selection.id().clone(),
                position,
            },
        )
        .await?;
        self.commit_acknowledgement_proxy = Some(proxy);
        Ok(())
    }

    async fn recover_after_commit_acknowledgement_loss(
        &mut self,
    ) -> Result<ContractRunOutcome, Self::Error> {
        if let Some(proxy) = self.commit_acknowledgement_proxy.take() {
            proxy.shutdown().await;
        }
        self.run_batch().await
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

    async fn application_attempt_count(&self) -> Result<u64, Self::Error> {
        Ok(self.application_attempts.load(Ordering::SeqCst))
    }

    async fn application_attempt_transaction_tokens(&self) -> Result<Vec<String>, Self::Error> {
        Ok(self
            .application_attempt_transactions
            .lock()
            .expect("fixture transaction-token mutex should not be poisoned")
            .clone())
    }

    async fn retry_sleep_requests(&self) -> Result<Vec<Duration>, Self::Error> {
        Ok(self
            .retry_sleep_requests
            .lock()
            .expect("fixture retry-sleep mutex should not be poisoned")
            .clone())
    }

    async fn transaction_attempt_row_count(&self) -> Result<u64, Self::Error> {
        let count = query_scalar::<_, i64>("SELECT COUNT(*) FROM projection_attempts")
            .fetch_one(self.database.pool())
            .await?;
        Ok(u64::try_from(count).expect("fixture attempt row count should be nonnegative"))
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
        Ok(self
            .hook_log
            .lock()
            .expect("fixture hook log mutex should not be poisoned")
            .clone())
    }

    async fn hook_attempt_count(&self) -> Result<u64, Self::Error> {
        Ok(self.hook_attempts.load(Ordering::SeqCst))
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

    async fn seed_progress_identity(
        &mut self,
        source_id: DeliverySourceId,
        selection_id: ProjectionSelectionId,
        position: DeliveryPosition,
    ) -> Result<(), Self::Error> {
        let position = i64::try_from(position.get()).expect("fixture position should fit BIGINT");
        let _ = query(
            "INSERT INTO eventcore_projection_progress \
             (projector_name, source_id, selection_id, last_position) VALUES ($1, $2, $3, $4)",
        )
        .bind(self.projector_name.as_ref())
        .bind(source_id.as_ref())
        .bind(selection_id.as_ref())
        .bind(position)
        .execute(self.database.pool())
        .await?;
        Ok(())
    }
}

impl TransactionalProjectionExecutionFixture for PostgresAtomicFixture {
    fn configure_batch_size(&mut self, batch_size: BatchSize) {
        self.batch_size = batch_size;
    }

    async fn append_unselected(&mut self) -> Result<DeliveryPosition, Self::Error> {
        let stream_id = StreamId::try_new(format!("invoice::unselected::{}", Uuid::now_v7()))
            .expect("fixture stream ID should be valid");
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))?
            .append(UnselectedEvent { stream_id })?;
        let _ = self.event_store.append_events(writes).await?;
        self.source.high_watermark().await?.ok_or_else(|| {
            FixtureError::Sql(sqlx::Error::Protocol(
                "unselected append did not advance the global frontier".to_owned(),
            ))
        })
    }

    async fn observe_overlapping_leadership(
        &mut self,
    ) -> Result<ProjectionLeadershipObservation, Self::Error> {
        let _ = self.append_values(&[serde_json::json!({})]).await?;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let source = self.source.clone();
        let store = self.store.clone();
        let projector = self.projector(
            Some(ApplyGate {
                entered: entered.clone(),
                release: Some(release.clone()),
            }),
            None,
        );
        let config = PostgresProjectionConfig::new(self.selection.clone());
        let mut leader_task = tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        });

        if timeout(RUN_TIMEOUT, entered.wait()).await.is_err() {
            abort_and_join(&mut leader_task).await;
            return Err(FixtureError::TimedOut);
        }
        let competing = timeout(
            RUN_TIMEOUT,
            run_transactional_projection(
                self.projector(None, None),
                &self.source,
                &self.store,
                PostgresProjectionConfig::new(self.selection.clone()),
            ),
        )
        .await;
        if timeout(RUN_TIMEOUT, release.wait()).await.is_err() {
            abort_and_join(&mut leader_task).await;
            return Err(FixtureError::TimedOut);
        }
        let leader_result = join_with_timeout(&mut leader_task).await?;
        let competing_failure = match competing {
            Ok(Err(error)) => classify_runner_error(error, None),
            Ok(Ok(_)) => ProjectionFailureObservation::Other,
            Err(_) => return Err(FixtureError::TimedOut),
        };

        Ok(ProjectionLeadershipObservation {
            competing_failure,
            leader_outcome: convert_outcome(leader_result?),
        })
    }

    async fn observe_leadership_loss(
        &mut self,
    ) -> Result<ProjectionLeadershipLossObservation, Self::Error> {
        let _ = self.append_values(&[serde_json::json!({})]).await?;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (pid_sender, pid_receiver) = oneshot::channel();
        let source = self.source.clone();
        let store = self.store.clone();
        let projector = self.projector(
            Some(ApplyGate {
                entered: entered.clone(),
                release: Some(release.clone()),
            }),
            Some(pid_sender),
        );
        let config = PostgresProjectionConfig::new(self.selection.clone());
        let mut leader_task = tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        });

        let pid = match timeout(RUN_TIMEOUT, pid_receiver).await {
            Ok(Ok(pid)) => pid,
            Ok(Err(_)) | Err(_) => {
                abort_and_join(&mut leader_task).await;
                return Err(FixtureError::TimedOut);
            }
        };
        if timeout(RUN_TIMEOUT, entered.wait()).await.is_err() {
            abort_and_join(&mut leader_task).await;
            return Err(FixtureError::TimedOut);
        }
        let terminated: bool = match query_scalar("SELECT pg_terminate_backend($1)")
            .bind(pid)
            .fetch_one(self.database.pool())
            .await
        {
            Ok(terminated) => terminated,
            Err(error) => {
                abort_and_join(&mut leader_task).await;
                return Err(error.into());
            }
        };
        if !terminated {
            abort_and_join(&mut leader_task).await;
            return Err(FixtureError::Sql(sqlx::Error::Protocol(
                "fixture did not terminate the exact signalled leader PID".to_owned(),
            )));
        }
        if timeout(RUN_TIMEOUT, release.wait()).await.is_err() {
            abort_and_join(&mut leader_task).await;
            return Err(FixtureError::TimedOut);
        }
        let result = join_with_timeout(&mut leader_task).await?;
        let failure = match result {
            Err(error) => classify_runner_error(error, None),
            Ok(_) => ProjectionFailureObservation::Other,
        };

        Ok(ProjectionLeadershipLossObservation {
            failure,
            effect_count: self.effect_count().await?,
            progress: self.progress().await?,
        })
    }

    async fn observe_fixed_high_watermark(
        &mut self,
    ) -> Result<ProjectionFixedHighWaterObservation, Self::Error> {
        let initial_position = self.append_values(&[serde_json::json!({})]).await?[0];
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let source = self.source.clone();
        let store = self.store.clone();
        let projector = self.projector(
            Some(ApplyGate {
                entered: entered.clone(),
                release: Some(release.clone()),
            }),
            None,
        );
        let config = PostgresProjectionConfig::new(self.selection.clone());
        let mut first_task = tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        });

        if timeout(RUN_TIMEOUT, entered.wait()).await.is_err() {
            abort_and_join(&mut first_task).await;
            return Err(FixtureError::TimedOut);
        }
        let appended_position = match self.append_values(&[serde_json::json!({})]).await {
            Ok(positions) => positions[0],
            Err(error) => {
                abort_and_join(&mut first_task).await;
                return Err(error);
            }
        };
        if timeout(RUN_TIMEOUT, release.wait()).await.is_err() {
            abort_and_join(&mut first_task).await;
            return Err(FixtureError::TimedOut);
        }
        let first_outcome = join_with_timeout(&mut first_task).await??;
        let progress_after_first = self.progress().await?;
        let second_outcome = self.run_batch().await?;

        Ok(ProjectionFixedHighWaterObservation {
            initial_position,
            appended_position,
            first_outcome: convert_outcome(first_outcome),
            progress_after_first,
            second_outcome,
        })
    }
}

async fn join_with_timeout<T>(task: &mut JoinHandle<T>) -> Result<T, FixtureError> {
    match timeout(RUN_TIMEOUT, &mut *task).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            abort_and_join(task).await;
            Err(FixtureError::TimedOut)
        }
    }
}

async fn abort_and_join<T>(task: &mut JoinHandle<T>) {
    task.abort();
    let _ = timeout(RUN_TIMEOUT, &mut *task).await;
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

type FixtureContractFuture<'a> = Pin<Box<dyn Future<Output = Result<(), FixtureError>> + 'a>>;

async fn assert_fixture_contract(
    contract: impl for<'a> FnOnce(&'a mut PostgresAtomicFixture) -> FixtureContractFuture<'a>,
) {
    let mut fixture = PostgresAtomicFixture::new()
        .await
        .expect("fixture should initialize");
    let result = AssertUnwindSafe(contract(&mut fixture))
        .catch_unwind()
        .await;
    let cleanup = fixture.cleanup().await;
    cleanup.expect("test schema cleanup should succeed even after contract panic");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("contract fixture setup must not fail: {error}"),
        Err(payload) => resume_unwind(payload),
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
    assert_eq!(
        ProjectionRetryPolicy::new(u32::MAX, Duration::ZERO, 1.0, Duration::ZERO),
        Err(ProjectionConfigurationError::TooManyRetries),
    );
}

// Break caught: recording at future construction, dropping the configured sleeper during config
// cloning, or bypassing the public accessor would make awaited retry-delay observation untruthful.
#[tokio::test]
async fn retry_sleeper_builder_and_clone_route_requested_durations() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let config =
        PostgresProjectionConfig::new(selection()).with_retry_sleeper(RecordingRetrySleeper {
            requests: requests.clone(),
        });
    let cloned = config.clone();

    drop(config.retry_sleeper().sleep(Duration::from_millis(11)));
    assert!(
        requests
            .lock()
            .expect("fixture retry-sleep mutex should not be poisoned")
            .is_empty(),
        "constructing and dropping an unpolled sleep future must not record a delay",
    );
    config
        .retry_sleeper()
        .sleep(Duration::from_millis(17))
        .await;
    cloned
        .retry_sleeper()
        .sleep(Duration::from_millis(29))
        .await;

    assert_eq!(
        *requests
            .lock()
            .expect("fixture retry-sleep mutex should not be poisoned"),
        vec![Duration::from_millis(17), Duration::from_millis(29)],
    );
    assert!(format!("{config:?}").contains("RecordingRetrySleeper"));
}

// Break caught: bypassing or losing the configured poll sleeper would make continuous-mode idle
// waits unobservable and prevent deterministic no-busy-loop contract tests.
#[tokio::test]
async fn poll_sleeper_builder_and_clone_route_requested_durations() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let config =
        PostgresProjectionConfig::new(selection()).with_poll_sleeper(RecordingPollSleeper {
            requests: requests.clone(),
        });
    let cloned = config.clone();

    drop(config.poll_sleeper().sleep(Duration::from_millis(11)));
    assert!(
        requests
            .lock()
            .expect("fixture poll-sleep mutex should not be poisoned")
            .is_empty(),
        "constructing and dropping an unpolled sleep future must not record a wait",
    );
    config.poll_sleeper().sleep(Duration::from_millis(17)).await;
    cloned.poll_sleeper().sleep(Duration::from_millis(29)).await;

    assert_eq!(
        *requests
            .lock()
            .expect("fixture poll-sleep mutex should not be poisoned"),
        vec![Duration::from_millis(17), Duration::from_millis(29)],
    );
    assert!(format!("{config:?}").contains("RecordingPollSleeper"));
}

// Break caught: hardcoding the fixture sentinel in the adapter classifier would discard the
// actual application hook source and make distinct operational failures indistinguishable.
#[test]
fn after_commit_error_classifier_forwards_distinct_sources() {
    let position = DeliveryPosition::new(NonZeroU64::new(7).expect("positive test position"));
    for source_message in ["hook source alpha", "hook source beta"] {
        assert_eq!(
            classify_runner_error(
                TransactionalProjectionError::AfterCommitFailed {
                    committed_position: position,
                    source: Box::new(std::io::Error::other(source_message)),
                },
                Some(ProjectionApplicationBehavior::AfterCommitFail),
            ),
            ProjectionFailureObservation::AfterCommitFailed {
                committed_position: position,
                source: source_message.to_owned(),
            },
        );
    }
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
    assert_fixture_contract(|fixture| Box::pin(transactional_projection_contract(fixture))).await;
}

// Break caught: applying a non-idempotent read-model mutation outside the runner transaction
// would make an application error visible even though its position remains pending.
#[tokio::test]
async fn application_mutation_failure_rolls_back_effect_and_progress() {
    assert_fixture_contract(|fixture| Box::pin(mutation_failure_rolls_back_contract(fixture)))
        .await;
}

// Break caught: persisting progress through another connection, or committing the mutation first,
// would leave a non-idempotent effect visible when the progress write fails.
#[tokio::test]
async fn progress_failure_rolls_back_effect_and_progress() {
    assert_fixture_contract(|fixture| Box::pin(progress_failure_rolls_back_contract(fixture)))
        .await;
}

// Break caught: restarting from a source cursor instead of durable destination progress would
// reapply the first non-idempotent event before it reaches the newly appended event.
#[tokio::test]
async fn restart_resumes_from_last_committed_position() {
    assert_fixture_contract(|fixture| {
        Box::pin(restart_resumes_from_committed_position_contract(fixture))
    })
    .await;
}

// Break caught: dropping a selected decode failure or advancing past it would permanently lose
// malformed persisted input instead of returning a typed error at the pending position.
#[tokio::test]
async fn malformed_selected_input_returns_decode_and_does_not_advance() {
    assert_fixture_contract(|fixture| Box::pin(malformed_selected_input_contract(fixture))).await;
}

// Break caught: collapsing identity violations loses the operator-visible distinction between a
// new source and a changed selection, and either mismatch must stop before application code.
#[tokio::test]
async fn source_identity_mismatch_stops_before_application_code() {
    assert_fixture_contract(|fixture| Box::pin(source_identity_mismatch_contract(fixture))).await;
}

// Break caught: collapsing identity violations loses the operator-visible distinction between a
// changed selection and a new source, and either mismatch must stop before application code.
#[tokio::test]
async fn selection_identity_mismatch_stops_before_application_code() {
    assert_fixture_contract(|fixture| Box::pin(selection_identity_mismatch_contract(fixture)))
        .await;
}

// Break caught: treating a failed commit acknowledgement as a proven rollback can cause an
// in-memory retry or an after-commit action to duplicate work after PostgreSQL committed it.
#[tokio::test]
async fn commit_acknowledgement_loss_is_indeterminate_without_after_commit_or_retry() {
    assert_fixture_contract(|fixture| Box::pin(commit_acknowledgement_loss_contract(fixture)))
        .await;
}

// Break caught: an unbounded retry loop, an off-by-one retry count, or retaining a failed
// transaction would exceed the configured attempts or expose its transaction-scoped rows.
#[tokio::test]
async fn retry_exhaustion_is_bounded_and_rolls_back_every_attempt() {
    assert_fixture_contract(|fixture| Box::pin(retry_exhaustion_contract(fixture))).await;
}

// Break caught: retrying in the failed transaction, or ignoring the maximum-delay cap, would
// retain both attempt rows or exceed the fixture's bound before the successful application.
#[tokio::test]
async fn transient_retry_uses_a_fresh_transaction_and_capped_delay() {
    assert_fixture_contract(|fixture| Box::pin(transient_retry_success_contract(fixture))).await;
}

// Break caught: returning `initial.min(maximum)` for every retry would flatten exponential
// backoff instead of requesting 10 ms, 20 ms, then the capped 25 ms.
#[tokio::test]
async fn retry_backoff_grows_exponentially_then_caps_at_the_maximum() {
    assert_fixture_contract(|fixture| Box::pin(exponential_retry_backoff_contract(fixture))).await;
}

// Break caught: multiplying a finite policy into infinity without saturating can panic, hang, or
// pass a non-representable duration instead of capping later retries at 25 ms.
#[tokio::test]
async fn retry_backoff_saturates_finite_multiplier_overflow_without_panicking() {
    assert_fixture_contract(|fixture| Box::pin(finite_overflow_retry_backoff_contract(fixture)))
        .await;
}

// Break caught: reapplying from an in-memory cursor without reloading durable progress would
// duplicate application work after another invocation committed the pending position.
#[tokio::test]
async fn retry_reloads_progress_before_reapplying() {
    assert_fixture_contract(|fixture| Box::pin(retry_reloads_progress_contract(fixture))).await;
}

// Break caught: committing the failed application transaction, or counting skip as processed,
// would expose its mutation instead of advancing only progress in a fresh transaction.
#[tokio::test]
async fn skip_rolls_back_application_work_and_advances_only_progress() {
    assert_fixture_contract(|fixture| Box::pin(explicit_skip_contract(fixture))).await;
}

// Break caught: advancing the stopped position or losing prior run counters would make a caller
// unable to resume the exact pending event with truthful processed and skipped totals.
#[tokio::test]
async fn stop_leaves_exact_position_pending_with_prior_counts() {
    assert_fixture_contract(|fixture| Box::pin(stop_leaves_position_pending_contract(fixture)))
        .await;
}

// Break caught: treating fatal as retry, skip, or stop would lose its typed application failure
// or advance durable progress for a transaction that must remain pending.
#[tokio::test]
async fn fatal_returns_application_failure_without_progress_or_hook() {
    assert_fixture_contract(|fixture| Box::pin(fatal_leaves_position_pending_contract(fixture)))
        .await;
}

// Break caught: invoking the hook before commit or on a rollback path would let it observe
// missing durable state or run for an event whose application transaction failed.
#[tokio::test]
async fn after_commit_observes_committed_state_and_is_suppressed_on_rollback() {
    assert_fixture_contract(|fixture| {
        Box::pin(after_commit_ordering_and_rollback_contract(fixture))
    })
    .await;
}

// Break caught: reapplying an event or replaying a failed in-process hook after its transaction
// committed would duplicate non-idempotent application or notification work.
#[tokio::test]
async fn after_commit_failure_reports_committed_position_without_replay() {
    assert_fixture_contract(|fixture| Box::pin(after_commit_failure_contract(fixture))).await;
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

// Break caught: stopping after one non-empty page would strand selected events behind the page
// boundary while falsely reporting the captured frontier as caught up.
#[tokio::test]
async fn batch_drains_more_than_one_page() {
    assert_fixture_contract(|fixture| Box::pin(multi_page_batch_drain_contract(fixture))).await;
}

// Break caught: treating a missing source frontier as an error or manufacturing progress would
// prevent a newly deployed projection from completing cleanly before its first event.
#[tokio::test]
async fn batch_catches_up_when_source_is_initially_empty() {
    assert_fixture_contract(|fixture| Box::pin(initially_empty_batch_contract(fixture))).await;
}

// Break caught: requiring a selected event to certify completion would hang when the global
// source contains events but none satisfy the projection selection.
#[tokio::test]
async fn batch_catches_up_when_selection_matches_nothing() {
    assert_fixture_contract(|fixture| Box::pin(no_match_batch_contract(fixture))).await;
}

// Break caught: requiring durable progress to equal the global frontier would keep polling when
// a selected event is followed by an unselected event in the captured finite range.
#[tokio::test]
async fn batch_catches_up_through_a_trailing_unselected_frontier() {
    assert_fixture_contract(|fixture| Box::pin(trailing_unselected_frontier_contract(fixture)))
        .await;
}

// Break caught: returning early on an empty source before loading durable progress silently
// reuses a cursor bound to a different source and selection.
#[tokio::test]
async fn initially_empty_batch_validates_saved_identity_with_source_precedence() {
    assert_fixture_contract(|fixture| Box::pin(empty_source_identity_validation_contract(fixture)))
        .await;
}

// Break caught: returning early on an empty source before validating selection identity silently
// accepts a changed projection definition when its source identity is unchanged.
#[tokio::test]
async fn initially_empty_batch_validates_selection_only_mismatch() {
    assert_fixture_contract(|fixture| {
        Box::pin(empty_source_selection_identity_validation_contract(fixture))
    })
    .await;
}

// Break caught: validating identity only while applying an envelope silently accepts incompatible
// saved progress whenever a non-empty global frontier yields an empty selected page.
#[tokio::test]
async fn no_match_batch_validates_saved_identity_with_source_precedence() {
    assert_fixture_contract(|fixture| Box::pin(no_match_identity_validation_contract(fixture)))
        .await;
}

// Break caught: validating selection identity only while applying an event accepts a changed
// selection whenever the captured global range contains no selected envelope.
#[tokio::test]
async fn no_match_batch_validates_selection_only_mismatch() {
    assert_fixture_contract(|fixture| {
        Box::pin(no_match_selection_identity_validation_contract(fixture))
    })
    .await;
}

// Break caught: releasing leadership before the event transaction finishes permits two writers
// to race the same non-idempotent read model for one projector identity.
#[tokio::test]
async fn leadership_rejects_overlapping_second_writer_while_leader_is_active() {
    assert_fixture_contract(|fixture| Box::pin(overlapping_leadership_contract(fixture))).await;
}

// Break caught: classifying a terminated leader connection as recoverable progress failure, or
// writing through another pooled connection, lets a stale runner mutate effect or progress.
#[tokio::test]
async fn leadership_loss_of_exact_backend_fences_all_stale_writes() {
    assert_fixture_contract(|fixture| Box::pin(leadership_loss_fencing_contract(fixture))).await;
}

// Break caught: refreshing the finite frontier between pages would consume events appended after
// the run began and could prevent batch mode from ever terminating under sustained writes.
#[tokio::test]
async fn batch_captures_high_watermark_once_despite_concurrent_append() {
    assert_fixture_contract(|fixture| Box::pin(fixed_high_watermark_contract(fixture))).await;
}
