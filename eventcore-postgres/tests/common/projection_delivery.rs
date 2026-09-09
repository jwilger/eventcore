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
