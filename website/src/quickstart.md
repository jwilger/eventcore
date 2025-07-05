# Quick Start Guide

Get up and running with EventCore in 15 minutes!

## Installation

Add EventCore to your `Cargo.toml`:

```toml
[dependencies]
eventcore = "0.1"
eventcore-postgres = "0.1"  # For PostgreSQL backend
# OR
eventcore-memory = "0.1"   # For in-memory backend

# Required dependencies
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
async-trait = "0.1"
```

## Your First Event-Sourced Application

Let's build a simple task management system to demonstrate EventCore's key concepts.

### 1. Define Your Domain Types

```rust
use eventcore::prelude::*;
use serde::{Deserialize, Serialize};

// Domain types with compile-time validation
#[nutype(sanitize(trim), validate(not_empty, len_char_max = 50))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, AsRef)]
pub struct TaskId(String);

#[nutype(sanitize(trim), validate(not_empty, len_char_max = 200))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTitle(String);

// Events that represent state changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskEvent {
    Created { title: TaskTitle },
    Completed,
    Reopened,
}
```

### 2. Create Your First Command

```rust
#[derive(Clone, Command)]
#[command(event = "TaskEvent")]
pub struct CreateTask {
    pub task_id: TaskId,
    pub title: TaskTitle,
}

impl CreateTask {
    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::from(self.task_id.as_ref())]
    }
}

#[async_trait]
impl CommandLogic for CreateTask {
    type State = Option<TaskState>;
    type Event = TaskEvent;

    async fn handle(
        &self,
        _: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Ensure task doesn't already exist
        require!(state.is_none(), "Task already exists");

        // Emit the event
        Ok(vec![emit!(
            StreamId::from(self.task_id.as_ref()),
            TaskEvent::Created {
                title: self.title.clone()
            }
        )])
    }
}
```

### 3. Define State and Event Application

```rust
#[derive(Default, Debug, Clone)]
pub struct TaskState {
    pub title: TaskTitle,
    pub completed: bool,
}

impl CreateTask {
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<TaskEvent>) {
        if let Some(task_state) = state {
            match &event.event {
                TaskEvent::Created { title } => {
                    // This shouldn't happen with proper command validation
                    *task_state = TaskState {
                        title: title.clone(),
                        completed: false,
                    };
                }
                TaskEvent::Completed => {
                    task_state.completed = true;
                }
                TaskEvent::Reopened => {
                    task_state.completed = false;
                }
            }
        } else if let TaskEvent::Created { title } = &event.event {
            *state = Some(TaskState {
                title: title.clone(),
                completed: false,
            });
        }
    }
}
```

### 4. Set Up the Event Store and Execute Commands

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize event store (using in-memory for this example)
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    // Create a new task
    let task_id = TaskId::try_new("task-001".to_string())?;
    let title = TaskTitle::try_new("Learn EventCore".to_string())?;
    
    let create_cmd = CreateTask {
        task_id: task_id.clone(),
        title,
    };

    // Execute the command
    let result = executor.execute(create_cmd).await?;
    println!("Task created with {} event(s)", result.events.len());

    // Create a complete task command
    let complete_cmd = CompleteTask {
        task_id: task_id.clone(),
    };

    let result = executor.execute(complete_cmd).await?;
    println!("Task completed!");

    Ok(())
}
```

### 5. Add a Projection for Queries

```rust
#[derive(Debug, Clone)]
pub struct TaskListProjection {
    tasks: Arc<RwLock<HashMap<TaskId, TaskSummary>>>,
}

#[derive(Debug, Clone)]
struct TaskSummary {
    title: TaskTitle,
    completed: bool,
}

#[async_trait]
impl Projection for TaskListProjection {
    type Event = TaskEvent;

    async fn handle_event(
        &mut self,
        event: StoredEvent<Self::Event>,
        stream_id: &StreamId,
    ) -> Result<(), ProjectionError> {
        let task_id = TaskId::try_new(stream_id.as_ref().to_string())
            .map_err(|e| ProjectionError::InvalidData(e.to_string()))?;

        let mut tasks = self.tasks.write().await;
        
        match event.event {
            TaskEvent::Created { title } => {
                tasks.insert(task_id, TaskSummary {
                    title,
                    completed: false,
                });
            }
            TaskEvent::Completed => {
                if let Some(task) = tasks.get_mut(&task_id) {
                    task.completed = true;
                }
            }
            TaskEvent::Reopened => {
                if let Some(task) = tasks.get_mut(&task_id) {
                    task.completed = false;
                }
            }
        }

        Ok(())
    }
}
```

## Next Steps

Congratulations! You've built your first event-sourced application with EventCore. Here's what to explore next:

1. **[Domain Modeling Guide](./manual/02-getting-started/02-domain-modeling.html)** - Learn best practices for modeling your domain with types
2. **[Commands Deep Dive](./manual/03-core-concepts/01-commands-and-macros.html)** - Understand multi-stream operations and dynamic consistency
3. **[Building Web APIs](./manual/04-building-web-apis/01-setting-up-endpoints.html)** - Integrate EventCore with Axum or Actix
4. **[Testing Strategies](./manual/02-getting-started/05-testing.html)** - Property-based testing and chaos testing

## Example Projects

Check out these complete examples in the repository:

- **Banking System** - Multi-account transfers with ACID guarantees
- **E-Commerce Platform** - Order processing with inventory management
- **Saga Orchestration** - Long-running business processes

## Getting Help

- 📖 [Full Documentation](./manual/01-introduction/01-what-is-eventcore.html)
- 💬 [Discord Community](https://discord.gg/eventcore)
- 🐛 [Report Issues](https://github.com/eventcore-rs/eventcore/issues)
- 📚 [API Reference](./api/eventcore/index.html)