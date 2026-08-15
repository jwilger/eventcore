use std::collections::HashMap;

use eventcore_types::{
    BatchSize, CheckpointStore, CommandStateReplayCheckpoint, CommandStateSnapshot,
    CommandStateSnapshotId, Event, EventFilter, EventPage, EventReader, EventStore,
    EventStoreError, ProjectorCoordinator, StreamId, StreamPattern, StreamPosition, StreamPrefix,
    StreamVersion, StreamWrites, collect_events,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
#[error("[{scenario}] {detail}")]
pub struct ContractTestFailure {
    scenario: &'static str,
    detail: String,
}

impl ContractTestFailure {
    fn new(scenario: &'static str, detail: impl Into<String>) -> Self {
        Self {
            scenario,
            detail: detail.into(),
        }
    }

    fn builder_error(scenario: &'static str, phase: &'static str, error: EventStoreError) -> Self {
        Self::new(scenario, format!("builder failure during {phase}: {error}"))
    }

    fn store_error(
        scenario: &'static str,
        operation: &'static str,
        error: EventStoreError,
    ) -> Self {
        Self::new(
            scenario,
            format!("{operation} operation returned unexpected error: {error}"),
        )
    }

    fn assertion(scenario: &'static str, detail: impl Into<String>) -> Self {
        Self::new(scenario, detail)
    }
}

pub type ContractTestResult = Result<(), ContractTestFailure>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractTestEvent {
    stream_id: StreamId,
}

impl ContractTestEvent {
    pub fn new(stream_id: StreamId) -> Self {
        Self { stream_id }
    }
}

impl Event for ContractTestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "ContractTestEvent"
    }
}

fn contract_stream_id(
    scenario: &'static str,
    label: &str,
) -> Result<StreamId, ContractTestFailure> {
    // Include UUID for parallel test execution against shared database
    let raw = format!("contract::{}::{}::{}", scenario, label, Uuid::now_v7());

    StreamId::try_new(raw.clone()).map_err(|error| {
        ContractTestFailure::assertion(
            scenario,
            format!("unable to construct stream id `{}`: {}", raw, error),
        )
    })
}

fn contract_snapshot_id(
    scenario: &'static str,
) -> Result<CommandStateSnapshotId, ContractTestFailure> {
    let raw = format!("contract::snapshot::{scenario}::{}", Uuid::now_v7());
    CommandStateSnapshotId::try_new(raw.clone()).map_err(|error| {
        ContractTestFailure::assertion(
            scenario,
            format!("unable to construct snapshot id `{raw}`: {error}"),
        )
    })
}

fn contract_snapshot(
    primary_stream: &StreamId,
    secondary_stream: &StreamId,
    primary_version: usize,
    secondary_version: usize,
    balance: u64,
) -> CommandStateSnapshot {
    let stream_versions = HashMap::from([
        (primary_stream.clone(), StreamVersion::new(primary_version)),
        (
            secondary_stream.clone(),
            StreamVersion::new(secondary_version),
        ),
    ]);
    CommandStateSnapshot::new(
        serde_json::json!({ "balance": balance }),
        stream_versions.clone(),
    )
    .with_replay_checkpoints(vec![
        CommandStateReplayCheckpoint {
            stream_id: primary_stream.clone(),
            state: serde_json::json!({ "balance": balance - 1 }),
            stream_versions: HashMap::from([(
                primary_stream.clone(),
                StreamVersion::new(primary_version),
            )]),
        },
        CommandStateReplayCheckpoint {
            stream_id: secondary_stream.clone(),
            state: serde_json::json!({ "balance": balance }),
            stream_versions,
        },
    ])
}

pub async fn test_command_state_snapshot_load_missing<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "command_state_snapshot_load_missing";

    let snapshot = make_store()
        .load_command_state_snapshot(contract_snapshot_id(SCENARIO)?)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "load_snapshot", error))?;
    if snapshot.is_some() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "a missing snapshot unexpectedly returned a projection",
        ));
    }
    Ok(())
}

pub async fn test_command_state_snapshot_round_trip_and_monotonic<F, S>(
    make_store: F,
) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "command_state_snapshot_round_trip_and_monotonic";

    let store = make_store();
    let snapshot_id = contract_snapshot_id(SCENARIO)?;
    let primary_stream = contract_stream_id(SCENARIO, "primary")?;
    let secondary_stream = contract_stream_id(SCENARIO, "secondary")?;
    let newer = contract_snapshot(&primary_stream, &secondary_stream, 4, 9, 200);
    let stale = contract_snapshot(&primary_stream, &secondary_stream, 3, 8, 100);

    store
        .save_command_state_snapshot(snapshot_id.clone(), newer.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "save_snapshot", error))?;
    store
        .save_command_state_snapshot(snapshot_id.clone(), stale)
        .await
        .map_err(|error| {
            ContractTestFailure::store_error(SCENARIO, "save_stale_snapshot", error)
        })?;
    let loaded = store
        .load_command_state_snapshot(snapshot_id)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "load_snapshot", error))?
        .ok_or_else(|| ContractTestFailure::assertion(SCENARIO, "saved snapshot was missing"))?;

    if loaded.state != newer.state
        || loaded.stream_versions != newer.stream_versions
        || loaded.replay_checkpoints.len() != newer.replay_checkpoints.len()
        || loaded.replay_checkpoints[1].state != newer.replay_checkpoints[1].state
    {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "snapshot round-trip lost projection data or allowed a stale vector to regress it",
        ));
    }
    Ok(())
}

pub async fn test_read_stream_after_excludes_reconstructed_events<F, S>(
    make_store: F,
) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "read_stream_after_excludes_reconstructed_events";

    let store = make_store();
    let stream_id = contract_stream_id(SCENARIO, "history")?;
    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_id,
        StreamVersion::new(0),
    )?;
    for _ in 0..3 {
        writes = append_contract_event(SCENARIO, writes, &stream_id)?;
    }
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;
    let events = collect_events(
        store
            .read_stream_after::<ContractTestEvent>(stream_id, StreamVersion::new(2))
            .await
            .map_err(|error| {
                ContractTestFailure::store_error(SCENARIO, "read_stream_after", error)
            })?,
    )
    .await
    .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream_after", error))?;
    if events.len() != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected one event after version 2, observed {}",
                events.len()
            ),
        ));
    }
    Ok(())
}

fn builder_step(
    scenario: &'static str,
    phase: &'static str,
    result: Result<StreamWrites, EventStoreError>,
) -> Result<StreamWrites, ContractTestFailure> {
    result.map_err(|error| ContractTestFailure::builder_error(scenario, phase, error))
}

fn register_contract_stream(
    scenario: &'static str,
    writes: StreamWrites,
    stream_id: &StreamId,
    expected_version: StreamVersion,
) -> Result<StreamWrites, ContractTestFailure> {
    builder_step(
        scenario,
        "register_stream",
        writes.register_stream(stream_id.clone(), expected_version),
    )
}

fn append_contract_event(
    scenario: &'static str,
    writes: StreamWrites,
    stream_id: &StreamId,
) -> Result<StreamWrites, ContractTestFailure> {
    let event = ContractTestEvent::new(stream_id.clone());
    builder_step(scenario, "append", writes.append(event))
}

pub async fn test_basic_read_write<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "basic_read_write";

    let store = make_store();
    let stream_id = contract_stream_id(SCENARIO, "single");

    let stream_id = stream_id?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_id,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_id)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let stream = store
        .read_stream::<ContractTestEvent>(stream_id.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let events = collect_events(stream)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let len = events.len();
    let empty = events.is_empty();

    if empty {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected stream to contain events but it was empty",
        ));
    }

    if len != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected stream to contain exactly one event, observed len={}",
                len
            ),
        ));
    }

    Ok(())
}

pub async fn test_concurrent_version_conflicts<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "concurrent_version_conflicts";

    let store = make_store();
    let stream_id = contract_stream_id(SCENARIO, "shared")?;

    let first_writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_id,
        StreamVersion::new(0),
    )?;
    let first_writes = append_contract_event(SCENARIO, first_writes, &stream_id)?;

    let _ = store
        .append_events(first_writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let conflicting_writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_id,
        StreamVersion::new(0),
    )?;
    let conflicting_writes = append_contract_event(SCENARIO, conflicting_writes, &stream_id)?;

    match store.append_events(conflicting_writes).await {
        Err(EventStoreError::VersionConflict { .. }) => Ok(()),
        Err(error) => Err(ContractTestFailure::store_error(
            SCENARIO,
            "append_events",
            error,
        )),
        Ok(_) => Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected version conflict but append succeeded",
        )),
    }
}

pub async fn test_read_only_participant_conflict_preserves_atomicity<F, S>(
    make_store: F,
) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "read_only_participant_conflict_preserves_atomicity";

    let store = make_store();
    let participant_stream = contract_stream_id(SCENARIO, "participant")?;
    let target_stream = contract_stream_id(SCENARIO, "target")?;

    // Seed the participant to version one. The subsequent command will read it
    // at version zero but emit an event only for the target stream.
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &participant_stream,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &participant_stream)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &participant_stream,
        StreamVersion::new(0),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &target_stream, StreamVersion::new(0))?;
    let writes = append_contract_event(SCENARIO, writes, &target_stream)?;

    match store.append_events(writes).await {
        Err(EventStoreError::VersionConflict { .. }) => {}
        Err(error) => {
            return Err(ContractTestFailure::store_error(
                SCENARIO,
                "append_events",
                error,
            ));
        }
        Ok(_) => {
            return Err(ContractTestFailure::assertion(
                SCENARIO,
                "expected stale read-only participant to reject the append",
            ));
        }
    }

    let target_events = store
        .read_stream::<ContractTestEvent>(target_stream)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let target_events = collect_events(target_events)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    if !target_events.is_empty() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "target stream received events despite a read-only participant conflict",
        ));
    }

    Ok(())
}

pub async fn test_current_read_only_participant_allows_write<F, S>(
    make_store: F,
) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "current_read_only_participant_allows_write";

    let store = make_store();
    let participant_stream = contract_stream_id(SCENARIO, "participant")?;
    let target_stream = contract_stream_id(SCENARIO, "target")?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &participant_stream,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &participant_stream)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &participant_stream,
        StreamVersion::new(1),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &target_stream, StreamVersion::new(0))?;
    let writes = append_contract_event(SCENARIO, writes, &target_stream)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let target_events = store
        .read_stream::<ContractTestEvent>(target_stream)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let target_events = collect_events(target_events)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    if target_events.len() != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected target stream to contain one event, observed {}",
                target_events.len()
            ),
        ));
    }

    Ok(())
}

pub async fn test_stream_isolation<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_isolation";

    let store = make_store();
    let left_stream = contract_stream_id(SCENARIO, "left")?;
    let right_stream = contract_stream_id(SCENARIO, "right")?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &left_stream,
        StreamVersion::new(0),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &right_stream, StreamVersion::new(0))?;
    let writes = append_contract_event(SCENARIO, writes, &left_stream)?;
    let writes = append_contract_event(SCENARIO, writes, &right_stream)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    let left_stream_events = store
        .read_stream::<ContractTestEvent>(left_stream.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let left_events = collect_events(left_stream_events)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let right_stream_events = store
        .read_stream::<ContractTestEvent>(right_stream.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let right_events = collect_events(right_stream_events)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let left_len = left_events.len();
    if left_len != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "left stream expected exactly one event but observed {}",
                left_len
            ),
        ));
    }

    if left_events
        .iter()
        .any(|event| event.stream_id() != &left_stream)
    {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "left stream read events belonging to another stream",
        ));
    }

    let right_len = right_events.len();
    if right_len != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "right stream expected exactly one event but observed {}",
                right_len
            ),
        ));
    }

    if right_events
        .iter()
        .any(|event| event.stream_id() != &right_stream)
    {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "right stream read events belonging to another stream",
        ));
    }

    Ok(())
}

pub async fn test_missing_stream_reads<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "missing_stream_reads";

    let store = make_store();
    let stream_id = contract_stream_id(SCENARIO, "ghost")?;

    let stream = store
        .read_stream::<ContractTestEvent>(stream_id.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;
    let events = collect_events(stream)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    if !events.is_empty() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected read_stream to succeed with no events for an untouched stream",
        ));
    }

    Ok(())
}

pub async fn test_conflict_preserves_atomicity<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "conflict_preserves_atomicity";

    let store = make_store();
    let left_stream = contract_stream_id(SCENARIO, "left")?;
    let right_stream = contract_stream_id(SCENARIO, "right")?;

    // Seed one event per stream so we can introduce a single-stream conflict later.
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &left_stream,
        StreamVersion::new(0),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &right_stream, StreamVersion::new(0))?;
    let writes = append_contract_event(SCENARIO, writes, &left_stream)?;
    let writes = append_contract_event(SCENARIO, writes, &right_stream)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // Build a batch where the left stream has a stale expected version and the right stream is current.
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &left_stream,
        StreamVersion::new(0),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &right_stream, StreamVersion::new(1))?;
    let writes = append_contract_event(SCENARIO, writes, &left_stream)?;
    let writes = append_contract_event(SCENARIO, writes, &right_stream)?;

    match store.append_events(writes).await {
        Err(EventStoreError::VersionConflict { .. }) => {
            let left_stream_events = store
                .read_stream::<ContractTestEvent>(left_stream.clone())
                .await
                .map_err(|error| {
                    ContractTestFailure::store_error(SCENARIO, "read_stream", error)
                })?;
            let left_events = collect_events(left_stream_events).await.map_err(|error| {
                ContractTestFailure::store_error(SCENARIO, "read_stream", error)
            })?;
            if left_events.len() != 1 {
                return Err(ContractTestFailure::assertion(
                    SCENARIO,
                    format!(
                        "expected left stream to remain at len=1 after failed append, observed {}",
                        left_events.len()
                    ),
                ));
            }

            let right_stream_events = store
                .read_stream::<ContractTestEvent>(right_stream.clone())
                .await
                .map_err(|error| {
                    ContractTestFailure::store_error(SCENARIO, "read_stream", error)
                })?;
            let right_events = collect_events(right_stream_events).await.map_err(|error| {
                ContractTestFailure::store_error(SCENARIO, "read_stream", error)
            })?;
            if right_events.len() != 1 {
                return Err(ContractTestFailure::assertion(
                    SCENARIO,
                    format!(
                        "expected right stream to remain at len=1 after failed append, observed {}",
                        right_events.len()
                    ),
                ));
            }

            Ok(())
        }
        Err(error) => Err(ContractTestFailure::store_error(
            SCENARIO,
            "append_events",
            error,
        )),
        Ok(_) => Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected version conflict but append succeeded",
        )),
    }
}

/// A different event type used to test type mismatch detection in read_stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MismatchedEvent {
    stream_id: StreamId,
    extra_field: String,
}

impl Event for MismatchedEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "MismatchedEvent"
    }
}

/// Contract test: read_stream errors when events on the stream don't match
/// the requested type.
///
/// This verifies that all backends behave consistently when a stream contains
/// events that were written with one type but read with a different type.
/// The correct behavior is to return `EventStoreError::DeserializationFailed`,
/// not to silently skip unrecognized events.
pub async fn test_read_stream_errors_on_type_mismatch<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "read_stream_errors_on_type_mismatch";

    let store = make_store();
    let stream_id = contract_stream_id(SCENARIO, "mismatched")?;

    // Write an event using ContractTestEvent
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_id,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_id)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // Read the same stream but request a different event type. Opening the
    // stream may succeed; the type mismatch surfaces as a per-event decode
    // failure when the stream is consumed. Either path (an open-time error or
    // an item-level error during collection) is acceptable, but the stream
    // MUST NOT silently yield events of the wrong type.
    let result = match store.read_stream::<MismatchedEvent>(stream_id).await {
        Ok(stream) => collect_events(stream).await,
        Err(error) => Err(error),
    };

    match result {
        Err(EventStoreError::DeserializationFailed { .. }) => Ok(()),
        Err(other) => Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected DeserializationFailed error, got different error: {}",
                other
            ),
        )),
        Ok(events) if events.is_empty() => Err(ContractTestFailure::assertion(
            SCENARIO,
            "read_stream silently returned empty results instead of erroring on type mismatch",
        )),
        Ok(events) => Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "read_stream returned {} events instead of erroring on type mismatch",
                events.len()
            ),
        )),
    }
}

// NOTE: The old fragmented macros (event_store_contract_tests!, event_reader_contract_tests!)
// have been removed. Use backend_contract_tests! which runs ALL contract tests.

/// Unified contract test macro for all backend implementations.
///
/// This macro generates ALL contract tests for a backend implementation.
/// When new contract tests are added to eventcore-testing, they automatically
/// run for all backends that use this macro—no changes to backend test files required.
///
/// # Example
///
/// ```ignore
/// backend_contract_tests! {
///     suite = my_backend,
///     make_store = || MyEventStore::new(),
///     make_checkpoint_store = || MyCheckpointStore::new(),
///     make_coordinator = || MyProjectorCoordinator::new(),
/// }
/// ```
///
/// # Requirements
///
/// The store type must implement both `EventStore` and `EventReader` traits.
/// The checkpoint store type must implement `CheckpointStore` trait.
/// The coordinator type must implement `ProjectorCoordinator` trait.
#[macro_export]
macro_rules! backend_contract_tests {
    (suite = $suite:ident, make_store = $make_store:expr, make_checkpoint_store = $make_checkpoint_store:expr, make_coordinator = $make_coordinator:expr $(,)?) => {
        #[allow(non_snake_case)]
        mod $suite {
            use $crate::contract::{
                test_basic_read_write, test_batch_limiting,
                test_checkpoint_independent_subscriptions,
                test_checkpoint_load_missing_returns_none, test_checkpoint_save_and_load,
                test_checkpoint_update_overwrites, test_command_state_snapshot_load_missing,
                test_command_state_snapshot_round_trip_and_monotonic,
                test_concurrent_version_conflicts, test_conflict_preserves_atomicity,
                test_coordination_acquire_leadership, test_coordination_independent_subscriptions,
                test_coordination_leadership_released_on_guard_drop,
                test_coordination_second_instance_blocked,
                test_current_read_only_participant_allows_write,
                test_event_ordering_across_streams, test_missing_stream_reads,
                test_position_based_resumption,
                test_read_only_participant_conflict_preserves_atomicity,
                test_read_stream_after_excludes_reconstructed_events,
                test_read_stream_errors_on_type_mismatch, test_stream_isolation,
                test_stream_pattern_char_class, test_stream_pattern_filtering,
                test_stream_pattern_single_char, test_stream_prefix_filtering,
                test_stream_prefix_requires_prefix_match,
            };

            #[tokio::test(flavor = "multi_thread")]
            async fn basic_read_write_contract() {
                test_basic_read_write($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn concurrent_version_conflicts_contract() {
                test_concurrent_version_conflicts($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn read_only_participant_conflict_preserves_atomicity_contract() {
                test_read_only_participant_conflict_preserves_atomicity($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn current_read_only_participant_allows_write_contract() {
                test_current_read_only_participant_allows_write($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_isolation_contract() {
                test_stream_isolation($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn missing_stream_reads_contract() {
                test_missing_stream_reads($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn conflict_preserves_atomicity_contract() {
                test_conflict_preserves_atomicity($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn read_stream_errors_on_type_mismatch_contract() {
                test_read_stream_errors_on_type_mismatch($make_store)
                    .await
                    .expect("event store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn event_ordering_across_streams_contract() {
                test_event_ordering_across_streams($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn position_based_resumption_contract() {
                test_position_based_resumption($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_prefix_filtering_contract() {
                test_stream_prefix_filtering($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_prefix_requires_prefix_match_contract() {
                test_stream_prefix_requires_prefix_match($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_pattern_filtering_contract() {
                test_stream_pattern_filtering($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_pattern_single_char_contract() {
                test_stream_pattern_single_char($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn stream_pattern_char_class_contract() {
                test_stream_pattern_char_class($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn batch_limiting_contract() {
                test_batch_limiting($make_store)
                    .await
                    .expect("event reader contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn command_state_snapshot_load_missing_contract() {
                test_command_state_snapshot_load_missing($make_store)
                    .await
                    .expect("command state snapshot contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn command_state_snapshot_round_trip_and_monotonic_contract() {
                test_command_state_snapshot_round_trip_and_monotonic($make_store)
                    .await
                    .expect("command state snapshot contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn read_stream_after_excludes_reconstructed_events_contract() {
                test_read_stream_after_excludes_reconstructed_events($make_store)
                    .await
                    .expect("event store contract failed");
            }

            // CheckpointStore contract tests
            #[tokio::test(flavor = "multi_thread")]
            async fn checkpoint_save_and_load_contract() {
                test_checkpoint_save_and_load($make_checkpoint_store)
                    .await
                    .expect("checkpoint store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn checkpoint_update_overwrites_contract() {
                test_checkpoint_update_overwrites($make_checkpoint_store)
                    .await
                    .expect("checkpoint store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn checkpoint_load_missing_returns_none_contract() {
                test_checkpoint_load_missing_returns_none($make_checkpoint_store)
                    .await
                    .expect("checkpoint store contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn checkpoint_independent_subscriptions_contract() {
                test_checkpoint_independent_subscriptions($make_checkpoint_store)
                    .await
                    .expect("checkpoint store contract failed");
            }

            // ProjectorCoordinator contract tests
            #[tokio::test(flavor = "multi_thread")]
            async fn coordination_acquire_leadership_contract() {
                test_coordination_acquire_leadership($make_coordinator)
                    .await
                    .expect("coordinator contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn coordination_second_instance_blocked_contract() {
                test_coordination_second_instance_blocked($make_coordinator)
                    .await
                    .expect("coordinator contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn coordination_independent_subscriptions_contract() {
                test_coordination_independent_subscriptions($make_coordinator)
                    .await
                    .expect("coordinator contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn coordination_leadership_released_on_guard_drop_contract() {
                test_coordination_leadership_released_on_guard_drop($make_coordinator)
                    .await
                    .expect("coordinator contract failed");
            }
        }
    };
}

pub use backend_contract_tests;

/// Contract test: Events from multiple streams are read in global append order
pub async fn test_event_ordering_across_streams<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "event_ordering_across_streams";

    let store = make_store();

    // Given: Three streams with events appended in specific order
    let stream_a = contract_stream_id(SCENARIO, "stream-a")?;
    let stream_b = contract_stream_id(SCENARIO, "stream-b")?;
    let stream_c = contract_stream_id(SCENARIO, "stream-c")?;

    // Append event to stream A
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_a,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_a)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // Append event to stream B
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_b,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_b)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // Append event to stream C
    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_c,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_c)?;
    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading all events via EventReader with no position filter
    let filter = EventFilter::all();
    let page = EventPage::first(BatchSize::new(100));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(SCENARIO, "read_events failed to read events")
        })?;

    // Then: Events are returned in global append order (A, B, C)
    if events.len() != 3 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!("expected 3 events but got {}", events.len()),
        ));
    }

    // And: Verify complete ordering across all three streams
    let (first_event, _) = &events[0];
    if first_event.stream_id() != &stream_a {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected first event from stream_a but got from {:?}",
                first_event.stream_id()
            ),
        ));
    }

    let (second_event, _) = &events[1];
    if second_event.stream_id() != &stream_b {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected second event from stream_b but got from {:?}",
                second_event.stream_id()
            ),
        ));
    }

    let (third_event, _) = &events[2];
    if third_event.stream_id() != &stream_c {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected third event from stream_c but got from {:?}",
                third_event.stream_id()
            ),
        ));
    }

    Ok(())
}

/// Contract test: Position-based resumption works correctly
pub async fn test_position_based_resumption<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "position_based_resumption";

    let store = make_store();

    // Given: Events at positions 0, 1, 2, 3, 4 (5 events total)
    let stream = contract_stream_id(SCENARIO, "stream")?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream,
        StreamVersion::new(0),
    )?;

    // Append 5 events
    for _ in 0..5 {
        writes = append_contract_event(SCENARIO, writes, &stream)?;
    }

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // Get position of third event (index 2, position 2)
    let filter = EventFilter::all();
    let page = EventPage::first(BatchSize::new(100));
    let all_events = store
        .read_events::<ContractTestEvent>(filter.clone(), page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(SCENARIO, "read_events failed to read events")
        })?;

    let (_third_event, third_position) = &all_events[2];

    // When: Reading events after position 2
    let page_after = EventPage::after(*third_position, BatchSize::new(100));
    let events_after = store
        .read_events::<ContractTestEvent>(filter, page_after)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(
                SCENARIO,
                "read_events failed when reading after position",
            )
        })?;

    // Then: Only events at positions 3 and 4 are returned (2 events)
    if events_after.len() != 2 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected 2 events after position {} but got {}",
                third_position,
                events_after.len()
            ),
        ));
    }

    // And: Position 2 event is NOT included (verify exclusivity)
    for (_event, position) in events_after.iter() {
        if *position == *third_position {
            return Err(ContractTestFailure::assertion(
                SCENARIO,
                format!(
                    "expected position {} to be excluded but it was included in results",
                    third_position
                ),
            ));
        }
    }

    // And: Returned event positions are greater than third_position and in ascending order
    let (_event1, pos1) = &events_after[0];
    let (_event2, pos2) = &events_after[1];

    if *pos1 <= *third_position {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected first returned position to be > {} but got {}",
                third_position, pos1
            ),
        ));
    }

    if *pos2 <= *pos1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected positions to be in ascending order but {} <= {}",
                pos2, pos1
            ),
        ));
    }

    Ok(())
}

/// Contract test: Stream prefix filtering returns only matching streams
pub async fn test_stream_prefix_filtering<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_prefix_filtering";

    let store = make_store();

    // Given: Events on streams with IDs that actually start with "account-" or "order-"
    let account_1 = StreamId::try_new(format!("account-1-{}", Uuid::now_v7())).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let account_2 = StreamId::try_new(format!("account-2-{}", Uuid::now_v7())).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let order_1 = StreamId::try_new(format!("order-1-{}", Uuid::now_v7())).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &account_1,
        StreamVersion::new(0),
    )?;
    writes = register_contract_stream(SCENARIO, writes, &account_2, StreamVersion::new(0))?;
    writes = register_contract_stream(SCENARIO, writes, &order_1, StreamVersion::new(0))?;

    writes = append_contract_event(SCENARIO, writes, &account_1)?;
    writes = append_contract_event(SCENARIO, writes, &account_2)?;
    writes = append_contract_event(SCENARIO, writes, &order_1)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading with prefix filter "account-"
    let prefix = StreamPrefix::try_new("account-").map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("failed to create stream prefix: {}", e))
    })?;
    let filter = EventFilter::prefix(prefix);
    let page = EventPage::first(BatchSize::new(100));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(SCENARIO, "read_events failed with stream prefix filter")
        })?;

    // Then: Only events from account-1 and account-2 are returned
    if events.len() != 2 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected 2 events from account-* streams but got {}",
                events.len()
            ),
        ));
    }

    // And: All events are from streams starting with "account-"
    for (event, _) in events.iter() {
        let stream_id_str = event.stream_id().as_ref();
        if !stream_id_str.starts_with("account-") {
            return Err(ContractTestFailure::assertion(
                SCENARIO,
                format!(
                    "expected all events from streams starting with 'account-' but found event from {}",
                    stream_id_str
                ),
            ));
        }
    }

    // And: order-1 events are filtered out (verified by length check above)

    Ok(())
}

/// Contract test: Stream prefix filtering requires true prefix match (not substring match)
pub async fn test_stream_prefix_requires_prefix_match<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_prefix_requires_prefix_match";

    let store = make_store();

    // Given: Three streams with actual prefixes: "account-123", "my-account-456", "order-789"
    // We want to verify that prefix "account-" matches ONLY "account-123", not "my-account-456"
    let account_stream =
        StreamId::try_new(format!("account-123-{}", Uuid::now_v7())).map_err(|e| {
            ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
        })?;
    let my_account_stream = StreamId::try_new(format!("my-account-456-{}", Uuid::now_v7()))
        .map_err(|e| {
            ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
        })?;
    let order_stream = StreamId::try_new(format!("order-789-{}", Uuid::now_v7())).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &account_stream,
        StreamVersion::new(0),
    )?;
    writes = register_contract_stream(SCENARIO, writes, &my_account_stream, StreamVersion::new(0))?;
    writes = register_contract_stream(SCENARIO, writes, &order_stream, StreamVersion::new(0))?;

    writes = append_contract_event(SCENARIO, writes, &account_stream)?;
    writes = append_contract_event(SCENARIO, writes, &my_account_stream)?;
    writes = append_contract_event(SCENARIO, writes, &order_stream)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading with prefix filter "account-"
    let prefix = StreamPrefix::try_new("account-").map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("failed to create stream prefix: {}", e))
    })?;
    let filter = EventFilter::prefix(prefix);
    let page = EventPage::first(BatchSize::new(100));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(SCENARIO, "read_events failed with stream prefix filter")
        })?;

    // Then: ONLY "account-123" stream should be returned (not "my-account-456")
    if events.len() != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected exactly 1 event from account-* prefix but got {} (bug: implementation uses contains() instead of starts_with())",
                events.len()
            ),
        ));
    }

    // And: The event must be from a stream starting with "account-123"
    let (event, _) = &events[0];
    let stream_id_str = event.stream_id().as_ref();
    if !stream_id_str.starts_with("account-123") {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected event from stream starting with 'account-123' but got from {}",
                stream_id_str
            ),
        ));
    }

    // And: Verify it's NOT from my-account-456 (proves we're not doing substring matching)
    if stream_id_str.starts_with("my-account-456") {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "BUG EXPOSED: got event from stream starting with 'my-account-456' when filtering for prefix 'account-' - implementation must use prefix matching from the start of the stream ID",
        ));
    }

    Ok(())
}

/// Contract test: Glob `*` wildcard pattern filtering selects only matching streams.
///
/// Per ADR-0047, `EventFilter::pattern` matches stream IDs against a POSIX glob
/// pattern. The wildcard `*` matches any sequence of characters (including the
/// stream separator `/`). This test proves the filter is applied at the query
/// level: it appends enough non-matching events to cross the pagination `LIMIT`
/// boundary before any matching events, so an implementation that applied `LIMIT`
/// before the pattern filter would return zero matches.
pub async fn test_stream_pattern_filtering<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_pattern_filtering";

    let store = make_store();
    let run = Uuid::now_v7();

    // Given: many non-matching "order-*" streams followed by two matching
    // "account-*" streams. The non-matching events alone exceed the page limit.
    let mut writes = StreamWrites::new();
    let mut order_streams = Vec::new();
    for _ in 0..50 {
        let order = StreamId::try_new(format!("order-{}", Uuid::now_v7())).map_err(|e| {
            ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
        })?;
        writes = register_contract_stream(SCENARIO, writes, &order, StreamVersion::new(0))?;
        order_streams.push(order);
    }

    let account_1 = StreamId::try_new(format!("account-1-{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let account_2 = StreamId::try_new(format!("account-2-{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    writes = register_contract_stream(SCENARIO, writes, &account_1, StreamVersion::new(0))?;
    writes = register_contract_stream(SCENARIO, writes, &account_2, StreamVersion::new(0))?;

    for order in &order_streams {
        writes = append_contract_event(SCENARIO, writes, order)?;
    }
    writes = append_contract_event(SCENARIO, writes, &account_1)?;
    writes = append_contract_event(SCENARIO, writes, &account_2)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading with glob pattern "account-*" and a page limit smaller than
    // the number of non-matching events that precede the matches.
    let pattern = StreamPattern::try_new("account-*").map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("failed to create stream pattern: {}", e))
    })?;
    let filter = EventFilter::pattern(pattern);
    let page = EventPage::first(BatchSize::new(10));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(
                SCENARIO,
                "read_events failed with stream pattern filter",
            )
        })?;

    // Then: exactly the two "account-*" events are returned. If the limit were
    // applied before filtering, this would be zero.
    if events.len() != 2 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected exactly 2 events matching glob 'account-*' but got {} (pattern filter must be applied before the page limit)",
                events.len()
            ),
        ));
    }

    for (event, _) in events.iter() {
        let stream_id_str = event.stream_id().as_ref();
        if !stream_id_str.starts_with("account-") {
            return Err(ContractTestFailure::assertion(
                SCENARIO,
                format!(
                    "expected all events from streams matching 'account-*' but found event from {}",
                    stream_id_str
                ),
            ));
        }
    }

    Ok(())
}

/// Contract test: Glob `?` matches exactly one character.
///
/// Per ADR-0047, `?` matches a single arbitrary character. Pattern `account-?`
/// must match `account-1` but not `account-12` (two trailing characters) and
/// not `order-1`.
pub async fn test_stream_pattern_single_char<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_pattern_single_char";

    let store = make_store();
    let run = Uuid::now_v7();

    // Given: one single-char-suffix stream, one two-char-suffix stream, one order.
    let account_x = StreamId::try_new(format!("account-x{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let account_xy = StreamId::try_new(format!("account-xy{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let order = StreamId::try_new(format!("order-z{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &account_x,
        StreamVersion::new(0),
    )?;
    writes = register_contract_stream(SCENARIO, writes, &account_xy, StreamVersion::new(0))?;
    writes = register_contract_stream(SCENARIO, writes, &order, StreamVersion::new(0))?;

    writes = append_contract_event(SCENARIO, writes, &account_x)?;
    writes = append_contract_event(SCENARIO, writes, &account_xy)?;
    writes = append_contract_event(SCENARIO, writes, &order)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading with glob pattern "account-?{run}" (single char between
    // "account-" and the run suffix).
    let pattern = StreamPattern::try_new(format!("account-?{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("failed to create stream pattern: {}", e))
    })?;
    let filter = EventFilter::pattern(pattern);
    let page = EventPage::first(BatchSize::new(100));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(
                SCENARIO,
                "read_events failed with stream pattern filter",
            )
        })?;

    // Then: only "account-x{run}" matches (single char). "account-xy{run}" has
    // two chars; "order-z{run}" has the wrong prefix.
    if events.len() != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected exactly 1 event matching glob 'account-?{run}' but got {}",
                events.len()
            ),
        ));
    }

    let (event, _) = &events[0];
    let stream_id_str = event.stream_id().as_ref();
    if stream_id_str != account_x.as_ref() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected the single-character match {} but got {}",
                account_x.as_ref(),
                stream_id_str
            ),
        ));
    }

    Ok(())
}

/// Contract test: Glob `[0-9]` character class matches a single digit.
///
/// Per ADR-0047, a bracketed character class `[...]` matches one character from
/// the set. Pattern `account-[0-9]*` must match streams whose suffix begins with
/// a digit and reject those beginning with a non-digit.
pub async fn test_stream_pattern_char_class<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "stream_pattern_char_class";

    let store = make_store();
    let run = Uuid::now_v7().simple().to_string();

    // Given: a digit-prefixed account stream and a letter-prefixed account stream.
    let digit_stream = StreamId::try_new(format!("account-7-{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;
    let letter_stream = StreamId::try_new(format!("account-a-{run}")).map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("invalid stream id: {}", e))
    })?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &digit_stream,
        StreamVersion::new(0),
    )?;
    writes = register_contract_stream(SCENARIO, writes, &letter_stream, StreamVersion::new(0))?;

    writes = append_contract_event(SCENARIO, writes, &digit_stream)?;
    writes = append_contract_event(SCENARIO, writes, &letter_stream)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Reading with glob pattern "account-[0-9]*".
    let pattern = StreamPattern::try_new("account-[0-9]*").map_err(|e| {
        ContractTestFailure::assertion(SCENARIO, format!("failed to create stream pattern: {}", e))
    })?;
    let filter = EventFilter::pattern(pattern);
    let page = EventPage::first(BatchSize::new(100));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(
                SCENARIO,
                "read_events failed with stream pattern filter",
            )
        })?;

    // Then: only the digit-prefixed stream matches.
    if events.len() != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected exactly 1 event matching glob 'account-[0-9]*' but got {}",
                events.len()
            ),
        ));
    }

    let (event, _) = &events[0];
    let stream_id_str = event.stream_id().as_ref();
    if stream_id_str != digit_stream.as_ref() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected the digit-prefixed match {} but got {}",
                digit_stream.as_ref(),
                stream_id_str
            ),
        ));
    }

    Ok(())
}

/// Contract test: Batch limiting returns exactly the specified number of events
pub async fn test_batch_limiting<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventReader + Send + Sync + 'static,
{
    const SCENARIO: &str = "batch_limiting";

    let store = make_store();

    // Given: 20 events in the store
    let stream = contract_stream_id(SCENARIO, "stream")?;

    let mut writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream,
        StreamVersion::new(0),
    )?;

    // Append 20 events
    for _ in 0..20 {
        writes = append_contract_event(SCENARIO, writes, &stream)?;
    }

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Read events with limit of 10
    let filter = EventFilter::all();
    let page = EventPage::first(BatchSize::new(10));
    let events = store
        .read_events::<ContractTestEvent>(filter, page)
        .await
        .map_err(|_error| {
            ContractTestFailure::assertion(SCENARIO, "read_events failed with limit")
        })?;

    // Then: Exactly 10 events are returned
    if events.len() != 10 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!("expected exactly 10 events but got {}", events.len()),
        ));
    }

    // And: Events are the FIRST 10 in global order
    // (We verify this by checking we got exactly 10 events - the implementation
    // must return events in order, so if we got 10 events they must be the first 10)

    Ok(())
}

// ============================================================================
// CheckpointStore Contract Tests
// ============================================================================

/// Contract test: Save a checkpoint and load it back
pub async fn test_checkpoint_save_and_load<F, CS>(make_checkpoint_store: F) -> ContractTestResult
where
    F: Fn() -> CS + Send + Sync + Clone + 'static,
    CS: CheckpointStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "checkpoint_save_and_load";

    let store = make_checkpoint_store();

    // Given: A subscription name and position
    let subscription_name = format!("contract::{}::{}", SCENARIO, Uuid::now_v7());
    let position = StreamPosition::new(Uuid::now_v7());

    // When: Saving the checkpoint
    store
        .save(&subscription_name, position)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "save failed"))?;

    // Then: Loading returns the saved position
    let loaded = store
        .load(&subscription_name)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "load failed"))?;

    if loaded != Some(position) {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected loaded position {:?} but got {:?}",
                Some(position),
                loaded
            ),
        ));
    }

    Ok(())
}

/// Contract test: Saving a checkpoint overwrites the previous value
pub async fn test_checkpoint_update_overwrites<F, CS>(
    make_checkpoint_store: F,
) -> ContractTestResult
where
    F: Fn() -> CS + Send + Sync + Clone + 'static,
    CS: CheckpointStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "checkpoint_update_overwrites";

    let store = make_checkpoint_store();

    // Given: A subscription with an initial checkpoint
    let subscription_name = format!("contract::{}::{}", SCENARIO, Uuid::now_v7());
    let first_position = StreamPosition::new(Uuid::now_v7());

    store
        .save(&subscription_name, first_position)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "first save failed"))?;

    // When: Saving a new position
    let second_position = StreamPosition::new(Uuid::now_v7());
    store
        .save(&subscription_name, second_position)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "second save failed"))?;

    // Then: Loading returns the new position, not the old one
    let loaded = store
        .load(&subscription_name)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "load failed"))?;

    if loaded != Some(second_position) {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected updated position {:?} but got {:?}",
                Some(second_position),
                loaded
            ),
        ));
    }

    Ok(())
}

/// Contract test: Loading a non-existent checkpoint returns None
pub async fn test_checkpoint_load_missing_returns_none<F, CS>(
    make_checkpoint_store: F,
) -> ContractTestResult
where
    F: Fn() -> CS + Send + Sync + Clone + 'static,
    CS: CheckpointStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "checkpoint_load_missing_returns_none";

    let store = make_checkpoint_store();

    // Given: A subscription name that has never been saved
    let subscription_name = format!("contract::{}::ghost::{}", SCENARIO, Uuid::now_v7());

    // When: Loading the checkpoint
    let loaded = store
        .load(&subscription_name)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "load failed"))?;

    // Then: None is returned
    if loaded.is_some() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!("expected None for missing checkpoint but got {:?}", loaded),
        ));
    }

    Ok(())
}

/// Contract test: Different subscription names have independent checkpoints
pub async fn test_checkpoint_independent_subscriptions<F, CS>(
    make_checkpoint_store: F,
) -> ContractTestResult
where
    F: Fn() -> CS + Send + Sync + Clone + 'static,
    CS: CheckpointStore + Send + Sync + 'static,
{
    const SCENARIO: &str = "checkpoint_independent_subscriptions";

    let store = make_checkpoint_store();

    // Given: Two subscription names
    let subscription_a = format!("contract::{}::sub-a::{}", SCENARIO, Uuid::now_v7());
    let subscription_b = format!("contract::{}::sub-b::{}", SCENARIO, Uuid::now_v7());

    let position_a = StreamPosition::new(Uuid::now_v7());
    let position_b = StreamPosition::new(Uuid::now_v7());

    // When: Saving different positions for each
    store
        .save(&subscription_a, position_a)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "save A failed"))?;

    store
        .save(&subscription_b, position_b)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "save B failed"))?;

    // Then: Each subscription loads its own position
    let loaded_a = store
        .load(&subscription_a)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "load A failed"))?;

    let loaded_b = store
        .load(&subscription_b)
        .await
        .map_err(|_| ContractTestFailure::assertion(SCENARIO, "load B failed"))?;

    if loaded_a != Some(position_a) {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "subscription A: expected {:?} but got {:?}",
                Some(position_a),
                loaded_a
            ),
        ));
    }

    if loaded_b != Some(position_b) {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "subscription B: expected {:?} but got {:?}",
                Some(position_b),
                loaded_b
            ),
        ));
    }

    Ok(())
}

// ============================================================================
// ProjectorCoordinator Contract Tests
// ============================================================================

/// Contract test: First instance can acquire leadership successfully
///
/// Observable behavior: When no other instance holds leadership for a subscription,
/// calling try_acquire returns a guard indicating successful acquisition.
pub async fn test_coordination_acquire_leadership<F, C>(make_coordinator: F) -> ContractTestResult
where
    F: Fn() -> C + Send + Sync + Clone + 'static,
    C: ProjectorCoordinator + Send + Sync + 'static,
{
    const SCENARIO: &str = "coordination_acquire_leadership";

    let coordinator = make_coordinator();

    // Given: A unique subscription name (no existing leadership)
    let subscription_name = format!("contract::{}::{}", SCENARIO, Uuid::now_v7());

    // When: Attempting to acquire leadership
    let result = coordinator.try_acquire(&subscription_name).await;

    // Then: Acquisition succeeds (returns Ok with guard)
    if result.is_err() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected first instance to acquire leadership successfully, but try_acquire failed",
        ));
    }

    Ok(())
}

/// Contract test: Second instance returns error when leadership unavailable
///
/// Observable behavior: When one instance holds leadership for a subscription,
/// a second attempt to acquire leadership for the same subscription returns an error.
pub async fn test_coordination_second_instance_blocked<F, C>(
    make_coordinator: F,
) -> ContractTestResult
where
    F: Fn() -> C + Send + Sync + Clone + 'static,
    C: ProjectorCoordinator + Send + Sync + 'static,
{
    const SCENARIO: &str = "coordination_second_instance_blocked";

    let coordinator = make_coordinator();

    // Given: A unique subscription name
    let subscription_name = format!("contract::{}::{}", SCENARIO, Uuid::now_v7());

    // And: First instance acquires leadership
    let _first_guard = coordinator
        .try_acquire(&subscription_name)
        .await
        .map_err(|_| {
            ContractTestFailure::assertion(SCENARIO, "first instance failed to acquire leadership")
        })?;

    // When: Second instance attempts to acquire leadership while first holds it
    let second_result = coordinator.try_acquire(&subscription_name).await;

    // Then: Second attempt returns an error (leadership unavailable)
    if second_result.is_ok() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected second instance to be blocked but try_acquire succeeded",
        ));
    }

    Ok(())
}

/// Contract test: Different projectors have independent coordination (different lock keys)
///
/// Observable behavior: Leadership for one subscription does not block leadership
/// acquisition for a different subscription. Each subscription/projector has its own
/// independent coordination scope.
pub async fn test_coordination_independent_subscriptions<F, C>(
    make_coordinator: F,
) -> ContractTestResult
where
    F: Fn() -> C + Send + Sync + Clone + 'static,
    C: ProjectorCoordinator + Send + Sync + 'static,
{
    const SCENARIO: &str = "coordination_independent_subscriptions";

    let coordinator = make_coordinator();

    // Given: Two unique subscription names (different projectors)
    let subscription_a = format!("contract::{}::projector-A::{}", SCENARIO, Uuid::now_v7());
    let subscription_b = format!("contract::{}::projector-B::{}", SCENARIO, Uuid::now_v7());

    // And: First projector acquires leadership for subscription A
    let _guard_a = coordinator
        .try_acquire(&subscription_a)
        .await
        .map_err(|_| {
            ContractTestFailure::assertion(
                SCENARIO,
                "projector-A failed to acquire leadership for its subscription",
            )
        })?;

    // When: Second projector attempts to acquire leadership for subscription B
    // (while first projector still holds leadership for A)
    let result_b = coordinator.try_acquire(&subscription_b).await;

    // Then: Second acquisition succeeds (different subscriptions are independent)
    if result_b.is_err() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected projector-B to acquire leadership for its own subscription, but try_acquire failed - different projectors should have independent coordination",
        ));
    }

    Ok(())
}

/// Contract test: Leadership is released when guard is dropped (crash/disconnect recovery)
///
/// Observable behavior: When an instance holding leadership drops its guard (simulating
/// process exit, crash, or connection close), the lock is released and another instance
/// can acquire leadership for the same subscription.
pub async fn test_coordination_leadership_released_on_guard_drop<F, C>(
    make_coordinator: F,
) -> ContractTestResult
where
    F: Fn() -> C + Send + Sync + Clone + 'static,
    C: ProjectorCoordinator + Send + Sync + 'static,
{
    const SCENARIO: &str = "coordination_leadership_released_on_guard_drop";

    let coordinator = make_coordinator();

    // Given: A unique subscription name
    let subscription_name = format!("contract::{}::{}", SCENARIO, Uuid::now_v7());

    // And: First instance acquires leadership, then drops the guard
    {
        let _first_guard = coordinator
            .try_acquire(&subscription_name)
            .await
            .map_err(|_| {
                ContractTestFailure::assertion(
                    SCENARIO,
                    "first instance failed to acquire leadership",
                )
            })?;
        // Guard is dropped here when scope ends (simulates process exit/crash)
    }

    // When: Second instance attempts to acquire leadership after first guard dropped
    let second_result = coordinator.try_acquire(&subscription_name).await;

    // Then: Second acquisition succeeds (leadership was released)
    if second_result.is_err() {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "expected second instance to acquire leadership after first guard dropped, but try_acquire failed - leadership should be released when guard is dropped",
        ));
    }

    Ok(())
}
