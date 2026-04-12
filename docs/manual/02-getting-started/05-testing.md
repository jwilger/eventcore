# Chapter 2.5: Testing Your Application

Testing event-sourced systems is actually easier than testing traditional CRUD applications. With EventCore, you can test commands, projections, and entire workflows using deterministic event streams.

## Testing Philosophy

EventCore testing follows these principles:

1. **Test Behavior, Not Implementation** - Focus on what events are produced
2. **Use Real Events** - Test with actual domain events, not mocks
3. **Deterministic Tests** - Events provide repeatable test scenarios
4. **Fast Feedback** - In-memory event store for rapid testing

> **Note:** The examples below use `EventCollector` from `eventcore-testing` to collect events after command execution. This is the standard pattern for verifying command behavior in tests.

## Testing Commands

### Basic Command Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::{execute, RetryPolicy, CommandError, run_projection, StreamId};
    use eventcore_memory::InMemoryEventStore;
    use eventcore_testing::EventCollector;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_create_task_success() {
        // Given: An in-memory event store
        let store = InMemoryEventStore::new();

        // And: A create task command
        let command = CreateTask {
            task_id: StreamId::try_new("task-123").unwrap(),
            title: TaskTitle::try_new("Write tests").unwrap(),
            description: TaskDescription::try_new("Add comprehensive test coverage").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };

        // When: The command is executed
        execute(&store, command, RetryPolicy::new()).await.unwrap();

        // Then: Verify events via EventCollector projection
        let storage: Arc<Mutex<Vec<TaskEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let collector = EventCollector::new(storage.clone());
        run_projection(collector, &store).await.unwrap();

        let events = storage.lock().unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            TaskEvent::Created { title, creator, .. } => {
                assert_eq!(title.as_ref(), "Write tests");
                assert_eq!(creator.as_ref(), "alice");
            }
            _ => panic!("Expected TaskCreated event"),
        }
    }

    #[tokio::test]
    async fn test_create_duplicate_task_fails() {
        // Given: An in-memory event store with an existing task
        let store = InMemoryEventStore::new();

        let command = CreateTask {
            task_id: StreamId::try_new("task-123").unwrap(),
            title: TaskTitle::try_new("Task").unwrap(),
            description: TaskDescription::try_new("").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };

        // Create first time
        execute(&store, command, RetryPolicy::new()).await.unwrap();

        // When: Try to create again with same stream
        let duplicate = CreateTask {
            task_id: StreamId::try_new("task-123").unwrap(),
            title: TaskTitle::try_new("Task").unwrap(),
            description: TaskDescription::try_new("").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };

        let result = execute(&store, duplicate, RetryPolicy::new()).await;

        // Then: Should fail with BusinessRuleViolation
        assert!(result.is_err());
        match result.unwrap_err() {
            CommandError::BusinessRuleViolation(msg) => {
                assert!(msg.contains("already exists"));
            }
            other => panic!("Expected BusinessRuleViolation, got: {:?}", other),
        }
    }
}
```

### Testing Multi-Stream Commands

```rust
#[tokio::test]
async fn test_assign_task_multi_stream() {
    // Given: A store with a created task
    let store = InMemoryEventStore::new();

    let create = CreateTask {
        task_id: StreamId::try_new("task-123").unwrap(),
        title: TaskTitle::try_new("Multi-stream test").unwrap(),
        description: TaskDescription::try_new("").unwrap(),
        creator: UserName::try_new("alice").unwrap(),
        priority: Priority::default(),
    };

    execute(&store, create, RetryPolicy::new()).await.unwrap();

    // When: The task is assigned (multi-stream command)
    let assign = AssignTask {
        task_id: StreamId::try_new("task-123").unwrap(),
        assignee_id: StreamId::try_new("user-bob").unwrap(),
        assigned_by: UserName::try_new("alice").unwrap(),
    };

    execute(&store, assign, RetryPolicy::new()).await.unwrap();

    // Then: Events from both streams can be collected
    let storage: Arc<Mutex<Vec<SystemEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store).await.unwrap();

    let events = storage.lock().unwrap();
    // Should have events from both the task and user streams
    assert!(events.len() >= 2, "expected events from multiple streams");
}
```

## Testing Projections

### Unit Testing Projections

```rust
#[tokio::test]
async fn test_projection_via_execute_and_run() {
    // Given: A store with a created and assigned task
    let store = InMemoryEventStore::new();

    let create = CreateTask {
        task_id: StreamId::try_new("task-123").unwrap(),
        title: TaskTitle::try_new("Test task").unwrap(),
        description: TaskDescription::try_new("").unwrap(),
        creator: UserName::try_new("alice").unwrap(),
        priority: Priority::default(),
    };
    execute(&store, create, RetryPolicy::new()).await.unwrap();

    let assign = AssignTask {
        task_id: StreamId::try_new("task-123").unwrap(),
        assignee_id: StreamId::try_new("user-alice").unwrap(),
        assigned_by: UserName::try_new("alice").unwrap(),
    };
    execute(&store, assign, RetryPolicy::new()).await.unwrap();

    // When: Running the projection
    let projection = UserTaskListProjection::default();
    run_projection(projection, &store).await.unwrap();

    // Then: The projection state reflects the commands executed
    // (In practice, you'd use EventCollector or inspect projection state
    //  via a shared reference pattern)
}
```

### Testing Projection Accuracy

Test projections by executing commands and then running the projection
against the resulting event store:

```rust
#[tokio::test]
async fn test_statistics_projection_accuracy() {
    // Given: A store populated by executing multiple commands
    let store = InMemoryEventStore::new();

    for i in 0..10 {
        let create = CreateTask {
            task_id: StreamId::try_new(format!("task-{}", i)).unwrap(),
            title: TaskTitle::try_new("Task").unwrap(),
            description: TaskDescription::try_new("").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };
        execute(&store, create, RetryPolicy::new()).await.unwrap();
    }

    // When: Running an EventCollector to gather events
    let storage: Arc<Mutex<Vec<SystemEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store).await.unwrap();

    // Then: All 10 creation events were captured
    let events = storage.lock().unwrap();
    assert_eq!(events.len(), 10);
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
    // Given: An in-memory event store
    let store = InMemoryEventStore::new();

    // Step 1: Create task
    let create = CreateTask {
        task_id: StreamId::try_new("task-workflow").unwrap(),
        title: TaskTitle::try_new("Complete workflow").unwrap(),
        description: TaskDescription::try_new("Test the entire flow").unwrap(),
        creator: UserName::try_new("alice").unwrap(),
        priority: Priority::default(),
    };
    execute(&store, create, RetryPolicy::new()).await.unwrap();

    // Step 2: Assign to Bob
    let assign = AssignTask {
        task_id: StreamId::try_new("task-workflow").unwrap(),
        assignee_id: StreamId::try_new("user-bob").unwrap(),
        assigned_by: UserName::try_new("alice").unwrap(),
    };
    execute(&store, assign, RetryPolicy::new()).await.unwrap();

    // Step 3: Bob completes the task
    let complete = CompleteTask {
        task_id: StreamId::try_new("task-workflow").unwrap(),
        user_id: StreamId::try_new("user-bob").unwrap(),
        completed_by: UserName::try_new("bob").unwrap(),
    };
    execute(&store, complete, RetryPolicy::new()).await.unwrap();

    // Then: Collect all events to verify the workflow
    let storage: Arc<Mutex<Vec<SystemEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store).await.unwrap();

    let events = storage.lock().unwrap();
    // Should have events from create, assign, and complete
    assert!(events.len() >= 3, "expected at least 3 events from the workflow");
}
```

## Testing Helpers

EventCore provides testing utilities:

### EventStore Contract Suite

Infrastructure authors can validate their `EventStore` implementations without
rewriting the same scenario tests. Add the `eventcore-testing` crate to your
`[dev-dependencies]` from crates.io:

```toml
[dev-dependencies]
eventcore-testing = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Then invoke the macro from `eventcore_testing::contract` in an integration test:

```rust
use eventcore_testing::contract::event_store_contract_tests;

// Generates three #[tokio::test] functions that exercise the store.
event_store_contract_tests! {
    suite = postgres_backend,
    make_store = || MyPostgresEventStore::test_instance(),
}
```

The macro emits three async tests:

- `basic_read_write_contract` – verifies a store can append and read a single
  stream without data loss.
- `concurrent_version_conflicts_contract` – appends twice with a stale expected
  version and expects `EventStoreError::VersionConflict`.
- `stream_isolation_contract` – writes events to multiple streams in one batch
  and ensures reads never bleed across stream boundaries.

Failures return structured error messages (scenario + detail) so implementors
can pinpoint missing behaviors quickly. Running these tests in CI fulfills ADR-013
and ADR-015’s requirement that every backend prove semantic correctness.

### EventCollector Pattern

The recommended testing pattern uses `EventCollector` from `eventcore-testing`
to collect events after command execution:

```rust
use eventcore::{execute, RetryPolicy, run_projection};
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::EventCollector;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_with_event_collector() {
    // Execute commands against the store
    let store = InMemoryEventStore::new();
    execute(&store, my_command, RetryPolicy::new()).await.unwrap();

    // Collect events via projection
    let storage: Arc<Mutex<Vec<MyEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store).await.unwrap();

    // Assert on collected events
    let events = storage.lock().unwrap();
    assert_eq!(events.len(), 1);
}
```

### Test Scenarios

See `eventcore-examples/tests/` for complete working examples including:

- `single_stream_command_test.rs` -- basic command execution and error handling
- `multi_stream_atomic_test.rs` -- multi-stream atomicity and concurrent transfers
- `retry_policy_test.rs` -- retry behavior on version conflicts
- `scenario_test.rs` -- GWT-style testing with `TestScenario`

## Testing Error Cases

### Command Validation Errors

```rust
#[test]
fn test_invalid_domain_types() {
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
    let store = Arc::new(InMemoryEventStore::new());

    // Create a task
    let create = CreateTask {
        task_id: StreamId::try_new("task-concurrent").unwrap(),
        title: TaskTitle::try_new("Concurrent test").unwrap(),
        description: TaskDescription::try_new("").unwrap(),
        creator: UserName::try_new("alice").unwrap(),
        priority: Priority::default(),
    };
    execute(store.as_ref(), create, RetryPolicy::new()).await.unwrap();

    // Simulate concurrent assignments
    let store1 = Arc::clone(&store);
    let store2 = Arc::clone(&store);

    let assign1 = AssignTask {
        task_id: StreamId::try_new("task-concurrent").unwrap(),
        assignee_id: StreamId::try_new("user-bob").unwrap(),
        assigned_by: UserName::try_new("alice").unwrap(),
    };
    let assign2 = AssignTask {
        task_id: StreamId::try_new("task-concurrent").unwrap(),
        assignee_id: StreamId::try_new("user-charlie").unwrap(),
        assigned_by: UserName::try_new("alice").unwrap(),
    };

    // Execute both concurrently -- RetryPolicy handles version conflicts
    let (result1, result2) = tokio::join!(
        execute(store1.as_ref(), assign1, RetryPolicy::new()),
        execute(store2.as_ref(), assign2, RetryPolicy::new()),
    );

    // Both should succeed (one may retry due to version conflict)
    assert!(result1.is_ok() || result2.is_ok());
}
```

## Performance Testing

```rust
#[tokio::test]
#[ignore] // Run with --ignored flag
async fn test_high_volume_command_execution() {
    use std::time::Instant;

    let store = InMemoryEventStore::new();
    let command_count = 1_000;

    let start = Instant::now();

    for i in 0..command_count {
        let command = CreateTask {
            task_id: StreamId::try_new(format!("task-perf-{}", i)).unwrap(),
            title: TaskTitle::try_new("Perf test").unwrap(),
            description: TaskDescription::try_new("").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };
        execute(&store, command, RetryPolicy::new()).await.unwrap();
    }

    let duration = start.elapsed();
    let ops_per_second = command_count as f64 / duration.as_secs_f64();

    println!("Executed {} commands in {:?}", command_count, duration);
    println!("Rate: {:.2} ops/second", ops_per_second);
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
    // Given: An event store with some commands executed
    let store = InMemoryEventStore::new();

    // ... execute commands ...

    // Then: Collect all events for debugging
    let storage: Arc<Mutex<Vec<MyEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = EventCollector::new(storage.clone());
    run_projection(collector, &store).await.unwrap();

    let events = storage.lock().unwrap();
    for event in events.iter() {
        println!("Event: {:?}", event);
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
