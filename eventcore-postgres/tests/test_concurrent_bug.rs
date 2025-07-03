//! Tests for concurrent stream creation and optimistic concurrency control.

use eventcore::{
    CommandError, CommandExecutor, CommandResult, EventStore, ExecutionOptions,
    ReadStreams, StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Barrier;

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
    event_count: usize,
}

#[derive(Debug, Clone)]
struct CreateCommand {
    stream_id: StreamId,
    value: String,
}

impl eventcore::CommandStreams for CreateCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.stream_id.clone()]
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for CreateCommand {
    type State = TestState;
    type Event = TestEvent;

    fn apply(&self, state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
        state.exists = true;
        state.event_count += 1;
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // This command models "create if not exists" behavior
        // It should only succeed in creating the stream (writing the first event)
        // If the stream already exists, it's a business rule violation
        if state.exists {
            return Err(CommandError::BusinessRuleViolation(
                "Stream already exists".to_string(),
            ));
        }

        // Try to create the stream with the first event
        Ok(vec![StreamWrite::new(
            &read_streams,
            self.stream_id.clone(),
            TestEvent::Created { value: self.value.clone() },
        )?])
    }
}

#[tokio::test]
async fn test_concurrent_creation_with_retry() {
    // This test verifies that when two commands try to create the same stream:
    // 1. Exactly one command succeeds in creating the stream (version 1)
    // 2. The other command fails with BusinessRuleViolation
    //
    // The test uses a barrier to increase the likelihood of true concurrent
    // execution, but exact timing cannot be guaranteed.

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

    // Test concurrent creation of the same stream multiple times to ensure reliability
    for test_run in 0..2 {
        // Reduced to 2 iterations to debug
        // Use unique stream ID for each test run
        let stream_id = StreamId::try_new(format!(
            "concurrent-test-{}-{}",
            test_run,
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ))
        .unwrap();

        // Use a barrier to ensure both commands start at the same time
        let barrier = Arc::new(Barrier::new(2));

        // Spawn two concurrent operations trying to create the same stream
        let executor1 = executor.clone();
        let stream_id1 = stream_id.clone();
        let barrier1 = barrier.clone();
        let handle1 = tokio::spawn(async move {
            // Wait for both tasks to be ready
            barrier1.wait().await;

            let command = CreateCommand {
                stream_id: stream_id1.clone(),
                value: "value1".to_string(),
            };
            let result = executor1
                .execute(&command, ExecutionOptions::default())
                .await;
            result
        });

        let executor2 = executor.clone();
        let stream_id2 = stream_id.clone();
        let barrier2 = barrier.clone();
        let handle2 = tokio::spawn(async move {
            // Wait for both tasks to be ready
            barrier2.wait().await;

            let command = CreateCommand {
                stream_id: stream_id2.clone(),
                value: "value2".to_string(),
            };
            let result = executor2
                .execute(&command, ExecutionOptions::default())
                .await;
            result
        });

        let result1 = handle1.await.unwrap();
        let result2 = handle2.await.unwrap();

        // Check the results. We expect one of these scenarios:
        // 1. Both commands race: one succeeds (v1), one retries and fails with BusinessRuleViolation
        // 2. Sequential execution: one succeeds (v1), the other reads existing stream and fails
        //
        // Note: We previously saw a bug where the second command would succeed with v2,
        // but this should not happen with our "create if not exists" business logic.
        let success_count = [&result1, &result2].iter().filter(|r| r.is_ok()).count();
        let business_violation_count = [&result1, &result2]
            .iter()
            .filter(|r| matches!(r, Err(CommandError::BusinessRuleViolation(ref msg)) if msg == "Stream already exists"))
            .count();

        assert_eq!(
            success_count, 1,
            "Test run {test_run}: Expected exactly 1 success, got {success_count}. Results: {result1:?} and {result2:?}"
        );
        assert_eq!(
            business_violation_count, 1,
            "Test run {test_run}: Expected exactly 1 BusinessRuleViolation, got {business_violation_count}. Results: {result1:?} and {result2:?}"
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
            "Test run {}: Expected exactly 1 event, found {}",
            test_run,
            stream_data.events.len()
        );
    }
}

#[tokio::test]
async fn test_concurrent_creation_stress() {
    // Stress test: Multiple commands trying to create the same stream
    // This should reliably enforce the business invariant that only one succeeds

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string());

    let config = PostgresConfig::new(database_url);

    let event_store = PostgresEventStore::<TestEvent>::new(config)
        .await
        .expect("Failed to create PostgreSQL event store");

    event_store
        .initialize()
        .await
        .expect("Failed to initialize database");

    let executor = Arc::new(CommandExecutor::new(event_store));

    let stream_id = StreamId::try_new(format!(
        "stress-test-{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ))
    .unwrap();

    // Spawn 10 concurrent operations trying to create the same stream
    let mut handles = vec![];
    for i in 0..10 {
        let executor_clone = executor.clone();
        let stream_id_clone = stream_id.clone();
        let handle = tokio::spawn(async move {
            let command = CreateCommand {
                stream_id: stream_id_clone,
                value: format!("value{i}"),
            };
            executor_clone
                .execute(&command, ExecutionOptions::default())
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let results: Vec<_> = futures::future::join_all(handles).await;

    // Count outcomes
    let mut success_count = 0;
    let mut business_violation_count = 0;
    let mut other_error_count = 0;

    for result in results {
        match result.unwrap() {
            Ok(_) => success_count += 1,
            Err(CommandError::BusinessRuleViolation(ref msg)) if msg == "Stream already exists" => {
                business_violation_count += 1;
            }
            Err(e) => {
                other_error_count += 1;
                eprintln!("Unexpected error: {e:?}");
            }
        }
    }

    // Only one should succeed, all others should fail with business rule violation
    assert_eq!(success_count, 1, "Expected exactly 1 success");
    assert_eq!(
        business_violation_count, 9,
        "Expected exactly 9 business rule violations"
    );
    assert_eq!(other_error_count, 0, "Expected no other types of errors");

    // Verify exactly one event in the stream
    let stream_data = executor
        .event_store()
        .read_streams(&[stream_id], &eventcore::ReadOptions::default())
        .await
        .unwrap();

    assert_eq!(stream_data.events.len(), 1, "Expected exactly 1 event");
}