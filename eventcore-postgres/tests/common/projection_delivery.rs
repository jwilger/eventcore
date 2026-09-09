use std::ops::Deref;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, query};
use uuid::Uuid;

/// A PostgreSQL pool scoped to a test-owned schema.
pub(crate) struct IsolatedTestDatabase {
    pool: Pool<Postgres>,
    schema: String,
    connection_string: String,
}

/// Two pools that resolve event and projection tables through different schemas.
pub(crate) struct SplitSearchPathTestDatabase {
    source_pool: Pool<Postgres>,
    legacy_pool: Pool<Postgres>,
    projection_schema: String,
    event_schema: String,
    connection_string: String,
}

impl SplitSearchPathTestDatabase {
    pub(crate) fn source_pool(&self) -> Pool<Postgres> {
        self.source_pool.clone()
    }

    pub(crate) fn legacy_pool(&self) -> &Pool<Postgres> {
        &self.legacy_pool
    }

    pub(crate) async fn cleanup(&self) {
        self.source_pool.close().await;
        self.legacy_pool.close().await;

        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await
            .expect("test postgres should accept split-schema cleanup connections");
        let _ = query(&format!("DROP SCHEMA {} CASCADE", self.projection_schema))
            .execute(&cleanup_pool)
            .await
            .expect("projection schema should be dropped after the search-path test");
        let _ = query(&format!("DROP SCHEMA {} CASCADE", self.event_schema))
            .execute(&cleanup_pool)
            .await
            .expect("event schema should be dropped after the search-path test");
    }
}

impl Deref for IsolatedTestDatabase {
    type Target = Pool<Postgres>;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

impl IsolatedTestDatabase {
    pub(crate) fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    pub(crate) fn clone_pool(&self) -> Pool<Postgres> {
        self.pool.clone()
    }

    /// Closes this test pool and drops the schema after all assertions complete.
    pub(crate) async fn cleanup(&self) {
        self.pool.close().await;
        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await
            .expect("test postgres should accept schema cleanup connections");
        let _ = query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&cleanup_pool)
            .await
            .expect("isolated test schema should be dropped after a successful test");
    }
}

/// Creates an isolated schema while retaining the normal event-store migrations and triggers.
pub(crate) async fn create_isolated_test_pool() -> IsolatedTestDatabase {
    let _ = crate::common::POSTGRES_CONTAINER.get_or_init(|| {
        crate::common::ensure_postgres_running();
    });

    let connection_string = crate::common::connection_string();
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await
        .expect("test postgres should accept schema setup connections");
    let schema = format!("eventcore_projection_test_{}", Uuid::now_v7().simple());

    let _ = query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin_pool)
        .await
        .expect("test schema should be created");

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
        .await
        .expect("test pool should connect to its isolated schema");

    IsolatedTestDatabase {
        pool,
        schema,
        connection_string,
    }
}

/// Creates separate source and legacy-writer pools to exercise trigger search-path binding.
pub(crate) async fn create_split_search_path_test_database() -> SplitSearchPathTestDatabase {
    let _ = crate::common::POSTGRES_CONTAINER.get_or_init(|| {
        crate::common::ensure_postgres_running();
    });

    let connection_string = crate::common::connection_string();
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await
        .expect("test postgres should accept split-schema setup connections");
    let event_schema = format!("eventcore_delivery_events_{}", Uuid::now_v7().simple());
    let projection_schema = format!("eventcore_delivery_projection_{}", Uuid::now_v7().simple());
    let _ = query(&format!("CREATE SCHEMA {event_schema}"))
        .execute(&admin_pool)
        .await
        .expect("event schema should be created");
    let _ = query(&format!("CREATE SCHEMA {projection_schema}"))
        .execute(&admin_pool)
        .await
        .expect("projection schema should be created");

    let legacy_pool = pool_with_search_path(&connection_string, event_schema.clone()).await;
    let source_pool = pool_with_search_path(
        &connection_string,
        format!("{projection_schema}, {event_schema}"),
    )
    .await;

    SplitSearchPathTestDatabase {
        source_pool,
        legacy_pool,
        projection_schema,
        event_schema,
        connection_string,
    }
}

async fn pool_with_search_path(connection_string: &str, search_path: String) -> Pool<Postgres> {
    PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _metadata| {
            let search_path = search_path.clone();
            Box::pin(async move {
                let _ = query("SELECT set_config('search_path', $1, false)")
                    .bind(search_path)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(connection_string)
        .await
        .expect("split test pool should connect with its configured search path")
}
