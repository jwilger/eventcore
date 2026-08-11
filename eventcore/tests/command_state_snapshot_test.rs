use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use eventcore::{
    Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy, StreamId, execute,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_types::{CommandStateSnapshotId, EventStore, StreamVersion, StreamWrites};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AccountEvent {
    Deposited { account_id: StreamId },
}

impl Event for AccountEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            Self::Deposited { account_id } => account_id,
        }
    }

    fn event_type_name() -> &'static str {
        "command-state-snapshot-test.account-event"
    }
}

#[derive(Clone, Command)]
struct ObserveAccount {
    #[stream]
    account_id: StreamId,
    applied_events: Arc<AtomicUsize>,
}

impl CommandLogic for ObserveAccount {
    type Event = AccountEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        let _ = self.applied_events.fetch_add(1, Ordering::Relaxed);
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(NewEvents::default())
    }

    fn command_state_snapshot_id(&self) -> Option<CommandStateSnapshotId> {
        Some(
            CommandStateSnapshotId::try_new(format!(
                "command-state-snapshot-test::{}",
                self.account_id
            ))
            .expect("valid snapshot id"),
        )
    }

    fn serialize_command_state_snapshot(
        &self,
        _state: &Self::State,
    ) -> Result<serde_json::Value, CommandError> {
        Ok(serde_json::Value::Null)
    }

    fn deserialize_command_state_snapshot(
        &self,
        _state: serde_json::Value,
    ) -> Result<Self::State, CommandError> {
        Ok(())
    }
}

#[tokio::test]
async fn execute_reuses_a_durable_command_state_snapshot_after_the_refresh_threshold() {
    let store = InMemoryEventStore::new();
    let account_id =
        StreamId::try_new("accounts::snapshot-threshold".to_string()).expect("valid stream id");
    let writes = (0..100).fold(
        StreamWrites::new()
            .register_stream(account_id.clone(), StreamVersion::new(0))
            .expect("stream registration succeeds"),
        |writes, _| {
            writes
                .append(AccountEvent::Deposited {
                    account_id: account_id.clone(),
                })
                .expect("event append declaration succeeds")
        },
    );
    let _ = store
        .append_events(writes)
        .await
        .expect("seed events persist");

    let applied_events = Arc::new(AtomicUsize::new(0));
    let command = ObserveAccount {
        account_id,
        applied_events: Arc::clone(&applied_events),
    };

    let _ = execute(&store, command.clone(), RetryPolicy::new())
        .await
        .expect("initial command execution succeeds");
    assert_eq!(applied_events.load(Ordering::Relaxed), 100);

    applied_events.store(0, Ordering::Relaxed);

    let _ = execute(&store, command, RetryPolicy::new())
        .await
        .expect("subsequent command execution succeeds");

    assert_eq!(
        applied_events.load(Ordering::Relaxed),
        0,
        "the durable snapshot should avoid replaying the already-snapshotted history"
    );
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TransferEvent {
    Recorded { stream_id: StreamId, label: String },
}

impl Event for TransferEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            Self::Recorded { stream_id, .. } => stream_id,
        }
    }

    fn event_type_name() -> &'static str {
        "command-state-snapshot-test.transfer-event"
    }
}

#[derive(Clone, Command)]
struct ObserveTransfer {
    #[stream]
    source_stream: StreamId,
    #[stream]
    destination_stream: StreamId,
    applied_events: Arc<AtomicUsize>,
    observed_state: Arc<Mutex<Vec<String>>>,
}

impl CommandLogic for ObserveTransfer {
    type Event = TransferEvent;
    type State = Vec<String>;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        let _ = self.applied_events.fetch_add(1, Ordering::Relaxed);
        let TransferEvent::Recorded { label, .. } = event;
        state.push(label.clone());
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        *self
            .observed_state
            .lock()
            .expect("observation mutex is not poisoned") = state;
        Ok(NewEvents::default())
    }

    fn command_state_snapshot_id(&self) -> Option<CommandStateSnapshotId> {
        Some(
            CommandStateSnapshotId::try_new(format!(
                "command-state-snapshot-test::{}::{}",
                self.source_stream, self.destination_stream
            ))
            .expect("valid snapshot id"),
        )
    }

    fn serialize_command_state_snapshot(
        &self,
        state: &Self::State,
    ) -> Result<serde_json::Value, CommandError> {
        serde_json::to_value(state)
            .map_err(|error| CommandError::ValidationError(error.to_string()))
    }

    fn deserialize_command_state_snapshot(
        &self,
        state: serde_json::Value,
    ) -> Result<Self::State, CommandError> {
        serde_json::from_value(state)
            .map_err(|error| CommandError::ValidationError(error.to_string()))
    }
}

#[tokio::test]
async fn execute_catches_up_an_early_stream_before_replaying_later_streams() {
    let store = InMemoryEventStore::new();
    let source_stream = StreamId::try_new("transfers::source".to_string()).expect("valid stream");
    let destination_stream =
        StreamId::try_new("transfers::destination".to_string()).expect("valid stream");

    for (stream_id, prefix) in [
        (source_stream.clone(), "source"),
        (destination_stream.clone(), "destination"),
    ] {
        let writes = (0..100).fold(
            StreamWrites::new()
                .register_stream(stream_id.clone(), StreamVersion::new(0))
                .expect("stream registration succeeds"),
            |writes, index| {
                writes
                    .append(TransferEvent::Recorded {
                        stream_id: stream_id.clone(),
                        label: format!("{prefix}-{index}"),
                    })
                    .expect("event append declaration succeeds")
            },
        );
        let _ = store
            .append_events(writes)
            .await
            .expect("seed events persist");
    }

    let applied_events = Arc::new(AtomicUsize::new(0));
    let observed_state = Arc::new(Mutex::new(Vec::new()));
    let command = ObserveTransfer {
        source_stream: source_stream.clone(),
        destination_stream: destination_stream.clone(),
        applied_events: Arc::clone(&applied_events),
        observed_state: Arc::clone(&observed_state),
    };

    let _ = execute(&store, command.clone(), RetryPolicy::new())
        .await
        .expect("initial command execution succeeds");
    assert_eq!(applied_events.load(Ordering::Relaxed), 200);

    let tail = StreamWrites::new()
        .register_stream(source_stream.clone(), StreamVersion::new(100))
        .and_then(|writes| {
            writes.append(TransferEvent::Recorded {
                stream_id: source_stream,
                label: "source-tail".to_string(),
            })
        })
        .expect("tail event declaration succeeds");
    let _ = store
        .append_events(tail)
        .await
        .expect("tail event persists");

    applied_events.store(0, Ordering::Relaxed);
    let _ = execute(&store, command, RetryPolicy::new())
        .await
        .expect("caught-up command execution succeeds");

    assert_eq!(applied_events.load(Ordering::Relaxed), 101);
    let observed = observed_state
        .lock()
        .expect("observation mutex is not poisoned")
        .clone();
    assert_eq!(observed[100], "source-tail");
    assert_eq!(observed[101], "destination-0");
}
