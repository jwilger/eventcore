//! Continuous transactional PostgreSQL projection contract tests.

use std::convert::Infallible;
use std::env;
use std::future::Future;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eventcore_postgres::{
    NoopAfterCommit, PostgresProjectionConfig, PostgresProjectionStore, PostgresProjector,
    ProjectionPollSleeper, ProjectionRunOutcome, TransactionalProjectionError,
    run_transactional_projection,
};
use eventcore_testing::{
    ProjectionContinuousObservation, ProjectionIdleBoundaryObservation,
    ProjectionProgressObservation, ProjectionRunOutcome as ContractRunOutcome,
    TransactionalProjectionContinuousFixture, continuous_delivery_contract,
    continuous_idle_cancellation_contract,
};
use eventcore_types::{
    BatchSize, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, EventTypeName,
    PersistedEventEnvelope, PersistedEventId, ProjectionSelection, ProjectionSelectionId,
    ProjectionSource, ProjectionStreamFilter, ProjectorName, StreamId, StreamVersion,
};
use futures::FutureExt;
use serde::Deserialize;
use serde_json::value::RawValue;
use sqlx::{Pool, Postgres, Transaction, postgres::PgPoolOptions, query, query_scalar};
use thiserror::Error;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const RUN_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(17);

#[derive(Debug, Error)]
enum FixtureError {
    #[error("postgres operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("transactional projection run failed")]
    Runner(#[from] TransactionalProjectionError),
    #[error("fixture task failed")]
    Task(#[from] tokio::task::JoinError),
    #[error("continuous projection fixture timed out")]
    TimedOut,
    #[error("continuous poll observation channel closed before the runner completed")]
    PollObservationClosed,
    #[error("continuous idle-boundary observation failed: {0}")]
    IdleObservation(String),
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
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await?;
        let schema = format!("eventcore_continuous_test_{}", Uuid::now_v7().simple());
        let _ = query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await?;
        admin_pool.close().await;

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(5)
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

        Ok(Self {
            pool,
            schema,
            connection_string,
        })
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

#[derive(Clone)]
struct ControlledSource {
    source_id: DeliverySourceId,
    positions: Arc<Mutex<Vec<DeliveryPosition>>>,
}

impl ControlledSource {
    fn new(source_id: DeliverySourceId) -> Self {
        Self {
            source_id,
            positions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn append(&self) -> DeliveryPosition {
        let mut positions = self
            .positions
            .lock()
            .expect("controlled source mutex should not be poisoned");
        let next = u64::try_from(positions.len())
            .expect("fixture event count should fit u64")
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .expect("fixture delivery position should be positive");
        let position = DeliveryPosition::new(next);
        positions.push(position);
        position
    }

    fn append_many(&self, count: usize) -> Vec<DeliveryPosition> {
        (0..count).map(|_| self.append()).collect()
    }

    fn envelope(&self, position: DeliveryPosition) -> PersistedEventEnvelope {
        let stream_id = StreamId::try_new(format!("continuous::{}", position.get()))
            .expect("fixture stream ID should be valid");
        let payload = RawValue::from_string(format!(r#"{{"stream_id":"{stream_id}"}}"#))
            .expect("fixture payload should be valid raw JSON");
        let metadata = RawValue::from_string("{}".to_owned())
            .expect("fixture metadata should be valid raw JSON");
        PersistedEventEnvelope::new(
            self.source_id.clone(),
            position,
            PersistedEventId::new(Uuid::from_u128(u128::from(position.get()))),
            stream_id,
            StreamVersion::new(0),
            EventTypeName::try_new("continuous-event").expect("fixture event type should be valid"),
            payload,
            metadata,
        )
    }
}

impl ProjectionSource for ControlledSource {
    type Error = Infallible;

    fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    async fn high_watermark(&self) -> Result<Option<DeliveryPosition>, Self::Error> {
        Ok(self
            .positions
            .lock()
            .expect("controlled source mutex should not be poisoned")
            .last()
            .copied())
    }

    async fn read_envelopes(
        &self,
        _selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> Result<Vec<PersistedEventEnvelope>, Self::Error> {
        let limit: usize = limit.into();
        let upper = match through {
            DeliveryUpperBound::Inclusive(position) => Some(position),
            DeliveryUpperBound::Unbounded => None,
        };
        let positions = self
            .positions
            .lock()
            .expect("controlled source mutex should not be poisoned")
            .iter()
            .copied()
            .filter(|position| after.is_none_or(|after| *position > after))
            .filter(|position| upper.is_none_or(|upper| *position <= upper))
            .take(limit)
            .collect::<Vec<_>>();
        Ok(positions
            .into_iter()
            .map(|position| self.envelope(position))
            .collect())
    }
}

#[derive(Deserialize)]
struct ContinuousEvent {
    stream_id: StreamId,
}

struct IncrementProjector {
    name: ProjectorName,
}

impl PostgresProjector for IncrementProjector {
    type Event = ContinuousEvent;
    type Error = sqlx::Error;
    type AfterCommit = NoopAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    async fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        _position: DeliveryPosition,
        transaction: &'a mut Transaction<'c, Postgres>,
    ) -> Result<Self::AfterCommit, Self::Error>
    where
        'c: 'a,
    {
        let _ = &event.stream_id;
        let _ = query("UPDATE continuous_projection_effect SET total = total + 1")
            .execute(&mut **transaction)
            .await?;
        Ok(NoopAfterCommit)
    }
}

#[derive(Debug, Clone)]
struct ControlledPollSleeper {
    requests: Arc<Mutex<Vec<Duration>>>,
    started: mpsc::UnboundedSender<Result<ProjectionIdleBoundaryObservation, String>>,
    permits: Arc<Semaphore>,
    destination_pool: Pool<Postgres>,
    store: PostgresProjectionStore,
    projector_name: ProjectorName,
}

impl ProjectionPollSleeper for ControlledPollSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let requests = self.requests.clone();
        let started = self.started.clone();
        let permits = self.permits.clone();
        let destination_pool = self.destination_pool.clone();
        let store = self.store.clone();
        let projector_name = self.projector_name.clone();
        Box::pin(async move {
            requests
                .lock()
                .expect("poll observation mutex should not be poisoned")
                .push(duration);
            let observation =
                observe_idle_boundary(&destination_pool, &store, &projector_name).await;
            let _ = started.send(observation);
            let permit = permits
                .acquire_owned()
                .await
                .expect("fixture owns the poll semaphore for the run lifetime");
            permit.forget();
        })
    }
}

async fn observe_idle_boundary(
    destination_pool: &Pool<Postgres>,
    store: &PostgresProjectionStore,
    projector_name: &ProjectorName,
) -> Result<ProjectionIdleBoundaryObservation, String> {
    let effect_count = query_scalar::<_, i64>("SELECT total FROM continuous_projection_effect")
        .fetch_one(destination_pool)
        .await
        .map_err(|error| format!("could not observe committed effect: {error}"))?;
    let progress = store
        .progress(projector_name)
        .await
        .map_err(|error| format!("could not observe committed progress: {error}"))?;
    Ok(ProjectionIdleBoundaryObservation {
        effect_count: u64::try_from(effect_count)
            .map_err(|error| format!("effect count was negative: {error}"))?,
        progress: progress.map(|progress| ProjectionProgressObservation {
            source_id: progress.source_id().clone(),
            selection_id: progress.selection_id().clone(),
            position: progress.position(),
        }),
    })
}

struct PostgresContinuousFixture {
    database: IsolatedTestDatabase,
    store: PostgresProjectionStore,
    source: ControlledSource,
    selection: ProjectionSelection,
    projector_name: ProjectorName,
}

impl PostgresContinuousFixture {
    async fn new() -> Result<Self, FixtureError> {
        let database = IsolatedTestDatabase::new().await?;
        let store = PostgresProjectionStore::from_pool(database.pool.clone());
        store.migrate().await?;
        let _ = query("CREATE TABLE continuous_projection_effect (total BIGINT NOT NULL)")
            .execute(&database.pool)
            .await?;
        let _ = query("INSERT INTO continuous_projection_effect (total) VALUES (0)")
            .execute(&database.pool)
            .await?;
        let selection = ProjectionSelection::try_new(
            ProjectionSelectionId::try_new("continuous-events-v1")
                .expect("fixture selection ID should be valid"),
            ProjectionStreamFilter::All,
            vec![
                EventTypeName::try_new("continuous-event")
                    .expect("fixture event type should be valid"),
            ],
        )
        .expect("fixture selection should be valid");
        let projector_name =
            ProjectorName::try_new(format!("continuous-effect-{}", database.schema))
                .expect("schema-derived projector name should be valid");
        Ok(Self {
            database,
            store,
            source: ControlledSource::new(
                DeliverySourceId::try_new("controlled-continuous-source")
                    .expect("fixture source ID should be valid"),
            ),
            selection,
            projector_name,
        })
    }

    async fn cleanup(self) -> Result<(), FixtureError> {
        drop(self.store);
        self.database.cleanup().await?;
        Ok(())
    }

    async fn observe(
        &mut self,
        append_after_catch_up: bool,
    ) -> Result<ProjectionContinuousObservation, FixtureError> {
        let initial_through = if append_after_catch_up {
            self.source.append_many(5).last().copied()
        } else {
            None
        };
        let cancellation = CancellationToken::new();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let permits = Arc::new(Semaphore::new(0));
        let (started_sender, mut started_receiver) = mpsc::unbounded_channel();
        let config = PostgresProjectionConfig::new(self.selection.clone())
            .with_batch_size(BatchSize::new(2))
            .expect("fixture batch size should be positive")
            .continuous(cancellation.clone())
            .with_continuous_poll_interval(POLL_INTERVAL)
            .expect("fixture poll interval should be positive")
            .with_poll_sleeper(ControlledPollSleeper {
                requests: requests.clone(),
                started: started_sender,
                permits: permits.clone(),
                destination_pool: self.database.pool.clone(),
                store: self.store.clone(),
                projector_name: self.projector_name.clone(),
            });
        let source = self.source.clone();
        let store = self.store.clone();
        let projector = IncrementProjector {
            name: self.projector_name.clone(),
        };
        let mut runner = tokio::spawn(async move {
            run_transactional_projection(projector, &source, &store, config).await
        });

        let mut idle_boundaries = Vec::new();
        let first = wait_for_idle_or_completion(&mut runner, &mut started_receiver).await?;
        let appended_positions = match first {
            RunnerState::Completed(outcome) => {
                let outcome = outcome?;
                return self
                    .observation(
                        outcome,
                        initial_through,
                        Vec::new(),
                        requests,
                        idle_boundaries,
                    )
                    .await;
            }
            RunnerState::Idle(boundary) if append_after_catch_up => {
                idle_boundaries.push(boundary);
                let positions = self.source.append_many(3);
                permits.add_permits(1);
                match wait_for_idle_or_completion(&mut runner, &mut started_receiver).await? {
                    RunnerState::Completed(outcome) => {
                        let outcome = outcome?;
                        return self
                            .observation(
                                outcome,
                                initial_through,
                                positions,
                                requests,
                                idle_boundaries,
                            )
                            .await;
                    }
                    RunnerState::Idle(boundary) => {
                        idle_boundaries.push(boundary);
                        cancellation.cancel();
                        positions
                    }
                }
            }
            RunnerState::Idle(boundary) => {
                idle_boundaries.push(boundary);
                cancellation.cancel();
                Vec::new()
            }
        };

        let outcome = finish_runner(&mut runner).await?;
        self.observation(
            outcome,
            initial_through,
            appended_positions,
            requests,
            idle_boundaries,
        )
        .await
    }

    async fn observation(
        &self,
        outcome: ProjectionRunOutcome,
        initial_through: Option<DeliveryPosition>,
        appended_positions: Vec<DeliveryPosition>,
        requests: Arc<Mutex<Vec<Duration>>>,
        idle_boundaries: Vec<ProjectionIdleBoundaryObservation>,
    ) -> Result<ProjectionContinuousObservation, FixtureError> {
        let effect_count = query_scalar::<_, i64>("SELECT total FROM continuous_projection_effect")
            .fetch_one(&self.database.pool)
            .await?;
        let progress = self.store.progress(&self.projector_name).await?;
        Ok(ProjectionContinuousObservation {
            outcome: convert_outcome(outcome),
            initial_through,
            appended_positions,
            effect_count: u64::try_from(effect_count)
                .expect("fixture effect count should be nonnegative"),
            progress: progress.map(|progress| ProjectionProgressObservation {
                source_id: progress.source_id().clone(),
                selection_id: progress.selection_id().clone(),
                position: progress.position(),
            }),
            poll_sleep_requests: requests
                .lock()
                .expect("poll observation mutex should not be poisoned")
                .clone(),
            idle_boundaries,
        })
    }
}

enum RunnerState {
    Idle(ProjectionIdleBoundaryObservation),
    Completed(Result<ProjectionRunOutcome, FixtureError>),
}

async fn wait_for_idle_or_completion(
    runner: &mut JoinHandle<Result<ProjectionRunOutcome, TransactionalProjectionError>>,
    started: &mut mpsc::UnboundedReceiver<Result<ProjectionIdleBoundaryObservation, String>>,
) -> Result<RunnerState, FixtureError> {
    let observation = timeout(RUN_TIMEOUT, async {
        tokio::select! {
            biased;
            outcome = &mut *runner => Ok(RunnerState::Completed(
                outcome.map_err(FixtureError::from).and_then(|outcome| outcome.map_err(FixtureError::from)),
            )),
            notification = started.recv() => match notification {
                Some(Ok(observation)) => Ok(RunnerState::Idle(observation)),
                Some(Err(error)) => Err(FixtureError::IdleObservation(error)),
                None => Err(FixtureError::PollObservationClosed),
            },
        }
    })
    .await;
    match observation {
        Ok(Ok(state)) => Ok(state),
        Ok(Err(error)) => {
            abort_and_join(runner).await;
            Err(error)
        }
        Err(_) => {
            abort_and_join(runner).await;
            Err(FixtureError::TimedOut)
        }
    }
}

async fn finish_runner(
    runner: &mut JoinHandle<Result<ProjectionRunOutcome, TransactionalProjectionError>>,
) -> Result<ProjectionRunOutcome, FixtureError> {
    match timeout(RUN_TIMEOUT, &mut *runner).await {
        Ok(outcome) => Ok(outcome??),
        Err(_) => {
            abort_and_join(runner).await;
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
        unknown => panic!("unsupported projection run outcome: {unknown:?}"),
    }
}

impl TransactionalProjectionContinuousFixture for PostgresContinuousFixture {
    type Error = FixtureError;

    async fn observe_delivery_after_initial_catch_up(
        &mut self,
    ) -> Result<ProjectionContinuousObservation, Self::Error> {
        self.observe(true).await
    }

    async fn observe_idle_cancellation(
        &mut self,
    ) -> Result<ProjectionContinuousObservation, Self::Error> {
        self.observe(false).await
    }
}

type FixtureContractFuture<'a> = Pin<Box<dyn Future<Output = Result<(), FixtureError>> + 'a>>;

async fn assert_fixture_contract(
    contract: impl for<'a> FnOnce(&'a mut PostgresContinuousFixture) -> FixtureContractFuture<'a>,
) {
    let mut fixture = PostgresContinuousFixture::new()
        .await
        .expect("fixture should initialize");
    let result = AssertUnwindSafe(contract(&mut fixture))
        .catch_unwind()
        .await;
    fixture
        .cleanup()
        .await
        .expect("test schema cleanup should succeed even after contract panic");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("contract fixture must not fail: {error}"),
        Err(payload) => resume_unwind(payload),
    }
}

fn pending_runner() -> JoinHandle<Result<ProjectionRunOutcome, TransactionalProjectionError>> {
    tokio::spawn(std::future::pending())
}

// Break caught: propagating a failed idle-boundary observation while detaching the runner leaves
// its poll sleeper pending and retains the leader connection during fixture cleanup.
#[tokio::test]
async fn idle_observation_failure_aborts_and_bounded_joins_pending_runner() {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    sender
        .send(Err("fixture idle observation failure".to_owned()))
        .expect("fixture observation channel should be open");
    let mut runner = pending_runner();

    let result = wait_for_idle_or_completion(&mut runner, &mut receiver).await;

    assert!(matches!(
        result,
        Err(FixtureError::IdleObservation(error))
            if error == "fixture idle observation failure"
    ));
    assert!(
        runner.is_finished(),
        "observation failure must not detach a live runner",
    );
}

// Break caught: treating a closed observation channel as an ordinary fixture error while leaving
// the runner alive can hang destination-pool shutdown indefinitely.
#[tokio::test]
async fn closed_idle_observation_channel_aborts_and_bounded_joins_pending_runner() {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    drop(sender);
    let mut runner = pending_runner();

    let result = wait_for_idle_or_completion(&mut runner, &mut receiver).await;

    assert!(matches!(result, Err(FixtureError::PollObservationClosed)));
    assert!(
        runner.is_finished(),
        "closed observation channel must not detach a live runner",
    );
}

// Break caught: returning after the first catch-up cycle strands events appended later instead of
// observing them during the same continuous invocation.
#[tokio::test]
async fn continuous_mode_delivers_an_event_appended_after_initial_catch_up() {
    assert_fixture_contract(|fixture| Box::pin(continuous_delivery_contract(fixture))).await;
}

// Break caught: spinning on an empty frontier, sleeping for zero time, or treating cancellation as
// a failure makes an idle continuous projector consume CPU or shut down unreliably.
#[tokio::test]
async fn continuous_mode_awaits_one_positive_idle_poll_and_cancels_normally() {
    assert_fixture_contract(|fixture| Box::pin(continuous_idle_cancellation_contract(fixture)))
        .await;
}
