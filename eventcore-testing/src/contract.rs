use eventcore::{
    Event, EventStore, EventStoreError, EventSubscription, EventTypeName, StreamId, StreamVersion,
    StreamWrites, SubscriptionQuery,
};
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug)]
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

impl fmt::Display for ContractTestFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.scenario, self.detail)
    }
}

impl std::error::Error for ContractTestFailure {}

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

    fn event_type_name(&self) -> EventTypeName {
        "ContractTestEvent"
            .try_into()
            .expect("valid event type name")
    }

    #[cfg_attr(test, mutants::skip)] // test infrastructure - trait required method
    fn all_type_names() -> Vec<EventTypeName> {
        vec![
            "ContractTestEvent"
                .try_into()
                .expect("valid event type name"),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtherContractEvent {
    stream_id: StreamId,
}

impl OtherContractEvent {
    pub fn new(stream_id: StreamId) -> Self {
        Self { stream_id }
    }
}

impl Event for OtherContractEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> EventTypeName {
        "OtherContractEvent"
            .try_into()
            .expect("valid event type name")
    }

    #[cfg_attr(test, mutants::skip)] // test infrastructure - trait required method
    fn all_type_names() -> Vec<EventTypeName> {
        vec![
            "OtherContractEvent"
                .try_into()
                .expect("valid event type name"),
        ]
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

    let reader = store
        .read_stream::<ContractTestEvent>(stream_id.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let len = reader.len();
    let empty = reader.is_empty();

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
        Err(EventStoreError::VersionConflict) => Ok(()),
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

    let left_reader = store
        .read_stream::<ContractTestEvent>(left_stream.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let right_reader = store
        .read_stream::<ContractTestEvent>(right_stream.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    let left_len = left_reader.len();
    if left_len != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "left stream expected exactly one event but observed {}",
                left_len
            ),
        ));
    }

    if left_reader
        .iter()
        .any(|event| event.stream_id() != &left_stream)
    {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "left stream read events belonging to another stream",
        ));
    }

    let right_len = right_reader.len();
    if right_len != 1 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "right stream expected exactly one event but observed {}",
                right_len
            ),
        ));
    }

    if right_reader
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

    let reader = store
        .read_stream::<ContractTestEvent>(stream_id.clone())
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "read_stream", error))?;

    if !reader.is_empty() {
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
        Err(EventStoreError::VersionConflict) => {
            let left_reader = store
                .read_stream::<ContractTestEvent>(left_stream.clone())
                .await
                .map_err(|error| {
                    ContractTestFailure::store_error(SCENARIO, "read_stream", error)
                })?;
            if left_reader.len() != 1 {
                return Err(ContractTestFailure::assertion(
                    SCENARIO,
                    format!(
                        "expected left stream to remain at len=1 after failed append, observed {}",
                        left_reader.len()
                    ),
                ));
            }

            let right_reader = store
                .read_stream::<ContractTestEvent>(right_stream.clone())
                .await
                .map_err(|error| {
                    ContractTestFailure::store_error(SCENARIO, "read_stream", error)
                })?;
            if right_reader.len() != 1 {
                return Err(ContractTestFailure::assertion(
                    SCENARIO,
                    format!(
                        "expected right stream to remain at len=1 after failed append, observed {}",
                        right_reader.len()
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

pub async fn test_subscription_delivers_live_events<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> std::sync::Arc<S> + Send + Sync + Clone + 'static,
    S: EventStore + EventSubscription + Send + Sync + 'static,
{
    use futures::StreamExt;
    use std::sync::Arc;
    use std::time::Duration;

    const SCENARIO: &str = "subscription_delivers_live_events";

    // Given: Store with some initial (historical) events
    let store: Arc<S> = make_store();
    let stream_a = contract_stream_id(SCENARIO, "stream-a")?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_a,
        StreamVersion::new(0),
    )?;
    let writes = append_contract_event(SCENARIO, writes, &stream_a)?;
    let writes = append_contract_event(SCENARIO, writes, &stream_a)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Create subscription THEN append more events
    let subscription = store
        .subscribe::<ContractTestEvent>(SubscriptionQuery::all())
        .await
        .map_err(|error| {
            ContractTestFailure::new(
                SCENARIO,
                format!("subscribe returned unexpected error: {}", error),
            )
        })?;

    // Spawn task to append MORE events AFTER subscription is created
    let store_clone = Arc::clone(&store);
    let stream_a_clone = stream_a.clone();
    let _append_task = tokio::spawn(async move {
        // Small delay to ensure subscription is consuming
        tokio::time::sleep(Duration::from_millis(10)).await;

        let writes = register_contract_stream(
            SCENARIO,
            StreamWrites::new(),
            &stream_a_clone,
            StreamVersion::new(2),
        )
        .expect("register_stream");
        let writes = append_contract_event(SCENARIO, writes, &stream_a_clone).expect("append");
        let writes = append_contract_event(SCENARIO, writes, &stream_a_clone).expect("append");

        let _ = store_clone.append_events(writes).await;
    });

    // Collect events with timeout to prevent hanging
    let timeout_duration = Duration::from_secs(2);
    let events_result =
        tokio::time::timeout(timeout_duration, subscription.take(4).collect::<Vec<_>>()).await;

    // Then: All 4 events (2 historical + 2 live) should be delivered
    let events = events_result.map_err(|_| {
        ContractTestFailure::assertion(
            SCENARIO,
            "timeout waiting for live events - subscription may not deliver events appended after creation",
        )
    })?;

    if events.len() != 4 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected 4 events (2 historical + 2 live), observed {}",
                events.len()
            ),
        ));
    }

    Ok(())
}

pub async fn test_subscription_filters_by_event_type<F, S>(make_store: F) -> ContractTestResult
where
    F: Fn() -> S + Send + Sync + Clone + 'static,
    S: EventStore + EventSubscription + Send + Sync + 'static,
{
    const SCENARIO: &str = "subscription_filters_by_event_type";

    // Given: Store contains events of TWO different types across multiple streams
    let store = make_store();
    let stream_a = contract_stream_id(SCENARIO, "stream-a")?;
    let stream_b = contract_stream_id(SCENARIO, "stream-b")?;

    let writes = register_contract_stream(
        SCENARIO,
        StreamWrites::new(),
        &stream_a,
        StreamVersion::new(0),
    )?;
    let writes = register_contract_stream(SCENARIO, writes, &stream_b, StreamVersion::new(0))?;

    // Interleave ContractTestEvent and OtherContractEvent to verify type filtering
    let contract_event = ContractTestEvent::new(stream_a.clone());
    let other_event = OtherContractEvent::new(stream_b.clone());

    let writes = builder_step(SCENARIO, "append", writes.append(contract_event))?;
    let writes = builder_step(SCENARIO, "append", writes.append(other_event))?;
    let writes = append_contract_event(SCENARIO, writes, &stream_a)?;

    let _ = store
        .append_events(writes)
        .await
        .map_err(|error| ContractTestFailure::store_error(SCENARIO, "append_events", error))?;

    // When: Subscribe with type ContractTestEvent using SubscriptionQuery::all()
    let subscription = store
        .subscribe::<ContractTestEvent>(SubscriptionQuery::all())
        .await
        .map_err(|error| {
            ContractTestFailure::new(
                SCENARIO,
                format!("subscribe returned unexpected error: {}", error),
            )
        })?;

    // Collect events manually to verify auto-filtering by E::all_type_names()
    use futures::StreamExt;
    let events: Vec<ContractTestEvent> = subscription.take(2).collect().await;

    // Then: Only ContractTestEvent events should be delivered (OtherContractEvent filtered out)
    if events.len() != 2 {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            format!(
                "expected exactly 2 ContractTestEvent events, observed {}",
                events.len()
            ),
        ));
    }

    if events.iter().any(|e| e.stream_id() != &stream_a) {
        return Err(ContractTestFailure::assertion(
            SCENARIO,
            "subscription delivered events from wrong stream (expected only stream-a)",
        ));
    }

    Ok(())
}

#[macro_export]
macro_rules! event_store_contract_tests {
    (suite = $suite:ident, make_store = $make_store:expr $(,)?) => {
        #[allow(non_snake_case)]
        mod $suite {
            use $crate::contract::{
                test_basic_read_write, test_concurrent_version_conflicts,
                test_conflict_preserves_atomicity, test_missing_stream_reads,
                test_stream_isolation,
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
        }
    };
}

pub use event_store_contract_tests;

#[macro_export]
macro_rules! event_subscription_contract_tests {
    (suite = $suite:ident, make_store = $make_store:expr $(,)?) => {
        #[allow(non_snake_case)]
        mod $suite {
            use $crate::contract::{
                test_subscription_delivers_live_events, test_subscription_filters_by_event_type,
            };

            #[tokio::test(flavor = "multi_thread")]
            async fn subscription_filters_by_event_type_contract() {
                test_subscription_filters_by_event_type($make_store)
                    .await
                    .expect("event subscription contract failed");
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn subscription_delivers_live_events_contract() {
                test_subscription_delivers_live_events(|| std::sync::Arc::new($make_store()))
                    .await
                    .expect("event subscription contract failed");
            }
        }
    };
}

pub use event_subscription_contract_tests;
