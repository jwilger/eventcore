//! Public contract tests for transactional PostgreSQL projections.

use std::convert::Infallible;
use std::env;
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eventcore_postgres::{
    AfterCommit, PostgresProjectionConfig, PostgresProjectionMode, PostgresProjectionSource,
    PostgresProjectionSourceError, PostgresProjectionStore, PostgresProjector,
    ProjectionConfigurationError, ProjectionRetryPolicy, ProjectionRunOutcome,
    TransactionalProjectionError, run_transactional_projection,
};
use eventcore_testing::{
    ProjectionApplicationBehavior, ProjectionAttemptObservation, ProjectionFailureObservation,
    ProjectionHookLogEntry, ProjectionProgressObservation,
    ProjectionRunOutcome as ContractRunOutcome, TransactionalProjectionFixture,
    commit_acknowledgement_loss_contract, malformed_selected_input_contract,
    mutation_failure_rolls_back_contract, progress_failure_rolls_back_contract,
    restart_resumes_from_committed_position_contract, selection_identity_mismatch_contract,
    source_identity_mismatch_contract, transactional_projection_contract,
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
    behavior: ProjectionApplicationBehavior,
    apply_barrier: Option<Arc<Barrier>>,
    application_attempts: Arc<AtomicU64>,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
}

struct RecordingAfterCommit {
    position: DeliveryPosition,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
}

impl AfterCommit for RecordingAfterCommit {
    type Error = Infallible;

    async fn execute(self) -> Result<(), Self::Error> {
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
        if let Some(barrier) = &self.apply_barrier {
            let _ = barrier.wait().await;
        }
        if self.behavior == ProjectionApplicationBehavior::Fail {
            return Err(sqlx::Error::Protocol(
                "fixture application mutation failure".to_owned(),
            ));
        }
        let _ = query("UPDATE invoice_projection_effect SET total = total + 1")
            .execute(&mut **transaction)
            .await?;
        if self.behavior == ProjectionApplicationBehavior::ApplyThenFail {
            return Err(sqlx::Error::Protocol(
                "fixture application mutation failed after applying its effect".to_owned(),
            ));
        }
        Ok(RecordingAfterCommit {
            position,
            hook_log: self.hook_log.clone(),
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
    behavior: ProjectionApplicationBehavior,
    apply_barrier: Option<Arc<Barrier>>,
    application_attempts: Arc<AtomicU64>,
    hook_log: Arc<Mutex<Vec<ProjectionHookLogEntry>>>,
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
            application_attempts: Arc::new(AtomicU64::new(0)),
            hook_log: Arc::new(Mutex::new(Vec::new())),
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
        let store = self
            .commit_acknowledgement_proxy
            .as_ref()
            .map_or_else(|| self.store.clone(), |proxy| proxy.store.clone());
        let projector = IncrementProjector {
            name: self.projector_name.clone(),
            behavior: self.behavior,
            apply_barrier: self.apply_barrier.clone(),
            application_attempts: self.application_attempts.clone(),
            hook_log: self.hook_log.clone(),
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

    async fn run_batch_attempt_with_timeout(
        &self,
    ) -> Result<ProjectionAttemptObservation, FixtureError> {
        let mut config = PostgresProjectionConfig::new(self.selection.clone());
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
        match result {
            Ok(outcome) => Ok(ProjectionAttemptObservation::Completed(convert_outcome(
                outcome,
            ))),
            Err(FixtureError::Runner(error)) => Ok(ProjectionAttemptObservation::Failed(
                classify_runner_error(error),
            )),
            Err(error) => Err(error),
        }
    }
}

fn classify_runner_error(error: TransactionalProjectionError) -> ProjectionFailureObservation {
    match error {
        TransactionalProjectionError::ApplicationFatal { position, .. } => {
            ProjectionFailureObservation::ApplicationFatal { position }
        }
        TransactionalProjectionError::Progress { position, .. } => {
            ProjectionFailureObservation::Progress { position }
        }
        TransactionalProjectionError::Decode { position, .. } => {
            ProjectionFailureObservation::Decode { position }
        }
        TransactionalProjectionError::CommitIndeterminate { position, .. } => {
            ProjectionFailureObservation::CommitIndeterminate { position }
        }
        TransactionalProjectionError::SourceIdentityMismatch { .. } => {
            ProjectionFailureObservation::SourceIdentityMismatch
        }
        TransactionalProjectionError::SelectionIdentityMismatch { .. } => {
            ProjectionFailureObservation::SelectionIdentityMismatch
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
        self.behavior = behavior;
    }

    async fn run_batch(&mut self) -> Result<ContractRunOutcome, Self::Error> {
        Ok(convert_outcome(
            self.run_with_timeout(PostgresProjectionConfig::new(self.selection.clone()))
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
