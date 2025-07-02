//! Focused test for retry behavior - ensures commands are fully re-executed on retry
//!
//! This test verifies that when a command encounters a version conflict and retries,
//! it re-reads the stream, rebuilds state, and re-executes the command logic.
//! Without this, commands can violate business rules by writing events based on stale state.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use eventcore::{
    Command, CommandError, CommandExecutor, CommandResult, ExecutionOptions, ReadStreams,
    StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum TestEvent {
    Created { id: usize },
    Updated { id: usize },
}

// Required for EventStore implementation
impl<'a> TryFrom<&'a Self> for TestEvent {
    type Error = &'static str;

    fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Debug, Default, Clone)]
struct TestState {
    exists: bool,
    creation_id: Option<usize>,
    update_count: usize,
}

/// A command that tracks how many times it's executed
#[derive(Debug)]
struct TestCommand {
    execution_count: Arc<AtomicUsize>,
}

impl TestCommand {
    fn new() -> Self {
        Self {
            execution_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Debug, Clone)]
struct TestInput {
    stream_id: StreamId,
    command_id: usize,
    should_create: bool,
}

#[async_trait::async_trait]
impl Command for TestCommand {
    type Input = TestInput;
    type State = TestState;
    type Event = TestEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.stream_id.clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TestEvent::Created { id } => {
                state.exists = true;
                state.creation_id = Some(*id);
            }
            TestEvent::Updated { id: _ } => {
                state.update_count += 1;
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Increment execution count every time handle is called
        let exec_count = self.execution_count.fetch_add(1, Ordering::SeqCst) + 1;
        eprintln!(
            "TestCommand execution #{} for command_id={}, state.exists={}, state.creation_id={:?}",
            exec_count, input.command_id, state.exists, state.creation_id
        );

        // Business logic: only allow creation if stream doesn't exist
        if input.should_create {
            if state.exists {
                // This is the key check - if we see the stream exists, we should fail
                eprintln!("TestCommand: Stream exists, failing with BusinessRuleViolation");
                return Err(CommandError::BusinessRuleViolation(format!(
                    "Stream already exists with creation_id={:?}",
                    state.creation_id
                )));
            }
            eprintln!("TestCommand: Stream doesn't exist, creating event");
            Ok(vec![StreamWrite::new(
                &read_streams,
                input.stream_id,
                TestEvent::Created {
                    id: input.command_id,
                },
            )?])
        } else {
            // Update is always allowed
            eprintln!("TestCommand: Updating event");
            Ok(vec![StreamWrite::new(
                &read_streams,
                input.stream_id,
                TestEvent::Updated {
                    id: input.command_id,
                },
            )?])
        }
    }
}

#[tokio::test]
async fn test_concurrent_creation_business_rule() {
    // This is a simpler version of the concurrent bug test
    // It verifies that our business rule is enforced even with retries
    let event_store = InMemoryEventStore::<TestEvent>::new();
    let executor = Arc::new(CommandExecutor::new(event_store));

    let stream_id = StreamId::try_new("business-rule-test").unwrap();

    // Spawn two tasks that try to create the same stream
    let executor1 = executor.clone();
    let stream_id1 = stream_id.clone();
    let handle1 = tokio::spawn(async move {
        let command = TestCommand::new();
        executor1
            .execute(
                &command,
                TestInput {
                    stream_id: stream_id1,
                    command_id: 100,
                    should_create: true,
                },
                ExecutionOptions::default(),
            )
            .await
    });

    let executor2 = executor.clone();
    let stream_id2 = stream_id.clone();
    let handle2 = tokio::spawn(async move {
        let command = TestCommand::new();
        executor2
            .execute(
                &command,
                TestInput {
                    stream_id: stream_id2,
                    command_id: 200,
                    should_create: true,
                },
                ExecutionOptions::default(),
            )
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    // Exactly one should succeed
    let success_count = [&result1, &result2].iter().filter(|r| r.is_ok()).count();
    assert_eq!(success_count, 1, "Exactly one command should succeed");

    // The other should fail with business rule violation
    let has_business_violation = [&result1, &result2]
        .iter()
        .any(|r| matches!(r, Err(CommandError::BusinessRuleViolation(_))));
    assert!(
        has_business_violation,
        "One command should fail with business rule violation"
    );
}
