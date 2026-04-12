# Chapter 2.3: Implementing Commands

Now we'll implement the commands we discovered during domain modeling. EventCore's macro system makes this straightforward while maintaining type safety.

## Command Structure

Every EventCore command follows this pattern:

1. **Derive the Command macro** - Generates boilerplate
2. **Declare streams with #[stream]** - Define what streams you need
3. **Implement CommandLogic** - Your business logic
4. **Generate events** - What happened as a result

## Our First Command: Create Task

Let's implement task creation:

### `src/domain/commands/create_task.rs`

```rust
use crate::domain::{events::*, types::*};
use chrono::Utc;
use eventcore::{Command, CommandError, CommandLogic, NewEvents, StreamId};

/// Command to create a new task
#[derive(Command)]
pub struct CreateTask {
    /// The task stream - will contain all task events
    #[stream]
    pub task_id: StreamId,

    /// Task details
    pub title: TaskTitle,
    pub description: TaskDescription,
    pub creator: UserName,
    pub priority: Priority,
}

/// State for create task command - tracks if task exists
#[derive(Default)]
pub struct CreateTaskState {
    exists: bool,
}

impl CreateTaskState {
    pub fn exists(&self) -> bool {
        self.exists
    }
}

impl CommandLogic for CreateTask {
    type State = CreateTaskState;
    type Event = TaskEvent;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        match event {
            TaskEvent::Created { .. } => {
                state.exists = true;
            }
            _ => {} // Other events don't affect creation
        }
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Business rule: Can't create a task that already exists
        if state.exists() {
            return Err(CommandError::business_rule_violated(
                format!("Task {} already exists", self.task_id.as_ref())
            ));
        }

        // Generate the TaskCreated event
        let event = TaskEvent::Created {
            task_id: TaskId::from(&self.task_id),
            title: self.title.clone(),
            description: self.description.clone(),
            creator: self.creator.clone(),
            created_at: Utc::now(),
        };

        // Return domain events. The executor maps events to streams via event.stream_id().
        Ok(vec![event].into())
    }
}
```

### Key Points

1. **#[derive(Command)]** generates the `CommandStreams` implementation

2. **#[stream] attribute** declares which streams this command reads/writes

3. **apply() method** reconstructs state from events (pure: takes owned state, returns owned state)

4. **handle() method** contains your business logic and returns `Result<NewEvents<Self::Event>, CommandError>`

5. **execute() free function** runs the command: `execute(&store, command, RetryPolicy::new()).await?`

## Multi-Stream Command: Assign Task

Task assignment affects both the task and the user:

### `src/domain/commands/assign_task.rs`

```rust
use crate::domain::{events::*, types::*};
use chrono::Utc;
use eventcore::{Command, CommandError, CommandLogic, NewEvents, StreamId};

/// Command to assign a task to a user
/// This is a multi-stream command affecting both task and user streams
#[derive(Command)]
pub struct AssignTask {
    #[stream]
    pub task_id: StreamId,

    #[stream]
    pub assignee_id: StreamId,

    pub assigned_by: UserName,
}

/// State that combines task and user information
#[derive(Default)]
pub struct AssignTaskState {
    // Task state
    task_exists: bool,
    task_title: String,
    current_assignee: Option<UserName>,
    task_status: TaskStatus,

    // User state
    user_exists: bool,
    user_name: Option<UserName>,
    active_task_count: u32,
}

impl CommandLogic for AssignTask {
    type State = AssignTaskState;
    type Event = SystemEvent;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        // Apply events from different streams
        match event {
            SystemEvent::Task(task_event) => {
                match task_event {
                    TaskEvent::Created { title, .. } => {
                        state.task_exists = true;
                        state.task_title = title.to_string();
                    }
                    TaskEvent::Assigned { assignee, .. } => {
                        state.current_assignee = Some(assignee.clone());
                    }
                    TaskEvent::Unassigned { .. } => {
                        state.current_assignee = None;
                    }
                    TaskEvent::Completed { .. } => {
                        state.task_status = TaskStatus::Completed;
                    }
                    _ => {}
                }
            }
            SystemEvent::User(user_event) => {
                match user_event {
                    UserEvent::TaskAssigned { user_name, .. } => {
                        state.user_exists = true;
                        state.user_name = Some(user_name.clone());
                        state.active_task_count += 1;
                    }
                    UserEvent::TaskCompleted { .. } => {
                        state.active_task_count = state.active_task_count.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Validate business rules
        if !state.task_exists {
            return Err(CommandError::business_rule_violated("Cannot assign non-existent task"));
        }
        if state.task_status == TaskStatus::Completed {
            return Err(CommandError::business_rule_violated("Cannot assign completed task"));
        }
        if state.task_status == TaskStatus::Cancelled {
            return Err(CommandError::business_rule_violated("Cannot assign cancelled task"));
        }

        let now = Utc::now();
        let task_id = TaskId::from(&self.task_id);
        let assignee = UserName::from(&self.assignee_id);

        let mut events = Vec::new();

        // If task is currently assigned, unassign first
        if let Some(previous_assignee) = state.current_assignee {
            events.push(SystemEvent::Task(TaskEvent::Unassigned {
                task_id,
                previous_assignee,
                unassigned_by: self.assigned_by.clone(),
                unassigned_at: now,
            }));
        }

        // Write assignment event to task stream
        events.push(SystemEvent::Task(TaskEvent::Assigned {
            task_id,
            assignee: assignee.clone(),
            assigned_by: self.assigned_by.clone(),
            assigned_at: now,
        }));

        // Write assignment event to user stream
        events.push(SystemEvent::User(UserEvent::TaskAssigned {
            user_name: assignee,
            task_id,
            assigned_at: now,
        }));

        Ok(events.into())
    }
}
```

### Multi-Stream Benefits

1. **Atomic Updates**: Both task and user streams update together
2. **Consistent State**: No partial updates possible
3. **Rich Events**: Each stream gets relevant events
4. **Type Safety**: Executor enforces writes only to declared streams

## Command with Business Logic: Complete Task

### `src/domain/commands/complete_task.rs`

```rust
use crate::domain::{events::*, types::*};
use chrono::Utc;
use eventcore::{Command, CommandError, CommandLogic, NewEvents, StreamId};

/// Command to complete a task
#[derive(Command)]
pub struct CompleteTask {
    #[stream]
    pub task_id: StreamId,

    #[stream]
    pub user_id: StreamId,

    pub completed_by: UserName,
}

#[derive(Default)]
pub struct CompleteTaskState {
    task_exists: bool,
    task_status: TaskStatus,
    assignee: Option<UserName>,

    user_name: Option<UserName>,
    completed_count: u32,
}

impl CommandLogic for CompleteTask {
    type State = CompleteTaskState;
    type Event = SystemEvent;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
        match event {
            SystemEvent::Task(task_event) => {
                match task_event {
                    TaskEvent::Created { .. } => {
                        state.task_exists = true;
                        state.task_status = TaskStatus::Open;
                    }
                    TaskEvent::Assigned { assignee, .. } => {
                        state.assignee = Some(assignee.clone());
                    }
                    TaskEvent::Started { .. } => {
                        state.task_status = TaskStatus::InProgress;
                    }
                    TaskEvent::Completed { .. } => {
                        state.task_status = TaskStatus::Completed;
                    }
                    _ => {}
                }
            }
            SystemEvent::User(user_event) => {
                match user_event {
                    UserEvent::TaskCompleted { user_name, .. } => {
                        state.user_name = Some(user_name.clone());
                        state.completed_count += 1;
                    }
                    _ => {}
                }
            }
        }
        state
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Business rules
        if !state.task_exists {
            return Err(CommandError::business_rule_violated("Cannot complete non-existent task"));
        }
        if state.task_status == TaskStatus::Completed {
            return Err(CommandError::business_rule_violated("Task is already completed"));
        }
        if state.task_status == TaskStatus::Cancelled {
            return Err(CommandError::business_rule_violated("Cannot complete cancelled task"));
        }

        // Only assigned user can complete (or admin)
        if let Some(assignee) = &state.assignee {
            if assignee != &self.completed_by && self.completed_by.as_ref() != "admin" {
                return Err(CommandError::business_rule_violated(
                    "Only assigned user or admin can complete task"
                ));
            }
        }

        let now = Utc::now();
        let task_id = TaskId::from(&self.task_id);

        let mut events = Vec::new();

        // Mark task as completed
        events.push(SystemEvent::Task(TaskEvent::Completed {
            task_id,
            completed_by: self.completed_by.clone(),
            completed_at: now,
        }));

        // Update user's completion stats
        events.push(SystemEvent::User(UserEvent::TaskCompleted {
            user_name: self.completed_by.clone(),
            task_id,
            completed_at: now,
        }));

        Ok(events.into())
    }
}
```

## Helper Functions

Add these to `src/domain/types.rs`:

```rust
use eventcore::StreamId;

impl From<&StreamId> for TaskId {
    fn from(stream_id: &StreamId) -> Self {
        // Extract TaskId from stream ID like "task-{uuid}"
        let id_str = stream_id.as_ref()
            .strip_prefix("task-")
            .unwrap_or(stream_id.as_ref());

        TaskId(Uuid::parse_str(id_str).unwrap_or_else(|_| Uuid::nil()))
    }
}

impl From<&StreamId> for UserName {
    fn from(stream_id: &StreamId) -> Self {
        // Extract UserName from stream ID like "user-{name}"
        let name = stream_id.as_ref()
            .strip_prefix("user-")
            .unwrap_or(stream_id.as_ref());

        UserName::try_new(name).unwrap_or_else(|_|
            UserName::try_new("unknown").unwrap()
        )
    }
}
```

## Testing Our Commands

Add to `src/main.rs`:

```rust
#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::domain::commands::*;
    use crate::domain::types::*;
    use eventcore::{execute, RetryPolicy, run_projection};
    use eventcore_memory::InMemoryEventStore;
    use eventcore_testing::EventCollector;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_create_task() {
        // Given: An in-memory event store
        let store = InMemoryEventStore::new();

        // And: A create task command
        let command = CreateTask {
            task_id: StreamId::try_new("task-123").unwrap(),
            title: TaskTitle::try_new("Write tests").unwrap(),
            description: TaskDescription::try_new("Add unit tests").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };

        // When: The command is executed
        execute(&store, command, RetryPolicy::new()).await.unwrap();

        // Then: Events are written (verify via projection)
        let storage: Arc<Mutex<Vec<TaskEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let collector = EventCollector::new(storage.clone());
        run_projection(collector, &store).await.unwrap();

        let events = storage.lock().unwrap();
        assert_eq!(events.len(), 1);
    }
}
```

## Running the Demo

Update the demo in `src/main.rs`:

```rust
use eventcore::{execute, RetryPolicy, StreamId};

async fn run_demo(store: &InMemoryEventStore)
-> Result<(), Box<dyn std::error::Error>>
{
    println!("EventCore Task Management Demo");
    println!("==============================\n");

    // Create a task
    let task_stream = StreamId::try_new("task-demo-1")?;
    println!("1. Creating task...");

    let create = CreateTask {
        task_id: task_stream.clone(),
        title: TaskTitle::try_new("Build awesome features").unwrap(),
        description: TaskDescription::try_new("Use EventCore to build great things").unwrap(),
        creator: UserName::try_new("alice").unwrap(),
        priority: Priority::default(),
    };

    execute(store, create, RetryPolicy::new()).await?;
    println!("   Task created!\n");

    // Assign the task
    println!("2. Assigning task to Bob...");

    let assign = AssignTask {
        task_id: task_stream.clone(),
        assignee_id: StreamId::try_new("user-bob")?,
        assigned_by: UserName::try_new("alice").unwrap(),
    };

    execute(store, assign, RetryPolicy::new()).await?;
    println!("   Task assigned!\n");

    // Complete the task
    println!("3. Bob completes the task...");

    let complete = CompleteTask {
        task_id: task_stream,
        user_id: StreamId::try_new("user-bob")?,
        completed_by: UserName::try_new("bob").unwrap(),
    };

    execute(store, complete, RetryPolicy::new()).await?;
    println!("   Task completed!\n");

    println!("Demo complete!");
    Ok(())
}
```

## Key Takeaways

1. **Macro Magic**: `#[derive(Command)]` eliminates boilerplate
2. **Stream Declaration**: `#[stream]` attributes declare what you need
3. **Type Safety**: Can only write to declared streams (enforced by the executor)
4. **Multi-Stream**: Natural support for operations across entities
5. **Business Logic**: Clear separation in `handle()` method
6. **State Building**: `apply()` reconstructs state from events

## Common Patterns

### Conditional Stream Access

Sometimes you need streams based on runtime data. Dynamic discovery is handled by the executor as a separate phase; your `handle()` implementation returns the events the command needs to emit.

```rust
fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // Discover we need another stream based on state and return events accordingly.
    if state.requires_manager_approval {
        let approval_event = TaskEvent::ManagerApprovalRequested {
            task_id: TaskId::from(&self.task_id),
            requested_by: self.creator.clone(),
        };

        return Ok(NewEvents::from(vec![approval_event]));
    }

    Ok(NewEvents::from(vec![]))
}
```

### Batch Operations

For operations on multiple items:

```rust
let mut events = Vec::new();

for task_id in &self.task_ids {
    events.push(TaskEvent::BatchUpdated { /* ... */ });
}

Ok(NewEvents::from(events))
```

## Summary

We've implemented our core commands using EventCore's macro system:

- ✅ Single-stream commands (CreateTask)
- ✅ Multi-stream commands (AssignTask)
- ✅ Complex business logic (CompleteTask)
- ✅ Type-safe stream access
- ✅ Comprehensive testing

Next, let's build [projections](./04-projections.md) to query our data →
