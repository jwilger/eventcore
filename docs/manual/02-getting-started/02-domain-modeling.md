# Chapter 2.2: Modeling the Domain

Now that our project is set up, let's use event modeling to design our task management system. We'll discover the events, commands, and read models that make up our domain.

## Step 1: Brainstorm the Events

What happens in a task management system? Let's think through a typical workflow:

```
Events (Orange - things that happened):
- Task Created
- Task Assigned
- Task Started
- Comment Added
- Task Completed
- Task Reopened
- Priority Changed
- Due Date Set
- Task Cancelled
```

## Step 2: Build the Timeline

Let's visualize how these events flow through time:

```
Timeline →
            Task Created ──┬── Task Assigned ──┬── Comment Added ──┬── Task Completed
                          │                    │                   │
User: Alice               │   User: Bob       │   User: Bob      │   User: Bob
Title: "Fix login bug"    │   Assignee: Bob   │   "Found issue"  │
                          │                    │                   │
Stream: task-123          │   Streams:        │   Stream:        │   Streams:
                          │   - task-123      │   - task-123     │   - task-123
                          │   - user-bob      │                  │   - user-bob
```

Notice how some operations involve multiple streams - this is where EventCore shines!

## Step 3: Identify Commands

For each event, what user action triggered it?

| Command (Blue)  | →   | Events (Orange)  | Streams Involved |
| --------------- | --- | ---------------- | ---------------- |
| Create Task     | →   | Task Created     | task             |
| Assign Task     | →   | Task Assigned    | task, assignee   |
| Start Task      | →   | Task Started     | task, user       |
| Add Comment     | →   | Comment Added    | task             |
| Complete Task   | →   | Task Completed   | task, user       |
| Reopen Task     | →   | Task Reopened    | task, user       |
| Change Priority | →   | Priority Changed | task             |
| Cancel Task     | →   | Task Cancelled   | task, user       |

## Step 4: Design Read Models

What questions do users need answered?

| Question                      | Read Model (Green) | Updated By Events              |
| ----------------------------- | ------------------ | ------------------------------ |
| "What are my tasks?"          | User Task List     | Assigned, Completed, Cancelled |
| "What's the task status?"     | Task Details       | All task events                |
| "What's the team workload?"   | Team Dashboard     | Created, Assigned, Completed   |
| "What happened to this task?" | Task History       | All events (audit log)         |

## Step 5: Discover Business Rules

As we model, we discover rules that our commands must enforce:

1. **Task Creation**
   - Title is required and non-empty
   - Description has reasonable length limit
   - Creator must be identified

2. **Task Assignment**
   - Can't assign to non-existent user
   - Should track assignment history
   - Unassigning is explicit action

3. **Task Completion**
   - Only assigned user can complete (or admin)
   - Can't complete cancelled tasks
   - Completion can be undone (reopen)

4. **Comments**
   - Must have content
   - Track author and timestamp
   - Comments are immutable

## Translating to EventCore

### Events Stay Close to Our Model

Our discovered events map directly to code using the `#[derive(Event)]` macro:

```rust
use eventcore::StreamId;
use eventcore_macros::Event;
use serde::{Serialize, Deserialize};

/// Task lifecycle events using the Event derive macro.
/// The #[stream] attribute marks which field identifies the aggregate.
#[derive(Event, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TaskEvent {
    Created {
        #[stream]
        task_id: StreamId,
        title: TaskTitle,
        description: TaskDescription,
        creator: UserName,
        created_at: DateTime<Utc>,
    },
    Assigned {
        #[stream]
        task_id: StreamId,
        assignee: UserName,
        assigned_by: UserName,
        assigned_at: DateTime<Utc>,
    },
    // ... other events with #[stream] on task_id
}
```

The `#[derive(Event)]` macro automatically implements the `Event` trait, generating:
- `stream_id()` - returns the field marked with `#[stream]`
- `event_type_name()` - returns the variant name (e.g., "Created", "Assigned")
- `all_type_names()` - returns all variant names for subscription filtering

### Commands Declare Their Streams

Multi-stream operations are explicit:

```rust
#[derive(Command, Clone)]
struct AssignTask {
    #[stream]
    task_id: StreamId,      // The task stream
    #[stream]
    user_id: StreamId,      // The assignee's stream
    assigned_by: UserName,
}
```

This command will:

1. Read both streams atomically
2. Validate the assignment
3. Write events to both streams
4. All in one transaction!

### State Models for Each Command

Each command needs different state views:

```rust
// State for task operations
#[derive(Default)]
struct TaskState {
    exists: bool,
    title: String,
    status: TaskStatus,
    assignee: Option<UserName>,
    creator: UserName,
}

// State for user operations
#[derive(Default)]
struct UserTasksState {
    user_name: UserName,
    assigned_tasks: Vec<TaskId>,
    completed_count: u32,
}
```

## Modeling Complex Scenarios

### Scenario: Task Handoff

When reassigning a task from Alice to Bob:

```
Timeline →
        Task Assigned       Task Unassigned      Task Assigned
        (to: Alice)         (from: Alice)        (to: Bob)
             │                    │                   │
             ├────────────────────┴───────────────────┤
             │                                        │
    Streams affected:                        Streams affected:
    - task-123                               - task-123
    - user-alice                             - user-alice
                                             - user-bob
```

In EventCore, we can model this as one atomic operation:

```rust
#[derive(Command, Clone)]
struct ReassignTask {
    #[stream]
    task_id: StreamId,
    #[stream]
    from_user: StreamId,
    #[stream]
    to_user: StreamId,
    reassigned_by: UserName,
}
```

### Scenario: Bulk Operations

Assigning multiple tasks to a user:

```rust
#[derive(Command, Clone)]
struct BulkAssignTasks {
    #[stream]
    user_id: StreamId,
    #[stream("tasks")]  // Multiple task streams
    task_ids: Vec<StreamId>,
    assigned_by: UserName,
}
```

The beauty of EventCore: this remains atomic across ALL streams!

## Visual Domain Model

Here's our complete domain model:

```
┌─────────────────────────────────────────────────────────────┐
│                        COMMANDS                              │
├─────────────────────────────────────────────────────────────┤
│ CreateTask │ AssignTask │ CompleteTask │ AddComment │ ...   │
└─────────────┬───────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────┐
│                         EVENTS                               │
├─────────────────────────────────────────────────────────────┤
│ TaskCreated │ TaskAssigned │ TaskCompleted │ CommentAdded   │
└─────────────┬───────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────┐
│                     READ MODELS                              │
├─────────────────────────────────────────────────────────────┤
│ UserTaskList │ TaskDetails │ TeamDashboard │ ActivityFeed   │
└─────────────────────────────────────────────────────────────┘
```

## Key Insights from Modeling

1. **Multi-Stream Operations are Common**
   - Task assignment affects task AND user streams
   - Completion updates task AND user statistics
   - EventCore handles this naturally

2. **Events are Business Facts**
   - "TaskAssigned" not "UpdateTask"
   - Events capture intent and context
   - Rich events enable better projections

3. **Commands Match User Intent**
   - "AssignTask" not "UpdateTaskAssignee"
   - Commands are what users want to do
   - Natural API emerges from modeling

4. **Read Models Serve Specific Needs**
   - UserTaskList for "my tasks" view
   - TeamDashboard for manager overview
   - Different projections from same events

## Refining Our Event Model

Based on our modeling, let's update `src/domain/events.rs` using the `#[derive(Event)]` macro:

```rust
use super::types::*;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use eventcore::StreamId;
use eventcore_macros::Event;

/// Events that can occur in our task management system.
/// Using #[derive(Event)] with #[stream] on each variant's stream field.
#[derive(Event, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    // Task lifecycle events
    Created {
        #[stream]
        task_id: StreamId,
        title: TaskTitle,
        description: TaskDescription,
        creator: UserName,
        created_at: DateTime<Utc>,
    },

    // Assignment events - note these affect multiple streams
    Assigned {
        #[stream]
        task_id: StreamId,
        assignee: UserName,
        assigned_by: UserName,
        assigned_at: DateTime<Utc>,
    },

    Unassigned {
        #[stream]
        task_id: StreamId,
        previous_assignee: UserName,
        unassigned_by: UserName,
        unassigned_at: DateTime<Utc>,
    },

    // Work events
    Started {
        #[stream]
        task_id: StreamId,
        started_by: UserName,
        started_at: DateTime<Utc>,
    },

    Completed {
        #[stream]
        task_id: StreamId,
        completed_by: UserName,
        completed_at: DateTime<Utc>,
    },

    // Collaboration events
    CommentAdded {
        #[stream]
        task_id: StreamId,
        comment_id: Uuid,
        comment: CommentText,
        author: UserName,
        commented_at: DateTime<Utc>,
    },

    // Management events
    PriorityChanged {
        #[stream]
        task_id: StreamId,
        old_priority: Priority,
        new_priority: Priority,
        changed_by: UserName,
        changed_at: DateTime<Utc>,
    },

    DueDateSet {
        #[stream]
        task_id: StreamId,
        due_date: DateTime<Utc>,
        set_by: UserName,
        set_at: DateTime<Utc>,
    },
}

/// Events specific to user streams.
/// Each variant has a #[stream] field identifying the user aggregate.
#[derive(Event, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserEvent {
    /// Track when user is assigned a task
    TaskAssigned {
        #[stream]
        user_id: StreamId,
        task_id: TaskId,
        assigned_at: DateTime<Utc>,
    },

    /// Track when user completes a task
    TaskCompleted {
        #[stream]
        user_id: StreamId,
        task_id: TaskId,
        completed_at: DateTime<Utc>,
    },

    /// Track workload changes
    WorkloadUpdated {
        #[stream]
        user_id: StreamId,
        active_tasks: u32,
        completed_today: u32,
    },
}
```

> **Note:** The `#[derive(Event)]` macro eliminates the need for manual trait implementations.
> Each event type automatically gets `stream_id()`, `event_type_name()`, and `all_type_names()`
> methods generated from the struct/enum definition.

## Summary

Through event modeling, we've discovered:

1. **Our Events**: Business facts that capture what happened
2. **Our Commands**: User intentions that trigger events
3. **Our Read Models**: Views that answer user questions
4. **Our Streams**: How data is organized (tasks, users)

The key insight: by modeling events first, the rest of the system design follows naturally. EventCore's multi-stream capabilities mean we can implement our model exactly as designed, without compromise.

Next, let's [implement our commands](./03-commands.md) using EventCore's macro system →
