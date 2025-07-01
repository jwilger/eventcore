//! Tests for concurrent stream creation and optimistic concurrency control.

use eventcore::{
    Command, CommandError, CommandExecutor, CommandResult, EventStore, ExecutionOptions,
    ReadStreams, StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum TestEvent {
    Created { value: String },
}

impl<'a> TryFrom<&'a Self> for TestEvent {
    type Error = &'static str;

    fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Debug, Default)]
struct TestState {
    exists: bool,
}

#[derive(Debug)]
struct CreateCommand;

#[derive(Debug, Clone)]
struct CreateInput {
    stream_id: StreamId,
    value: String,
}

#[async_trait::async_trait]
impl Command for CreateCommand {
    type Input = CreateInput;
    type State = TestState;
    type Event = TestEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
        state.exists = true;
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.exists {
            return Err(CommandError::BusinessRuleViolation(
                "Already exists".to_string(),
            ));
        }

        Ok(vec![StreamWrite::new(
            &read_streams,
            input.stream_id,
            TestEvent::Created { value: input.value },
        )?])
    }
}

#[tokio::test]
async fn test_concurrent_creation_with_retry() {
    // This test verifies that optimistic concurrency control works correctly:
    // 1. Two commands try to create the same stream concurrently
    // 2. One gets a version conflict and is automatically retried by the executor
    // 3. On retry, it sees the stream exists and returns BusinessRuleViolation
    // 4. The other command succeeds normally
    // 5. Only one event is actually written to the stream

    // Create PostgreSQL store
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let config = PostgresConfig::new(database_url);

    let event_store = PostgresEventStore::<TestEvent>::new(config)
        .await
        .expect("Failed to create PostgreSQL event store");

    let executor = Arc::new(CommandExecutor::new(event_store));

    // Test concurrent creation of the same stream
    let stream_id = StreamId::try_new(format!(
        "test-{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ))
    .unwrap();

    // Spawn two concurrent operations trying to create the same stream
    let executor1 = executor.clone();
    let stream_id1 = stream_id.clone();
    let handle1 = tokio::spawn(async move {
        executor1
            .execute(
                &CreateCommand,
                CreateInput {
                    stream_id: stream_id1,
                    value: "value1".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    });

    let executor2 = executor.clone();
    let stream_id2 = stream_id.clone();
    let handle2 = tokio::spawn(async move {
        executor2
            .execute(
                &CreateCommand,
                CreateInput {
                    stream_id: stream_id2,
                    value: "value2".to_string(),
                },
                ExecutionOptions::default(),
            )
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    // One should succeed, one should fail with BusinessRuleViolation after retry
    // This is the correct behavior: the executor retries on ConcurrencyConflict,
    // reads the updated state, and the business logic correctly detects "Already exists"
    assert!(
        (result1.is_ok()
            && matches!(result2, Err(CommandError::BusinessRuleViolation(ref msg)) if msg == "Already exists"))
            || (result2.is_ok()
                && matches!(result1, Err(CommandError::BusinessRuleViolation(ref msg)) if msg == "Already exists")),
        "Expected one success and one BusinessRuleViolation('Already exists'), got: {result1:?} and {result2:?}"
    );

    // Check that there's exactly one event in the stream
    let stream_data = executor
        .event_store()
        .read_streams(&[stream_id], &eventcore::ReadOptions::default())
        .await
        .unwrap();

    assert_eq!(
        stream_data.events.len(),
        1,
        "Expected exactly 1 event, found {}",
        stream_data.events.len()
    );
}
