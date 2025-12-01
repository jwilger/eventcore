# Chapter 2.5: Testing Your Application

Testing event-sourced systems is actually easier than testing traditional CRUD applications. With EventCore, you can test commands, projections, and entire workflows using deterministic event streams.

## Testing Philosophy

EventCore testing follows these principles:

1. **Test Behavior, Not Implementation** - Focus on what events are produced
2. **Use Real Events** - Test with actual domain events, not mocks
3. **Deterministic Tests** - Events provide repeatable test scenarios
4. **Fast Feedback** - In-memory event store for rapid testing

> **Note:** The examples below reference helper types such as `StoredEventBuilder` and `create_test_event`. Until `eventcore-testing` grows those fixtures, treat them as utilities you define inside your own tests.

## Testing Commands

### Basic Command Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::prelude::*;
    use eventcore_memory::InMemoryEventStore;

    #[tokio::test]
    async fn test_create_task_success() {
        // Arrange
        let store = InMemoryEventStore::<SystemEvent>::new();
        let executor = CommandExecutor::new(store);

        let task_id = TaskId::new();
        let command = CreateTask::new(
            task_id,
            TaskTitle::try_new("Write tests").unwrap(),
            TaskDescription::try_new("Add comprehensive test coverage").unwrap(),
            UserName::try_new("alice").unwrap(),
        ).unwrap();

        // Act
        let result = executor.execute(&command).await;

        // Assert
        assert!(result.is_ok());
        let execution_result = result.unwrap();
        assert_eq!(execution_result.events_written.len(), 1);

        // Verify the event
        match &execution_result.events_written[0] {
            SystemEvent::Task(TaskEvent::Created { title, creator, .. }) => {
                assert_eq!(title.as_ref(), "Write tests");
                assert_eq!(creator.as_ref(), "alice");
            }
            _ => panic!("Expected TaskCreated event"),
        }
    }

    #[tokio::test]
    async fn test_create_duplicate_task_fails() {
        // Arrange
        let store = InMemoryEventStore::<SystemEvent>::new();
        let executor = CommandExecutor::new(store);

        let task_id = TaskId::new();
        let command = CreateTask::new(
            task_id,
            TaskTitle::try_new("Task").unwrap(),
            TaskDescription::try_new("").unwrap(),
            UserName::try_new("alice").unwrap(),
        ).unwrap();

        // Act - Create first time
        executor.execute(&command).await.unwrap();

        // Act - Try to create again
        let result = executor.execute(&command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CommandError::ValidationFailed(msg) => {
                assert!(msg.contains("already exists"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }
}
```

### Testing Multi-Stream Commands

```rust
#[tokio::test]
async fn test_assign_task_multi_stream() {
    // Arrange
    let store = InMemoryEventStore::<SystemEvent>::new();
    let executor = CommandExecutor::new(store);

    // Create a task first
    let task_id = TaskId::new();
    let create = CreateTask::new(
        task_id,
        TaskTitle::try_new("Multi-stream test").unwrap(),
        TaskDescription::try_new("").unwrap(),
        UserName::try_new("alice").unwrap(),
    ).unwrap();

    executor.execute(&create).await.unwrap();

    // Assign the task
    let assign = AssignTask::new(
        task_id,
        UserName::try_new("bob").unwrap(),
        UserName::try_new("alice").unwrap(),
    ).unwrap();

    // Act
    let result = executor.execute(&assign).await.unwrap();

    // Assert - Should affect both task and user streams
    assert_eq!(result.streams_affected.len(), 2);
    assert!(result.streams_affected.contains(&StreamId::from_static(&format!("task-{}", task_id))));
    assert!(result.streams_affected.contains(&StreamId::from_static("user-bob")));

    // Verify events in both streams
    let task_events = store.read_stream(
        &StreamId::from_static(&format!("task-{}", task_id)),
        ReadOptions::default()
    ).await.unwrap();

    let user_events = store.read_stream(
        &StreamId::from_static("user-bob"),
        ReadOptions::default()
    ).await.unwrap();

    assert_eq!(task_events.events.len(), 2); // Created + Assigned
    assert_eq!(user_events.events.len(), 2); // TaskAssigned + WorkloadUpdated
}
```

## Testing Projections

### Unit Testing Projections

```rust
#[tokio::test]
async fn test_user_task_list_projection() {
    // StoredEventBuilder is a project-specific helper shown here for illustration.

    // Arrange
    let mut projection = UserTaskListProjection::default();
    let task_id = TaskId::new();
    let alice = UserName::try_new("alice").unwrap();

    // Build test events
    let events = vec![
        StoredEventBuilder::new()
            .with_stream_id(StreamId::from_static("task-123"))
            .with_payload(SystemEvent::Task(TaskEvent::Created {
                task_id,
                title: TaskTitle::try_new("Test task").unwrap(),
                description: TaskDescription::try_new("").unwrap(),
                creator: alice.clone(),
                created_at: Utc::now(),
            }))
            .build(),
        StoredEventBuilder::new()
            .with_stream_id(StreamId::from_static("task-123"))
            .with_payload(SystemEvent::Task(TaskEvent::Assigned {
                task_id,
                assignee: alice.clone(),
                assigned_by: alice.clone(),
                assigned_at: Utc::now(),
            }))
            .build(),
    ];

    // Act
    for event in events {
        projection.apply(&event).await.unwrap();
    }

    // Assert
    let tasks = projection.get_user_tasks(&alice);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task_id);
    assert_eq!(tasks[0].status, TaskStatus::Open);
}
```

### Testing Projection Accuracy

```rust
#[tokio::test]
async fn test_statistics_projection_accuracy() {
    let mut projection = TeamStatisticsProjection::default();

    // Create a series of events
    let events = create_test_scenario(TestScenario {
        tasks_created: 10,
        tasks_assigned: 8,
        tasks_completed: 5,
        users: vec!["alice", "bob", "charlie"],
    });

    // Apply all events
    for event in events {
        projection.apply(&event).await.unwrap();
    }

    // Verify statistics
    assert_eq!(projection.total_tasks_created, 10);
    assert_eq!(projection.tasks_by_status[&TaskStatus::Completed], 5);
    assert_eq!(projection.tasks_by_status[&TaskStatus::Open], 2); // 10 - 8 assigned
    assert_eq!(projection.tasks_by_status[&TaskStatus::InProgress], 3); // 8 - 5 completed

    // Verify completion rate
    assert_eq!(projection.completion_rate(), 50.0); // 5/10 * 100
}
```

## Property-Based Testing

EventCore works well with property-based testing:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn task_assignment_maintains_consistency(
        task_count in 1..50usize,
        user_count in 1..10usize,
        assignment_ratio in 0.0..1.0f64,
    ) {
        // Property: Total assigned tasks equals sum of user assignments
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut projection = UserTaskListProjection::default();
            let users = generate_users(user_count);
            let tasks = generate_tasks(task_count);

            // Assign tasks based on ratio
            let assignments = assign_tasks_to_users(&tasks, &users, assignment_ratio);

            // Apply events
            for event in assignments {
                projection.apply(&event).await.unwrap();
            }

            // Verify consistency
            let total_assigned: usize = users.iter()
                .map(|u| projection.get_user_tasks(u).len())
                .sum();

            let expected_assigned = (task_count as f64 * assignment_ratio) as usize;
            assert_eq!(total_assigned, expected_assigned);
        });
    }
}
```

## Integration Testing

### Testing Complete Workflows

```rust
#[tokio::test]
async fn test_complete_task_workflow() {
    // Setup
    let store = InMemoryEventStore::<SystemEvent>::new();
    let executor = CommandExecutor::new(store.clone());
    let mut projection = UserTaskListProjection::default();

    // Execute workflow
    let task_id = TaskId::new();
    let alice = UserName::try_new("alice").unwrap();
    let bob = UserName::try_new("bob").unwrap();

    // 1. Create task
    let create = CreateTask::new(
        task_id,
        TaskTitle::try_new("Complete workflow").unwrap(),
        TaskDescription::try_new("Test the entire flow").unwrap(),
        alice.clone(),
    ).unwrap();
    executor.execute(&create).await.unwrap();

    // 2. Assign to Bob
    let assign = AssignTask::new(task_id, bob.clone(), alice.clone()).unwrap();
    executor.execute(&assign).await.unwrap();

    // 3. Bob completes the task
    let complete = CompleteTask {
        task_id: StreamId::from_static(&format!("task-{}", task_id)),
        user_id: StreamId::from_static(&format!("user-{}", bob)),
        completed_by: bob.clone(),
    };
    executor.execute(&complete).await.unwrap();

    // Update projection with all events
    let all_events = store.read_all_events(ReadOptions::default()).await.unwrap();
    for event in all_events {
        projection.apply(&event).await.unwrap();
    }

    // Verify end state
    let bob_tasks = projection.get_user_tasks(&bob);
    assert_eq!(bob_tasks.len(), 1);
    assert_eq!(bob_tasks[0].status, TaskStatus::Completed);
    assert!(bob_tasks[0].completed_at.is_some());
}
```

## Testing Helpers

EventCore provides testing utilities:

### Event Builders

```rust
// Helper builders live alongside your tests until shared fixtures land.

fn create_test_event(payload: SystemEvent) -> StoredEvent<SystemEvent> {
    StoredEventBuilder::new()
        .with_id(EventId::new())
        .with_stream_id(StreamId::from_static("test-stream"))
        .with_version(EventVersion::new(1))
        .with_payload(payload)
        .with_metadata(
            EventMetadataBuilder::new()
                .with_user_id(UserId::from("test-user"))
                .build()
        )
        .build()
}
```

### Test Scenarios

```rust
// Define your own scenario helpers to keep tests readable.

struct TaskScenario;

impl TestScenario for TaskScenario {
    type Event = SystemEvent;

    fn events(&self) -> Vec<EventToWrite<Self::Event>> {
        vec![
            // Series of events that create a test scenario
            create_task_event("task-1", "Test Task 1"),
            assign_task_event("task-1", "alice"),
            complete_task_event("task-1", "alice"),
        ]
    }
}
```

### Assertion Helpers

```rust
// Assertions helpers can also live locally until the crate exposes them.

#[tokio::test]
async fn test_event_ordering() {
    let events = vec![/* ... */];

    // Assert events are properly ordered
    assert_events_ordered(&events);

    // Assert no duplicate event IDs
    assert_unique_event_ids(&events);

    // Assert version progression
    assert_stream_version_progression(&events, &StreamId::from_static("test"));
}
```

## Testing Error Cases

### Command Validation Errors

```rust
#[tokio::test]
async fn test_invalid_command_inputs() {
    let executor = CommandExecutor::new(InMemoryEventStore::<SystemEvent>::new());

    // Test empty title
    let result = TaskTitle::try_new("");
    assert!(result.is_err());

    // Test whitespace-only title
    let result = TaskTitle::try_new("   ");
    assert!(result.is_err());

    // Test overly long description
    let long_desc = "x".repeat(3000);
    let result = TaskDescription::try_new(&long_desc);
    assert!(result.is_err());
}
```

### Concurrency Conflicts

```rust
#[tokio::test]
async fn test_concurrent_modifications() {
    let store = InMemoryEventStore::<SystemEvent>::new();
    let executor = CommandExecutor::new(store);

    // Create a task
    let task_id = TaskId::new();
    let create = CreateTask::new(
        task_id,
        TaskTitle::try_new("Concurrent test").unwrap(),
        TaskDescription::try_new("").unwrap(),
        UserName::try_new("alice").unwrap(),
    ).unwrap();
    executor.execute(&create).await.unwrap();

    // Simulate concurrent updates
    let assign1 = AssignTask::new(task_id, UserName::try_new("bob").unwrap(), UserName::try_new("alice").unwrap()).unwrap();
    let assign2 = AssignTask::new(task_id, UserName::try_new("charlie").unwrap(), UserName::try_new("alice").unwrap()).unwrap();

    // Execute both concurrently
    let (result1, result2) = tokio::join!(
        executor.execute(&assign1),
        executor.execute(&assign2)
    );

    // One should succeed, one should retry and then succeed
    assert!(result1.is_ok() || result2.is_ok());
}
```

## Performance Testing

```rust
#[tokio::test]
#[ignore] // Run with --ignored flag
async fn test_high_volume_event_processing() {
    use std::time::Instant;

    let mut projection = UserTaskListProjection::default();
    let event_count = 10_000;

    // Generate events
    let events: Vec<_> = (0..event_count)
        .map(|i| create_task_assigned_event(i))
        .collect();

    // Measure processing time
    let start = Instant::now();

    for event in events {
        projection.apply(&event).await.unwrap();
    }

    let duration = start.elapsed();
    let events_per_second = event_count as f64 / duration.as_secs_f64();

    println!("Processed {} events in {:?}", event_count, duration);
    println!("Rate: {:.2} events/second", events_per_second);

    // Assert reasonable performance
    assert!(events_per_second > 1000.0, "Projection too slow");
}
```

## Test Organization

Structure your tests for clarity:

```
tests/
├── unit/
│   ├── commands/
│   │   ├── create_task_test.rs
│   │   ├── assign_task_test.rs
│   │   └── complete_task_test.rs
│   └── projections/
│       ├── task_list_test.rs
│       └── statistics_test.rs
├── integration/
│   ├── workflows/
│   │   └── task_lifecycle_test.rs
│   └── projections/
│       └── real_time_updates_test.rs
└── performance/
    └── high_volume_test.rs
```

## Debugging Tests

EventCore provides excellent debugging support:

```rust
#[tokio::test]
async fn test_with_debugging() {
    // Enable debug logging
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let store = InMemoryEventStore::<SystemEvent>::new();

    // Print all events after execution
    let events = store.read_all_events(ReadOptions::default()).await.unwrap();

    for event in &events {
        println!("Event: {:?}", event);
        println!("  Stream: {}", event.stream_id);
        println!("  Version: {}", event.version);
        println!("  Payload: {:?}", event.payload);
        println!("  Metadata: {:?}", event.metadata);
        println!();
    }
}
```

## Summary

Testing EventCore applications is straightforward because:

- ✅ **Events are deterministic** - Same events always produce same state
- ✅ **No mocking needed** - Use real events and in-memory stores
- ✅ **Fast feedback** - In-memory testing is instantaneous
- ✅ **Complete scenarios** - Test entire workflows easily
- ✅ **Time travel** - Test any historical state

Best practices:

1. Test commands by verifying produced events
2. Test projections by applying known events
3. Use property-based testing for invariants
4. Test complete workflows for integration
5. Keep tests fast with in-memory stores

You've now completed the Getting Started tutorial! You can:

- Model domains with events
- Implement type-safe commands
- Build projections for queries
- Test everything thoroughly

Continue to [Part 3: Core Concepts](../03-core-concepts/README.md) for deeper understanding →
