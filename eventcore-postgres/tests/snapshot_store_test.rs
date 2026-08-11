//! Contract coverage for durable command-state snapshots in PostgreSQL.

mod common;

use std::collections::HashMap;

use eventcore_types::{
    CommandStateReplayCheckpoint, CommandStateSnapshot, CommandStateSnapshotId, Event, EventStore,
    StreamId, StreamVersion, StreamWrites, collect_events,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct SnapshotTestEvent {
    stream_id: StreamId,
    sequence: usize,
}

impl Event for SnapshotTestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "SnapshotTestEvent"
    }
}

fn snapshot_id() -> CommandStateSnapshotId {
    CommandStateSnapshotId::try_new(format!("snapshot::{}", uuid::Uuid::now_v7()))
        .expect("generated snapshot id is valid")
}

fn stream_id() -> StreamId {
    StreamId::try_new(format!("snapshot-stream::{}", uuid::Uuid::now_v7()))
        .expect("generated stream id is valid")
}

#[tokio::test]
async fn command_state_snapshots_round_trip_and_never_regress_their_version_vector() {
    // Given: a durable snapshot that includes two reconstructed streams.
    let store = common::create_test_store().await;
    let snapshot_id = snapshot_id();
    let primary_stream = stream_id();
    let related_stream = stream_id();
    let newer = CommandStateSnapshot::new(
        json!({ "balance": 200 }),
        HashMap::from([
            (primary_stream.clone(), StreamVersion::new(10)),
            (related_stream.clone(), StreamVersion::new(4)),
        ]),
    )
    .with_replay_checkpoints(vec![CommandStateReplayCheckpoint {
        stream_id: primary_stream.clone(),
        state: json!({ "balance": 160 }),
        stream_versions: HashMap::from([(primary_stream.clone(), StreamVersion::new(10))]),
    }]);
    store
        .save_command_state_snapshot(snapshot_id.clone(), newer)
        .await
        .expect("new snapshot should persist");

    // When: a concurrent attempt finishes with a stale projection that omits
    // a stream and trails the primary stream version.
    let stale = CommandStateSnapshot::new(
        json!({ "balance": 100 }),
        HashMap::from([(primary_stream.clone(), StreamVersion::new(9))]),
    );
    store
        .save_command_state_snapshot(snapshot_id.clone(), stale)
        .await
        .expect("stale write should be safely ignored");

    // Then: the durable projection still contains the newer complete vector.
    let stored = store
        .load_command_state_snapshot(snapshot_id)
        .await
        .expect("snapshot read should succeed")
        .expect("snapshot should exist");
    assert_eq!(stored.state, json!({ "balance": 200 }));
    assert_eq!(
        stored.stream_versions.get(&primary_stream),
        Some(&StreamVersion::new(10))
    );
    assert_eq!(
        stored.stream_versions.get(&related_stream),
        Some(&StreamVersion::new(4))
    );
    assert_eq!(stored.replay_checkpoints.len(), 1);
    assert_eq!(
        stored.replay_checkpoints[0].state,
        json!({ "balance": 160 })
    );
}

#[tokio::test]
async fn read_stream_after_seeks_past_the_reconstructed_version() {
    // Given: a stream with a complete history and a snapshot that covers its
    // first two events.
    let store = common::create_test_store().await;
    let stream_id = stream_id();
    let writes = (0..4).try_fold(
        StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .expect("stream registration should succeed"),
        |writes, sequence| {
            writes.append(SnapshotTestEvent {
                stream_id: stream_id.clone(),
                sequence,
            })
        },
    );
    let _ = store
        .append_events(writes.expect("events should build"))
        .await
        .expect("events should append");

    // When: reconstruction asks for only events after version two.
    let events = collect_events(
        store
            .read_stream_after::<SnapshotTestEvent>(stream_id, StreamVersion::new(2))
            .await
            .expect("stream tail should open"),
    )
    .await
    .expect("stream tail should decode");

    // Then: PostgreSQL returns exactly the remaining suffix.
    assert_eq!(
        events
            .into_iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}
