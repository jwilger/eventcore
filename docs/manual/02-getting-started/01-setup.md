# Chapter 2.1: Setting Up Your Project

Let's create a new Rust project and add EventCore dependencies. We'll build a task management system that demonstrates EventCore's key features.

## Create a New Project

```bash
cargo new taskmaster --bin
cd taskmaster
```

## Add Dependencies

Edit `Cargo.toml` to include EventCore and related dependencies:

```toml
[package]
name = "taskmaster"
version = "0.1.0"
edition = "2024"

[dependencies]
# EventCore core functionality
eventcore = "0.1"

# For development/testing - switch to eventcore-postgres for production
eventcore-memory = "0.1"

# Async runtime
tokio = { version = "1.40", features = ["full"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Type validation
nutype = { version = "0.6", features = ["serde"] }

# Utilities
uuid = { version = "1.11", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2.0"

# For our CLI interface
clap = { version = "4.5", features = ["derive"] }

[dev-dependencies]
# Testing utilities
eventcore-testing = "0.1"
proptest = "1.6"
```

## Project Structure

Create the following directory structure:

```
taskmaster/
├── Cargo.toml
├── src/
│   ├── main.rs           # Application entry point
│   ├── domain/
│   │   ├── mod.rs        # Domain module
│   │   ├── types.rs      # Domain types with validation
│   │   ├── events.rs     # Event definitions
│   │   └── commands/     # Command implementations
│   │       ├── mod.rs
│   │       ├── create_task.rs
│   │       ├── assign_task.rs
│   │       └── complete_task.rs
│   ├── projections/
│   │   ├── mod.rs        # Projections module
│   │   ├── task_list.rs  # User task lists
│   │   └── statistics.rs # Task statistics
│   └── api/
│       ├── mod.rs        # API module (we'll add this in Part 4)
│       └── handlers.rs   # HTTP handlers
```

Create the directories:

```bash
mkdir -p src/domain/commands
mkdir -p src/projections
mkdir -p src/api
```

## Initial Setup Code

Let's create the basic module structure:

### `src/main.rs`

```rust
mod domain;
mod projections;

use clap::{Parser, Subcommand};
use eventcore::{execute, RetryPolicy};
use eventcore_memory::InMemoryEventStore;

#[derive(Parser)]
#[command(name = "taskmaster")]
#[command(about = "A task management system built with EventCore")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new task
    Create {
        /// Task title
        title: String,
        /// Task description
        description: String,
    },
    /// List all tasks
    List,
    /// Run interactive demo
    Demo,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize event store (in-memory for now)
    let store = InMemoryEventStore::new();

    let cli = Cli::parse();

    match cli.command {
        Commands::Create { title, description } => {
            println!("Creating task: {} - {}", title, description);
            // We'll implement this in Chapter 2.3
        }
        Commands::List => {
            println!("Listing tasks...");
            // We'll implement this in Chapter 2.4
        }
        Commands::Demo => {
            println!("Running demo...");
            run_demo(&store).await?;
        }
    }

    Ok(())
}

async fn run_demo(store: &InMemoryEventStore)
-> Result<(), Box<dyn std::error::Error>>
{
    println!("EventCore Task Management Demo");
    println!("==============================\n");

    // We'll add demo code as we build features

    Ok(())
}
```

### `src/domain/mod.rs`

```rust
pub mod types;
pub mod events;
pub mod commands;

// Re-export commonly used items
pub use types::*;
pub use events::*;
```

### `src/domain/types.rs`

```rust
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Validated task title - must be non-empty and reasonable length
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 200),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        AsRef,
        Serialize,
        Deserialize,
        Display
    )
)]
pub struct TaskTitle(String);

/// Validated task description
#[nutype(
    sanitize(trim),
    validate(len_char_max = 2000),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        AsRef,
        Serialize,
        Deserialize
    )
)]
pub struct TaskDescription(String);

/// Validated comment text
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 1000),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        AsRef,
        Serialize,
        Deserialize
    )
)]
pub struct CommentText(String);

/// Validated user name
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 100),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Serialize,
        Deserialize,
        Display
    )
)]
pub struct UserName(String);

/// Strongly-typed task ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Task priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Medium
    }
}

/// Task status - note we model this as events, not mutable state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Open,
    InProgress,
    Completed,
    Cancelled,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Open
    }
}
```

### `src/domain/events.rs`

```rust
use super::types::*;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Events that can occur in our task management system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TaskEvent {
    /// A new task was created
    Created {
        task_id: TaskId,
        title: TaskTitle,
        description: TaskDescription,
        creator: UserName,
        created_at: DateTime<Utc>,
    },

    /// Task was assigned to a user
    Assigned {
        task_id: TaskId,
        assignee: UserName,
        assigned_by: UserName,
        assigned_at: DateTime<Utc>,
    },

    /// Task was unassigned
    Unassigned {
        task_id: TaskId,
        unassigned_by: UserName,
        unassigned_at: DateTime<Utc>,
    },

    /// Task priority was changed
    PriorityChanged {
        task_id: TaskId,
        old_priority: Priority,
        new_priority: Priority,
        changed_by: UserName,
        changed_at: DateTime<Utc>,
    },

    /// Comment was added to task
    CommentAdded {
        task_id: TaskId,
        comment: CommentText,
        author: UserName,
        commented_at: DateTime<Utc>,
    },

    /// Task was completed
    Completed {
        task_id: TaskId,
        completed_by: UserName,
        completed_at: DateTime<Utc>,
    },

    /// Task was reopened after completion
    Reopened {
        task_id: TaskId,
        reopened_by: UserName,
        reopened_at: DateTime<Utc>,
        reason: Option<String>,
    },

    /// Task was cancelled
    Cancelled {
        task_id: TaskId,
        cancelled_by: UserName,
        cancelled_at: DateTime<Utc>,
        reason: Option<String>,
    },
}

// Implement the Event trait for stream routing
impl eventcore::Event for TaskEvent {
    fn stream_id(&self) -> &eventcore::StreamId {
        match self {
            TaskEvent::Created { task_id, .. }
            | TaskEvent::Assigned { task_id, .. }
            | TaskEvent::Unassigned { task_id, .. }
            | TaskEvent::PriorityChanged { task_id, .. }
            | TaskEvent::CommentAdded { task_id, .. }
            | TaskEvent::Completed { task_id, .. }
            | TaskEvent::Reopened { task_id, .. }
            | TaskEvent::Cancelled { task_id, .. } => {
                // task_id would need to be a StreamId or convertible to one
                todo!("implement stream_id routing")
            }
        }
    }

    fn event_type_name() -> &'static str {
        "TaskEvent"
    }
}
```

### `src/domain/commands/mod.rs`

```rust
mod create_task;
mod assign_task;
mod complete_task;

pub use create_task::*;
pub use assign_task::*;
pub use complete_task::*;
```

### `src/projections/mod.rs`

```rust
mod task_list;
mod statistics;

pub use task_list::*;
pub use statistics::*;
```

## Verify Setup

Let's make sure everything compiles:

```bash
cargo build
```

You should see output like:

```
   Compiling taskmaster v0.1.0
    Finished dev [unoptimized + debuginfo] target(s) in X.XXs
```

## Create a Simple Test

Add to `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::*;

    #[test]
    fn test_validated_types() {
        // Valid title
        let title = TaskTitle::try_new("Fix the bug").unwrap();
        assert_eq!(title.as_ref(), "Fix the bug");

        // Empty title should fail
        assert!(TaskTitle::try_new("").is_err());

        // Whitespace is trimmed
        let title = TaskTitle::try_new("  Trimmed  ").unwrap();
        assert_eq!(title.as_ref(), "Trimmed");
    }

    #[test]
    fn test_task_id_generation() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();

        // IDs should be unique
        assert_ne!(id1, id2);

        // IDs should be sortable by creation time (UUIDv7 property)
        assert!(id1.0 < id2.0);
    }
}
```

Run the tests:

```bash
cargo nextest run --workspace
# Fallback if nextest is not installed:
# cargo test --workspace
```

## Environment Setup for SQLite (Optional)

If you want persistence without running a database server, use the SQLite adapter:

```toml
[dependencies]
eventcore-sqlite = "0.1"
```

```rust
use eventcore_sqlite::SqliteEventStore;

// File-backed store - data persists across restarts
let store = SqliteEventStore::new("./taskmaster.db").await?;
```

No external services needed - the database is embedded in your application.

## Environment Setup for PostgreSQL (Optional)

If you want to use PostgreSQL instead of the in-memory store:

1. Start PostgreSQL with Docker:

```bash
docker run -d \
  --name eventcore-postgres \
  -e POSTGRES_PASSWORD=password \
  -e POSTGRES_DB=taskmaster \
  -p 5432:5432 \
  postgres:17
```

2. Update `Cargo.toml`:

```toml
[dependencies]
eventcore-postgres = "0.1"
```

3. Set environment variable:

```bash
export DATABASE_URL="postgres://postgres:password@localhost/taskmaster"
```

## Summary

We've set up:

- ✅ A new Rust project with EventCore dependencies
- ✅ Domain types with validation using `nutype`
- ✅ Event definitions for our task system
- ✅ Basic project structure
- ✅ Test infrastructure

Next, we'll [model our domain](./02-domain-modeling.md) using event modeling techniques →
