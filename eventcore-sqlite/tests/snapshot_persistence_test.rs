use std::collections::HashMap;

use eventcore_sqlite::{SqliteConfig, SqliteEventStore};
use eventcore_testing::contract::ContractTestEvent;
use eventcore_types::{
    CommandStateSnapshot, CommandStateSnapshotId, EventStore, StreamId, StreamVersion,
    StreamWrites, collect_events,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn command_state_snapshots_survive_reopen_and_do_not_regress() {
    // Given: a file-backed, migrated store and a newer command-state projection.
    let path = std::env::temp_dir().join(format!("eventcore-snapshot-{}.db", Uuid::now_v7()));
    let config = SqliteConfig {
        path: path.clone(),
        encryption_key: None,
    };
    let snapshot_id = CommandStateSnapshotId::try_new("accounts::command-state".to_string())
        .expect("valid snapshot id");
    let stream_id = StreamId::try_new("accounts::one".to_string()).expect("valid stream id");
    let newer = CommandStateSnapshot::new(
        json!({"balance": 42}),
        HashMap::from([(stream_id.clone(), StreamVersion::new(4))]),
    );

    let store = SqliteEventStore::new(config.clone()).expect("store opens");
    store.migrate().await.expect("migration succeeds");
    store
        .save_command_state_snapshot(snapshot_id.clone(), newer.clone())
        .await
        .expect("newer projection persists");
    drop(store);

    // When: another process reopens the database and attempts to save an older
    // projection for the same consistency boundary.
    let reopened = SqliteEventStore::new(config).expect("store reopens");
    let stale = CommandStateSnapshot::new(
        json!({"balance": 10}),
        HashMap::from([(stream_id, StreamVersion::new(2))]),
    );
    reopened
        .save_command_state_snapshot(snapshot_id.clone(), stale)
        .await
        .expect("stale save is harmless");

    // Then: the durable, more-complete projection remains available.
    let loaded = reopened
        .load_command_state_snapshot(snapshot_id)
        .await
        .expect("snapshot loads")
        .expect("snapshot exists");
    assert_eq!(loaded.state, newer.state);
    assert_eq!(loaded.stream_versions, newer.stream_versions);

    drop(reopened);
    std::fs::remove_file(path).expect("temporary database is removable");
}

#[tokio::test]
async fn read_stream_after_seeks_past_the_exclusive_version() {
    // Given: a stream with three persisted events.
    let store = SqliteEventStore::in_memory().expect("store opens");
    store.migrate().await.expect("migration succeeds");
    let stream_id = StreamId::try_new("accounts::one".to_string()).expect("valid stream id");
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .expect("stream registers")
        .append(ContractTestEvent::new(stream_id.clone()))
        .expect("first event appends")
        .append(ContractTestEvent::new(stream_id.clone()))
        .expect("second event appends")
        .append(ContractTestEvent::new(stream_id.clone()))
        .expect("third event appends");
    let _ = store.append_events(writes).await.expect("events persist");

    // When: the first two versions have already been folded into a projection.
    let stream = store
        .read_stream_after::<ContractTestEvent>(stream_id, StreamVersion::new(2))
        .await
        .expect("stream opens");

    // Then: only the subsequent event is replayed.
    let events = collect_events(stream).await.expect("events decode");
    assert_eq!(events.len(), 1);
}
