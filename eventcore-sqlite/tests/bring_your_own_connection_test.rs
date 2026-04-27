//! Integration tests verifying that consumers can supply their own
//! `rusqlite::Connection` to `SqliteEventStore` and `SqliteCheckpointStore`.
//!
//! These tests also exercise the `rusqlite` re-export at
//! `eventcore_sqlite::rusqlite`, ensuring downstream consumers do not need to
//! declare their own `rusqlite` dependency.

use eventcore_sqlite::rusqlite;
use eventcore_sqlite::{SqliteCheckpointStore, SqliteEventStore};
use eventcore_types::{CheckpointStore, StreamPosition};
use uuid::Uuid;

#[tokio::test]
async fn event_store_accepts_externally_constructed_connection() {
    // Given: a connection constructed by the consumer using the re-exported
    // rusqlite (no separate rusqlite dependency declared).
    let conn = rusqlite::Connection::open_in_memory()
        .expect("consumer should be able to open in-memory connection");

    // When: we construct a SqliteEventStore from that connection and migrate.
    let store = SqliteEventStore::from_connection(conn);

    store
        .migrate()
        .await
        .expect("migration should succeed against consumer-supplied connection");

    // Then: the store is usable end-to-end via the EventStore trait. The
    // contract suite already exercises the trait surface; here we just need
    // to demonstrate that migrate succeeded and the store is constructed.
    let _: &SqliteEventStore = &store;
}

#[tokio::test]
async fn checkpoint_store_accepts_externally_constructed_connection() {
    let conn = rusqlite::Connection::open_in_memory()
        .expect("consumer should be able to open in-memory connection");

    let store = SqliteCheckpointStore::from_connection(conn);

    store
        .migrate()
        .await
        .expect("checkpoint migration should succeed against consumer-supplied connection");

    let position = StreamPosition::new(Uuid::now_v7());
    store
        .save("subscription-byoc", position)
        .await
        .expect("save should succeed");
    let loaded = store
        .load("subscription-byoc")
        .await
        .expect("load should succeed");
    assert_eq!(loaded, Some(position));
}
