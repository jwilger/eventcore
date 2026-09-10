use std::ops::Deref;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, query};
use uuid::Uuid;

/// A PostgreSQL pool scoped to a test-owned schema.
pub(crate) struct IsolatedTestDatabase {
    pool: Option<Pool<Postgres>>,
    schema: String,
    connection_string: String,
}

/// Two pools that resolve event and projection tables through different schemas.
pub(crate) struct SplitSearchPathTestDatabase {
    source_pool: Option<Pool<Postgres>>,
    legacy_pool: Option<Pool<Postgres>>,
    projection_schema: String,
    event_schema: String,
    connection_string: String,
}

impl SplitSearchPathTestDatabase {
    pub(crate) fn plan() -> Self {
        Self {
            source_pool: None,
            legacy_pool: None,
            projection_schema: format!("eventcore_delivery_projection_{}", Uuid::now_v7().simple()),
            event_schema: format!("eventcore_delivery_events_{}", Uuid::now_v7().simple()),
            connection_string: crate::common::connection_string(),
        }
    }

    pub(crate) async fn initialize(&mut self) -> Result<(), sqlx::Error> {
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await?;
        let _ = query(&format!("CREATE SCHEMA {}", self.event_schema))
            .execute(&admin_pool)
            .await?;
        let _ = query(&format!("CREATE SCHEMA {}", self.projection_schema))
            .execute(&admin_pool)
            .await?;
        admin_pool.close().await;

        self.legacy_pool =
            Some(pool_with_search_path(&self.connection_string, self.event_schema.clone()).await?);
        self.source_pool = Some(
            pool_with_search_path(
                &self.connection_string,
                format!("{}, {}", self.projection_schema, self.event_schema),
            )
            .await?,
        );
        Ok(())
    }

    pub(crate) fn source_pool(&self) -> Pool<Postgres> {
        self.source_pool
            .as_ref()
            .expect("split database should be initialized")
            .clone()
    }

    pub(crate) fn legacy_pool(&self) -> &Pool<Postgres> {
        self.legacy_pool
            .as_ref()
            .expect("split database should be initialized")
    }

    pub(crate) async fn cleanup(&self) {
        if let Some(pool) = &self.source_pool {
            pool.close().await;
        }
        if let Some(pool) = &self.legacy_pool {
            pool.close().await;
        }

        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await
            .expect("configured test postgres should accept split cleanup connections");
        let projection_result = query(&format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.projection_schema
        ))
        .execute(&cleanup_pool)
        .await;
        let event_result = query(&format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.event_schema
        ))
        .execute(&cleanup_pool)
        .await;
        cleanup_pool.close().await;
        let _ = projection_result.expect("projection schema cleanup should succeed");
        let _ = event_result.expect("event schema cleanup should succeed");
    }
}

impl Deref for IsolatedTestDatabase {
    type Target = Pool<Postgres>;

    fn deref(&self) -> &Self::Target {
        self.pool()
    }
}

impl IsolatedTestDatabase {
    pub(crate) fn plan() -> Self {
        Self {
            pool: None,
            schema: format!("eventcore_projection_test_{}", Uuid::now_v7().simple()),
            connection_string: crate::common::connection_string(),
        }
    }

    pub(crate) async fn initialize(&mut self) -> Result<(), sqlx::Error> {
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await?;
        let _ = query(&format!("CREATE SCHEMA {}", self.schema))
            .execute(&admin_pool)
            .await?;
        admin_pool.close().await;

        let schema_for_pool = self.schema.clone();
        self.pool = Some(
            PgPoolOptions::new()
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
                .connect(&self.connection_string)
                .await?,
        );
        Ok(())
    }

    pub(crate) fn pool(&self) -> &Pool<Postgres> {
        self.pool
            .as_ref()
            .expect("isolated database should be initialized")
    }

    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    pub(crate) fn clone_pool(&self) -> Pool<Postgres> {
        self.pool().clone()
    }

    /// Closes this test pool and drops the schema after all assertions complete.
    pub(crate) async fn cleanup(&self) {
        if let Some(pool) = &self.pool {
            pool.close().await;
        }
        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.connection_string)
            .await
            .expect("configured test postgres should accept cleanup connections");
        let _ = query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&cleanup_pool)
            .await
            .expect("isolated test schema cleanup should succeed");
        cleanup_pool.close().await;
    }
}

pub(crate) async fn create_isolated_test_pool() -> IsolatedTestDatabase {
    let mut database = IsolatedTestDatabase::plan();
    database
        .initialize()
        .await
        .expect("configured test postgres should initialize an isolated schema");
    database
}

async fn pool_with_search_path(
    connection_string: &str,
    search_path: String,
) -> Result<Pool<Postgres>, sqlx::Error> {
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
}
