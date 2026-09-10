//! Consumer-facing contract for the transactional PostgreSQL projection facade.

#![cfg(feature = "postgres")]

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use eventcore::postgres::{PostgresEventStore, projections::*};
use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, NewEvents, ProjectionConfig, Projector,
    RetryPolicy, StreamDeclarations, StreamId, StreamPosition, execute, run_projection,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgPoolOptions, query, query_scalar};
use tokio::time::timeout;
use uuid::Uuid;

const DATABASE_TIMEOUT: Duration = Duration::from_secs(10);
const RUN_TIMEOUT: Duration = Duration::from_secs(10);

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct IsolatedSchema {
    pool: PgPool,
    schema: String,
    connection_string: String,
}

impl IsolatedSchema {
    async fn create(role: &str) -> TestResult<Self> {
        let host = std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5433".to_string());
        let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let admin_pool = timeout(
            DATABASE_TIMEOUT,
            PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(2))
                .connect(&connection_string),
        )
        .await
        .map_err(|error| format!("admin pool connection timed out: {error}"))??;
        let schema = format!("eventcore_facade_{role}_{}", Uuid::now_v7().simple());
        let creation = timeout(
            DATABASE_TIMEOUT,
            query(&format!("CREATE SCHEMA {schema}")).execute(&admin_pool),
        )
        .await;
        if let failure @ (Err(_) | Ok(Err(_))) = creation {
            let cleanup = drop_created_schema(&admin_pool, &schema).await;
            let _ = timeout(DATABASE_TIMEOUT, admin_pool.close()).await;
            return Err(format!(
                "schema creation failed: {}; partial schema cleanup: {}",
                describe_query_result(&failure),
                describe_result(&cleanup),
            )
            .into());
        }

        let pool_schema = schema.clone();
        let pool_result = timeout(
            DATABASE_TIMEOUT,
            PgPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(Duration::from_secs(2))
                .after_connect(move |connection, _metadata| {
                    let schema = pool_schema.clone();
                    Box::pin(async move {
                        let _ = query("SELECT set_config('search_path', $1, false)")
                            .bind(schema)
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .connect(&connection_string),
        )
        .await;
        let pool = match pool_result {
            Ok(Ok(pool)) => pool,
            failure => {
                let cleanup = drop_created_schema(&admin_pool, &schema).await;
                let _ = timeout(DATABASE_TIMEOUT, admin_pool.close()).await;
                return Err(format!(
                    "schema pool setup failed: {}; partial schema cleanup: {}",
                    describe_nested_result(failure),
                    describe_result(&cleanup),
                )
                .into());
            }
        };
        drop(admin_pool);

        Ok(Self {
            pool,
            schema,
            connection_string,
        })
    }

    async fn cleanup(self) -> TestResult {
        let mut failures = Vec::new();
        let pool_close = timeout(DATABASE_TIMEOUT, self.pool.close()).await;
        if let Err(error) = pool_close {
            failures.push(format!("schema pool close timed out: {error}"));
        }

        let cleanup_pool = timeout(
            DATABASE_TIMEOUT,
            PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(2))
                .connect(&self.connection_string),
        )
        .await;
        match cleanup_pool {
            Ok(Ok(cleanup_pool)) => {
                if let Err(error) = drop_created_schema(&cleanup_pool, &self.schema).await {
                    failures.push(error);
                }
                if let Err(error) = timeout(DATABASE_TIMEOUT, cleanup_pool.close()).await {
                    failures.push(format!("cleanup pool close timed out: {error}"));
                }
            }
            failure => failures.push(format!(
                "cleanup pool connection failed: {}",
                describe_nested_result(failure),
            )),
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

async fn drop_created_schema(pool: &PgPool, schema: &str) -> Result<(), String> {
    timeout(
        DATABASE_TIMEOUT,
        query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(pool),
    )
    .await
    .map_err(|error| format!("schema drop timed out: {error}"))?
    .map(|_| ())
    .map_err(|error| format!("schema drop failed: {error}"))
}

fn describe_result<T, E>(result: &Result<T, E>) -> String
where
    E: Display,
{
    match result {
        Ok(_) => "succeeded".to_string(),
        Err(error) => error.to_string(),
    }
}

fn describe_query_result(
    result: &Result<
        Result<sqlx::postgres::PgQueryResult, sqlx::Error>,
        tokio::time::error::Elapsed,
    >,
) -> String {
    match result {
        Ok(Ok(_)) => "succeeded".to_string(),
        Ok(Err(error)) => error.to_string(),
        Err(error) => format!("timed out: {error}"),
    }
}

fn describe_nested_result(
    result: Result<Result<PgPool, sqlx::Error>, tokio::time::error::Elapsed>,
) -> String {
    match result {
        Ok(Ok(_)) => "unexpected success".to_string(),
        Ok(Err(error)) => error.to_string(),
        Err(error) => format!("timed out: {error}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AccountCredited {
    account_id: StreamId,
    amount: i64,
}

impl Event for AccountCredited {
    fn stream_id(&self) -> &StreamId {
        &self.account_id
    }

    fn event_type_name() -> &'static str {
        "account-credited"
    }
}

struct CreditAccount {
    account_id: StreamId,
    amount: i64,
}

impl CommandStreams for CreditAccount {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.account_id.clone()])
            .expect("one valid account stream")
    }
}

impl CommandLogic for CreditAccount {
    type Event = AccountCredited;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![AccountCredited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }]
        .into())
    }
}

#[derive(Debug)]
struct ReadModelError(String);

impl Display for ReadModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ReadModelError {}

struct AccountTotalProjector {
    name: ProjectorName,
}

impl PostgresProjector for AccountTotalProjector {
    type Event = AccountCredited;
    type Error = ReadModelError;
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
        let _ = query(
            "INSERT INTO account_totals (singleton, total) VALUES (TRUE, $1) \
                 ON CONFLICT (singleton) DO UPDATE \
                 SET total = account_totals.total + EXCLUDED.total",
        )
        .bind(event.amount)
        .execute(&mut **tx)
        .await
        .map_err(|error| ReadModelError(error.to_string()))?;
        Ok(NoopAfterCommit)
    }
}

struct LegacyAccountProjector {
    total: Arc<AtomicI64>,
}

impl Projector for LegacyAccountProjector {
    type Event = AccountCredited;
    type Error = std::convert::Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        _context: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        let _ = self.total.fetch_add(event.amount, Ordering::SeqCst);
        Ok(())
    }

    fn name(&self) -> &str {
        "legacy-account-total"
    }
}

// Break caught: hiding facade vocabulary or SQLx signature types forces consumers to depend on
// eventcore-postgres/a guessed SQLx version; using one pool for both arguments hides topology bugs.
#[tokio::test]
async fn facade_runs_transactional_projection_across_distinct_pools_and_schemas() {
    let source = IsolatedSchema::create("source")
        .await
        .expect("source schema setup should succeed or clean up its partial state");
    let destination = match IsolatedSchema::create("destination").await {
        Ok(destination) => destination,
        Err(original_error) => {
            let source_cleanup = source.cleanup().await;
            panic!(
                "destination schema setup failed: {original_error}; source cleanup: {}",
                describe_result(&source_cleanup),
            );
        }
    };

    let result = AssertUnwindSafe(timeout(
        RUN_TIMEOUT,
        run_facade_contract(&source, &destination),
    ))
    .catch_unwind()
    .await;

    // Always attempt both cleanups. Neither cleanup result can replace an earlier setup, runtime,
    // assertion, or panic result.
    let source_cleanup = source.cleanup().await;
    let destination_cleanup = destination.cleanup().await;

    match result {
        Err(panic) => {
            eprintln!(
                "cleanup after projection panic: source={}, destination={}",
                describe_result(&source_cleanup),
                describe_result(&destination_cleanup),
            );
            resume_unwind(panic);
        }
        Ok(Err(error)) => panic!(
            "facade projection timed out: {error}; source cleanup: {}; destination cleanup: {}",
            describe_result(&source_cleanup),
            describe_result(&destination_cleanup),
        ),
        Ok(Ok(Err(error))) => panic!(
            "facade projection failed: {error}; source cleanup: {}; destination cleanup: {}",
            describe_result(&source_cleanup),
            describe_result(&destination_cleanup),
        ),
        Ok(Ok(Ok(()))) => {
            assert!(
                source_cleanup.is_ok() && destination_cleanup.is_ok(),
                "successful run must clean both schemas: source={}; destination={}",
                describe_result(&source_cleanup),
                describe_result(&destination_cleanup),
            );
        }
    }
}

async fn run_facade_contract(
    source_database: &IsolatedSchema,
    destination_database: &IsolatedSchema,
) -> TestResult {
    let event_store = PostgresEventStore::from_pool(source_database.pool.clone());
    event_store.migrate().await;

    let source_id = DeliverySourceId::try_new("primary-event-store")?;
    let source =
        PostgresProjectionSource::from_pool(source_database.pool.clone(), source_id.clone());
    source.migrate().await?;

    let projection_store = PostgresProjectionStore::from_pool(destination_database.pool.clone());
    projection_store.migrate().await?;
    let _ = query(
        "CREATE TABLE account_totals (\
         singleton BOOLEAN PRIMARY KEY CHECK (singleton), \
         total BIGINT NOT NULL)",
    )
    .execute(&destination_database.pool)
    .await?;

    // These independent schema observations make a same-pool implementation mutation fail even
    // before the runner's source reads and destination writes exercise opposite arguments.
    let source_has_events: bool =
        query_scalar("SELECT to_regclass('eventcore_events') IS NOT NULL")
            .fetch_one(&source_database.pool)
            .await?;
    let destination_has_events: bool =
        query_scalar("SELECT to_regclass('eventcore_events') IS NOT NULL")
            .fetch_one(&destination_database.pool)
            .await?;
    let source_has_read_model: bool =
        query_scalar("SELECT to_regclass('account_totals') IS NOT NULL")
            .fetch_one(&source_database.pool)
            .await?;
    let destination_has_read_model: bool =
        query_scalar("SELECT to_regclass('account_totals') IS NOT NULL")
            .fetch_one(&destination_database.pool)
            .await?;
    assert!(source_has_events);
    assert!(!destination_has_events);
    assert!(!source_has_read_model);
    assert!(destination_has_read_model);
    assert_ne!(source_database.schema, destination_database.schema);

    let _ = execute(
        &event_store,
        CreditAccount {
            account_id: StreamId::try_new("account-42")?,
            amount: 125,
        },
        RetryPolicy::new(),
    )
    .await?;

    let projector_name = ProjectorName::try_new("account-total-v1")?;
    let selection_id = ProjectionSelectionId::try_new("account-total-events-v1")?;
    let selection = ProjectionSelection::try_new(
        selection_id.clone(),
        ProjectionStreamFilter::All,
        vec![EventTypeName::try_new(AccountCredited::event_type_name())?],
    )?;
    let outcome = run_transactional_projection(
        AccountTotalProjector {
            name: projector_name.clone(),
        },
        &source,
        &projection_store,
        PostgresProjectionConfig::new(selection),
    )
    .await?;

    let first_position = DeliveryPosition::new(NonZeroU64::new(1).expect("one is non-zero"));
    assert_eq!(
        outcome,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(first_position),
        }
    );
    let total: i64 = query_scalar("SELECT total FROM account_totals WHERE singleton = TRUE")
        .fetch_one(&destination_database.pool)
        .await?;
    assert_eq!(total, 125);

    let progress = projection_store
        .progress(&projector_name)
        .await?
        .expect("one event should commit projection progress");
    assert_eq!(progress.source_id(), &source_id);
    assert_eq!(progress.selection_id(), &selection_id);
    assert_eq!(progress.position(), first_position);

    // Compatibility proof: the pre-existing trait and runner still compile and process the same
    // public event-store data in this consumer target.
    let legacy_total = Arc::new(AtomicI64::new(0));
    run_projection(
        LegacyAccountProjector {
            total: Arc::clone(&legacy_total),
        },
        &event_store,
        ProjectionConfig::default(),
    )
    .await?;
    assert_eq!(legacy_total.load(Ordering::SeqCst), 125);

    Ok(())
}
