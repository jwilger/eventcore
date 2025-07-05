# Your First EventCore Application

Let's build a complete event-sourced application from scratch: a simple blog engine that demonstrates EventCore's key concepts.

## Project Setup

1. **Create a new Rust project**

```bash
cargo new blog-engine
cd blog-engine
```

2. **Update Cargo.toml**

```toml
[package]
name = "blog-engine"
version = "0.1.0"
edition = "2021"

[dependencies]
eventcore = "0.1"
eventcore-memory = "0.1"
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
nutype = { version = "0.4", features = ["serde"] }
```

## Step 1: Define Domain Types

Create `src/types.rs`:

```rust
use eventcore::prelude::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Use nutype for domain validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 100),
    derive(Debug, Clone, PartialEq, Serialize, Deserialize, AsRef)
)]
pub struct PostId(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 200),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct PostTitle(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 10000),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct PostContent(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 100),
    derive(Debug, Clone, PartialEq, Serialize, Deserialize)
)]
pub struct AuthorId(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: AuthorId,
    pub content: String,
    pub created_at: DateTime<Utc>,
}
```

## Step 2: Define Events

Create `src/events.rs`:

```rust
use crate::types::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlogEvent {
    PostPublished {
        title: PostTitle,
        content: PostContent,
        author: AuthorId,
        published_at: DateTime<Utc>,
    },
    PostUpdated {
        title: PostTitle,
        content: PostContent,
        updated_at: DateTime<Utc>,
    },
    PostDeleted {
        deleted_at: DateTime<Utc>,
    },
    CommentAdded {
        comment: Comment,
    },
    CommentRemoved {
        comment_id: String,
    },
}
```

## Step 3: Define State

Create `src/state.rs`:

```rust
use crate::types::*;
use crate::events::BlogEvent;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct PostState {
    pub exists: bool,
    pub title: Option<PostTitle>,
    pub content: Option<PostContent>,
    pub author: Option<AuthorId>,
    pub published_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub comments: HashMap<String, Comment>,
}

impl PostState {
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn apply_event(&mut self, event: &BlogEvent) {
        match event {
            BlogEvent::PostPublished {
                title,
                content,
                author,
                published_at,
            } => {
                self.exists = true;
                self.title = Some(title.clone());
                self.content = Some(content.clone());
                self.author = Some(author.clone());
                self.published_at = Some(*published_at);
            }
            BlogEvent::PostUpdated {
                title,
                content,
                updated_at,
            } => {
                self.title = Some(title.clone());
                self.content = Some(content.clone());
                self.updated_at = Some(*updated_at);
            }
            BlogEvent::PostDeleted { deleted_at } => {
                self.deleted_at = Some(*deleted_at);
            }
            BlogEvent::CommentAdded { comment } => {
                self.comments.insert(comment.id.clone(), comment.clone());
            }
            BlogEvent::CommentRemoved { comment_id } => {
                self.comments.remove(comment_id);
            }
        }
    }
}
```

## Step 4: Implement Commands

Create `src/commands.rs`:

```rust
use crate::events::BlogEvent;
use crate::state::PostState;
use crate::types::*;
use chrono::Utc;
use eventcore::prelude::*;

// Publish a new blog post
#[derive(Clone, Command)]
#[command(event = "BlogEvent")]
pub struct PublishPost {
    pub post_id: PostId,
    pub title: PostTitle,
    pub content: PostContent,
    pub author: AuthorId,
}

impl PublishPost {
    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::from(format!("post-{}", self.post_id.as_ref()))]
    }
}

#[async_trait]
impl CommandLogic for PublishPost {
    type State = PostState;
    type Event = BlogEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.apply_event(&event.event);
    }

    async fn handle(
        &self,
        _: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate business rules
        require!(!state.exists, "Post already exists");

        // Emit event
        Ok(vec![emit!(
            StreamId::from(format!("post-{}", self.post_id.as_ref())),
            BlogEvent::PostPublished {
                title: self.title.clone(),
                content: self.content.clone(),
                author: self.author.clone(),
                published_at: Utc::now(),
            }
        )])
    }
}

// Add a comment to a post
#[derive(Clone, Command)]
#[command(event = "BlogEvent")]
pub struct AddComment {
    pub post_id: PostId,
    pub comment_id: String,
    pub author: AuthorId,
    pub content: String,
}

impl AddComment {
    fn read_streams(&self) -> Vec<StreamId> {
        vec![StreamId::from(format!("post-{}", self.post_id.as_ref()))]
    }
}

#[async_trait]
impl CommandLogic for AddComment {
    type State = PostState;
    type Event = BlogEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.apply_event(&event.event);
    }

    async fn handle(
        &self,
        _: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate
        require!(state.exists, "Post does not exist");
        require!(!state.is_deleted(), "Cannot comment on deleted post");
        require!(!state.comments.contains_key(&self.comment_id), 
                "Comment ID already exists");

        // Emit event
        Ok(vec![emit!(
            StreamId::from(format!("post-{}", self.post_id.as_ref())),
            BlogEvent::CommentAdded {
                comment: Comment {
                    id: self.comment_id.clone(),
                    author: self.author.clone(),
                    content: self.content.clone(),
                    created_at: Utc::now(),
                }
            }
        )])
    }
}
```

## Step 5: Create the Application

Update `src/main.rs`:

```rust
mod commands;
mod events;
mod state;
mod types;

use commands::*;
use eventcore::prelude::*;
use eventcore_memory::InMemoryEventStore;
use types::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the event store
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    // Create author and post IDs
    let author = AuthorId::try_new("alice".to_string())?;
    let post_id = PostId::try_new("hello-eventcore".to_string())?;

    // Publish a blog post
    let publish_cmd = PublishPost {
        post_id: post_id.clone(),
        title: PostTitle::try_new("Hello EventCore!".to_string())?,
        content: PostContent::try_new(
            "This is my first event-sourced blog post!".to_string()
        )?,
        author: author.clone(),
    };

    let result = executor.execute(publish_cmd).await?;
    println!("Post published with {} event(s)", result.events.len());

    // Add a comment
    let comment_cmd = AddComment {
        post_id: post_id.clone(),
        comment_id: "comment-1".to_string(),
        author: AuthorId::try_new("bob".to_string())?,
        content: "Great post!".to_string(),
    };

    let result = executor.execute(comment_cmd).await?;
    println!("Comment added!");

    // Try to add duplicate comment (will fail)
    let duplicate_comment = AddComment {
        post_id,
        comment_id: "comment-1".to_string(), // Same ID!
        author: AuthorId::try_new("charlie".to_string())?,
        content: "Another comment".to_string(),
    };

    match executor.execute(duplicate_comment).await {
        Ok(_) => println!("This shouldn't happen!"),
        Err(e) => println!("Expected error: {}", e),
    }

    Ok(())
}
```

## Step 6: Run Your Application

```bash
cargo run
```

You should see:
```
Post published with 1 event(s)
Comment added!
Expected error: Comment ID already exists
```

## What You've Learned

In this tutorial, you've implemented:

1. **Type-Safe Domain Modeling** - Using `nutype` for validation
2. **Event Sourcing Basics** - Events as the source of truth
3. **Command Pattern** - Encapsulating business operations
4. **Business Rule Validation** - Enforcing invariants
5. **State Reconstruction** - Building state from events

## Next Steps

Enhance your blog engine with:

- **Projections** for querying posts by author or tag
- **Multi-stream operations** for author profiles
- **Web API** using Axum or Actix
- **PostgreSQL backend** for persistence
- **Subscriptions** for real-time updates

Continue learning:
- [Building Web APIs](./manual/04-building-web-apis/01-setting-up-endpoints.html)
- [Advanced Topics](./manual/05-advanced-topics/01-schema-evolution.html)
- [Production Guide](./manual/06-operations/01-deployment-strategies.html)