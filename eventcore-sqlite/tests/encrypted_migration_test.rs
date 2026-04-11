#[cfg(feature = "encryption")]
mod encrypted_migration {
    use eventcore_sqlite::{SqliteConfig, SqliteEventStore};

    #[tokio::test]
    async fn encrypted_migration_on_fresh_database() {
        let db_path = std::env::temp_dir().join("eventcore-sqlcipher-fresh-db-test.db");
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteEventStore::new(SqliteConfig {
            path: db_path.clone(),
            encryption_key: Some("test-key".to_string()),
        })
        .expect("store should open");

        store
            .migrate()
            .await
            .expect("migration should succeed on encrypted database");

        // Clean up
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn encrypted_checkpoint_store_migration_on_fresh_database() {
        use eventcore_sqlite::SqliteCheckpointStore;

        let db_path = std::env::temp_dir().join("eventcore-sqlcipher-checkpoint-fresh-db-test.db");
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteCheckpointStore::new(SqliteConfig {
            path: db_path.clone(),
            encryption_key: Some("test-key".to_string()),
        })
        .expect("checkpoint store should open");

        store
            .migrate()
            .await
            .expect("checkpoint migration should succeed on encrypted database");

        // Clean up
        let _ = std::fs::remove_file(&db_path);
    }
}
