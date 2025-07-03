//! Tests for investigating duplicate event ID behavior in concurrent scenarios.

use eventcore::{
    CommandError, CommandExecutor, CommandLogic, CommandResult, CommandStreams, ExecutionOptions,
    ReadStreams, StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum TestEvent {
    ValueSet { value: u32 },
}

impl<'a> TryFrom<&'a Self> for TestEvent {
    type Error = &'static str;

    fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Debug, Default)]
struct TestState {
    current_value: u32,
}

#[derive(Debug, Clone)]
struct SetValueCommand {
    stream_id: StreamId,
    value: u32,
}

impl CommandStreams for SetValueCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.stream_id.clone()]
    }
}

#[async_trait::async_trait]
impl CommandLogic for SetValueCommand {
    type State = TestState;
    type Event = TestEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TestEvent::ValueSet { value } => state.current_value = *value,
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Add a small delay to increase chance of concurrent writes
        tokio::time::sleep(Duration::from_millis(10)).await;

        Ok(vec![StreamWrite::new(
            &read_streams,
            self.stream_id.clone(),
            TestEvent::ValueSet { value: self.value },
        )?])
    }
}

#[tokio::test]
async fn test_duplicate_event_id_investigation() {
    // Use tracing for logging
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // Create PostgreSQL store
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let config = PostgresConfig::new(database_url);

    let event_store = PostgresEventStore::<TestEvent>::new(config)
        .await
        .expect("Failed to create PostgreSQL event store");

    // Initialize database tables
    event_store
        .initialize()
        .await
        .expect("Failed to initialize database");

    let executor = Arc::new(CommandExecutor::new(event_store));

    // Use a unique stream for this test
    let stream_id = StreamId::try_new(format!(
        "test-{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ))
    .unwrap();

    // Launch multiple concurrent operations on the same stream
    let mut handles = vec![];
    for i in 0..5 {
        let executor = executor.clone();
        let stream_id = stream_id.clone();
        let value = i + 1;

        let handle = tokio::spawn(async move {
            println!("Operation {i} starting");
            let result = executor
                .execute(
                    SetValueCommand { stream_id, value },
                    ExecutionOptions::default(),
                )
                .await;
            println!("Operation {i} result: {result:?}");
            (i, result)
        });

        handles.push(handle);
    }

    // Wait for all operations
    let results: Vec<_> = futures::future::join_all(handles).await;

    // Analyze results
    let mut successes = 0;
    let mut concurrency_conflicts = 0;
    let mut duplicate_event_ids = 0;
    let mut other_errors = 0;

    for join_result in results {
        let (idx, result) = join_result.expect("Task should not panic");
        match result {
            Ok(_) => {
                successes += 1;
                println!("Operation {idx} succeeded");
            }
            Err(CommandError::ConcurrencyConflict { .. }) => {
                concurrency_conflicts += 1;
                println!("Operation {idx} had concurrency conflict");
            }
            Err(CommandError::EventStore(eventcore::EventStoreError::DuplicateEventId(id))) => {
                duplicate_event_ids += 1;
                println!("Operation {idx} had duplicate event ID: {id:?}");
            }
            Err(e) => {
                other_errors += 1;
                println!("Operation {idx} had other error: {e:?}");
            }
        }
    }

    println!("\nResults summary:");
    println!("Successes: {successes}");
    println!("Concurrency conflicts: {concurrency_conflicts}");
    println!("Duplicate event IDs: {duplicate_event_ids}");
    println!("Other errors: {other_errors}");

    // The critical test is that we have NO duplicate event IDs (which would indicate a race condition)
    assert_eq!(
        duplicate_event_ids, 0,
        "Should not have any duplicate event IDs - this is the critical test"
    );

    // With high contention and default retry (3 attempts), not all operations will succeed
    // This is expected behavior - some will exhaust their retries
    assert!(successes >= 1, "At least one operation should succeed");
    assert!(
        successes + concurrency_conflicts == 5,
        "All operations should either succeed or fail with concurrency conflicts"
    );
    assert_eq!(other_errors, 0, "Should not have any other errors");
}
