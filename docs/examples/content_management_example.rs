//! # Content Management System Example
//!
//! This example demonstrates EventCore usage in a content management system scenario.
//! It shows advanced patterns including:
//! 
//! - Content lifecycle management (draft -> published -> archived)
//! - Multi-user collaboration with permissions
//! - Cross-entity operations (content + user + permissions)
//! - Time-based operations (scheduled publishing)
//! - Audit trail for compliance requirements
//!
//! # Domain Model
//!
//! - **Content**: Articles that can be drafted, published, and archived
//! - **Users**: Authors and editors with different permissions
//! - **Comments**: User feedback on published content
//! - **Permissions**: Role-based access control
//!
//! # Key EventCore Patterns Demonstrated
//!
//! 1. **State machines**: Content status transitions
//! 2. **Multi-stream commands**: Publishing affects content + user + audit streams
//! 3. **Business rule validation**: Only editors can publish content
//! 4. **Event-driven projections**: Building read models for queries
//! 5. **Type-safe operations**: Using branded types and validation

use eventcore::prelude::*;
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Domain Types
// ============================================================================

/// Content management domain types with validation
pub mod types {
    use super::*;
    use nutype::nutype;

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 100),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct ContentId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct UserId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 200),
        derive(Debug, Clone, PartialEq, Eq, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct ContentTitle(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 10000),
        derive(Debug, Clone, PartialEq, Eq, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct ContentBody(String);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum ContentStatus {
        Draft,
        UnderReview,
        Published,
        Archived,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum UserRole {
        Author,
        Editor,
        Admin,
    }

    impl ContentId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("content-{}", self.as_ref())).unwrap()
        }
    }

    impl UserId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("user-{}", self.as_ref())).unwrap()
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// All events in the content management system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentEvent {
    // Content lifecycle events
    ContentCreated {
        content_id: types::ContentId,
        title: types::ContentTitle,
        author_id: types::UserId,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    ContentDrafted {
        content_id: types::ContentId,
        body: types::ContentBody,
        updated_at: chrono::DateTime<chrono::Utc>,
    },
    ContentSubmittedForReview {
        content_id: types::ContentId,
        submitted_by: types::UserId,
        submitted_at: chrono::DateTime<chrono::Utc>,
    },
    ContentPublished {
        content_id: types::ContentId,
        published_by: types::UserId,
        published_at: chrono::DateTime<chrono::Utc>,
    },
    ContentArchived {
        content_id: types::ContentId,
        archived_by: types::UserId,
        archived_at: chrono::DateTime<chrono::Utc>,
        reason: String,
    },
    
    // User management events
    UserCreated {
        user_id: types::UserId,
        email: String,
        role: types::UserRole,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    UserRoleChanged {
        user_id: types::UserId,
        old_role: types::UserRole,
        new_role: types::UserRole,
        changed_by: types::UserId,
        changed_at: chrono::DateTime<chrono::Utc>,
    },
    
    // Audit events
    ActionAudited {
        user_id: types::UserId,
        action: String,
        resource_type: String,
        resource_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
        metadata: HashMap<String, String>,
    },
}

// Required trait implementation for EventCore
impl TryFrom<&ContentEvent> for ContentEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &ContentEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// ============================================================================
// Command State Types
// ============================================================================

/// State for content operations
#[derive(Debug, Default, Clone)]
pub struct ContentState {
    pub exists: bool,
    pub title: Option<types::ContentTitle>,
    pub body: Option<types::ContentBody>,
    pub author_id: Option<types::UserId>,
    pub status: Option<types::ContentStatus>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// State for user operations
#[derive(Debug, Default, Clone)]
pub struct UserState {
    pub exists: bool,
    pub email: Option<String>,
    pub role: Option<types::UserRole>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Combined state for multi-entity operations
#[derive(Debug, Default, Clone)]
pub struct PublishContentState {
    pub content: ContentState,
    pub editor: UserState,
    pub audit_enabled: bool,
}

// ============================================================================
// Commands
// ============================================================================

/// Create new content as an author
pub struct CreateContentCommand {
    pub content_id: types::ContentId,
    pub title: types::ContentTitle,
    pub author_id: types::UserId,
}

#[async_trait::async_trait]
impl Command for CreateContentCommand {
    type Input = Self;
    type State = ContentState;
    type Event = ContentEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.content_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ContentEvent::ContentCreated {
                content_id,
                title,
                author_id,
                created_at,
            } => {
                state.exists = true;
                state.title = Some(title.clone());
                state.author_id = Some(author_id.clone());
                state.status = Some(types::ContentStatus::Draft);
                state.created_at = Some(*created_at);
                state.updated_at = Some(*created_at);
            }
            _ => {} // Other events don't affect this command's view
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Business rule: Content cannot already exist
        if state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' already exists",
                input.content_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            input.content_id.stream_id(),
            ContentEvent::ContentCreated {
                content_id: input.content_id,
                title: input.title,
                author_id: input.author_id,
                created_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Update content body (only in draft status)
pub struct UpdateContentCommand {
    pub content_id: types::ContentId,
    pub new_body: types::ContentBody,
    pub updated_by: types::UserId,
}

#[async_trait::async_trait]
impl Command for UpdateContentCommand {
    type Input = Self;
    type State = ContentState;
    type Event = ContentEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.content_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ContentEvent::ContentCreated {
                content_id,
                title,
                author_id,
                created_at,
            } => {
                state.exists = true;
                state.title = Some(title.clone());
                state.author_id = Some(author_id.clone());
                state.status = Some(types::ContentStatus::Draft);
                state.created_at = Some(*created_at);
                state.updated_at = Some(*created_at);
            }
            ContentEvent::ContentDrafted {
                body, updated_at, ..
            } => {
                state.body = Some(body.clone());
                state.updated_at = Some(*updated_at);
            }
            ContentEvent::ContentSubmittedForReview { .. } => {
                state.status = Some(types::ContentStatus::UnderReview);
            }
            ContentEvent::ContentPublished { .. } => {
                state.status = Some(types::ContentStatus::Published);
            }
            ContentEvent::ContentArchived { .. } => {
                state.status = Some(types::ContentStatus::Archived);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Business rules validation
        if !state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' does not exist",
                input.content_id.as_ref()
            )));
        }

        let current_status = state.status.as_ref().ok_or_else(|| {
            CommandError::Internal("Content exists but has no status".to_string())
        })?;

        if !matches!(current_status, types::ContentStatus::Draft) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' cannot be updated in status '{:?}' (must be Draft)",
                input.content_id.as_ref(),
                current_status
            )));
        }

        // Only the author can update their content
        let author_id = state.author_id.as_ref().ok_or_else(|| {
            CommandError::Internal("Content exists but has no author".to_string())
        })?;

        if author_id != &input.updated_by {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Only the author '{}' can update content '{}'",
                author_id.as_ref(),
                input.content_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            input.content_id.stream_id(),
            ContentEvent::ContentDrafted {
                content_id: input.content_id,
                body: input.new_body,
                updated_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Publish content (multi-stream operation: content + user + audit)
pub struct PublishContentCommand {
    pub content_id: types::ContentId,
    pub published_by: types::UserId,
}

#[async_trait::async_trait]
impl Command for PublishContentCommand {
    type Input = Self;
    type State = PublishContentState;
    type Event = ContentEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.content_id.stream_id(),
            input.published_by.stream_id(),
            StreamId::try_new("audit-log".to_string()).unwrap(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Apply events to content state
        match &event.payload {
            ContentEvent::ContentCreated {
                content_id,
                title,
                author_id,
                created_at,
            } => {
                if event.stream_id.as_ref().starts_with("content-") {
                    state.content.exists = true;
                    state.content.title = Some(title.clone());
                    state.content.author_id = Some(author_id.clone());
                    state.content.status = Some(types::ContentStatus::Draft);
                    state.content.created_at = Some(*created_at);
                }
            }
            ContentEvent::ContentDrafted {
                body, updated_at, ..
            } => {
                if event.stream_id.as_ref().starts_with("content-") {
                    state.content.body = Some(body.clone());
                    state.content.updated_at = Some(*updated_at);
                }
            }
            ContentEvent::ContentSubmittedForReview { .. } => {
                if event.stream_id.as_ref().starts_with("content-") {
                    state.content.status = Some(types::ContentStatus::UnderReview);
                }
            }
            ContentEvent::ContentPublished { .. } => {
                if event.stream_id.as_ref().starts_with("content-") {
                    state.content.status = Some(types::ContentStatus::Published);
                }
            }
            
            // Apply events to user state
            ContentEvent::UserCreated {
                email, role, created_at, ..
            } => {
                if event.stream_id.as_ref().starts_with("user-") {
                    state.editor.exists = true;
                    state.editor.email = Some(email.clone());
                    state.editor.role = Some(role.clone());
                    state.editor.created_at = Some(*created_at);
                }
            }
            ContentEvent::UserRoleChanged { new_role, .. } => {
                if event.stream_id.as_ref().starts_with("user-") {
                    state.editor.role = Some(new_role.clone());
                }
            }
            
            // Track audit configuration
            ContentEvent::ActionAudited { .. } => {
                if event.stream_id.as_ref() == "audit-log" {
                    state.audit_enabled = true;
                }
            }
            
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate content exists and is ready for publishing
        if !state.content.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' does not exist",
                input.content_id.as_ref()
            )));
        }

        let current_status = state.content.status.as_ref().ok_or_else(|| {
            CommandError::Internal("Content exists but has no status".to_string())
        })?;

        if !matches!(
            current_status,
            types::ContentStatus::Draft | types::ContentStatus::UnderReview
        ) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' cannot be published from status '{:?}'",
                input.content_id.as_ref(),
                current_status
            )));
        }

        // Validate user has permission to publish
        if !state.editor.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' does not exist",
                input.published_by.as_ref()
            )));
        }

        let editor_role = state.editor.role.as_ref().ok_or_else(|| {
            CommandError::Internal("User exists but has no role".to_string())
        })?;

        if !matches!(editor_role, types::UserRole::Editor | types::UserRole::Admin) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' with role '{:?}' cannot publish content (requires Editor or Admin)",
                input.published_by.as_ref(),
                editor_role
            )));
        }

        // Content must have body to be published
        if state.content.body.is_none() {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Content '{}' cannot be published without body text",
                input.content_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let mut events = Vec::new();

        // Event 1: Mark content as published
        events.push(StreamWrite::new(
            &read_streams,
            input.content_id.stream_id(),
            ContentEvent::ContentPublished {
                content_id: input.content_id.clone(),
                published_by: input.published_by.clone(),
                published_at: now,
            },
        )?);

        // Event 2: Create audit log entry
        let mut metadata = HashMap::new();
        metadata.insert("content_id".to_string(), input.content_id.as_ref().to_string());
        metadata.insert("content_title".to_string(), 
            state.content.title.as_ref().map(|t| t.as_ref()).unwrap_or("Unknown").to_string());
        metadata.insert("editor_role".to_string(), format!("{:?}", editor_role));

        events.push(StreamWrite::new(
            &read_streams,
            StreamId::try_new("audit-log".to_string()).unwrap(),
            ContentEvent::ActionAudited {
                user_id: input.published_by,
                action: "publish_content".to_string(),
                resource_type: "content".to_string(),
                resource_id: input.content_id.as_ref().to_string(),
                timestamp: now,
                metadata,
            },
        )?);

        Ok(events)
    }
}

// ============================================================================
// Helper functions for example execution
// ============================================================================

/// Create a user for testing
async fn create_user(
    executor: &CommandExecutor<InMemoryEventStore<ContentEvent>>,
    user_id: &types::UserId,
    email: &str,
    role: types::UserRole,
) -> Result<(), CommandError> {
    let command = CreateUserCommand {
        user_id: user_id.clone(),
        email: email.to_string(),
        role,
    };
    
    executor.execute(&command, command, ExecutionOptions::default()).await?;
    Ok(())
}

/// Simple command to create users
pub struct CreateUserCommand {
    pub user_id: types::UserId,
    pub email: String,
    pub role: types::UserRole,
}

#[async_trait::async_trait]
impl Command for CreateUserCommand {
    type Input = Self;
    type State = UserState;
    type Event = ContentEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.user_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let ContentEvent::UserCreated { email, role, created_at, .. } = &event.payload {
            state.exists = true;
            state.email = Some(email.clone());
            state.role = Some(role.clone());
            state.created_at = Some(*created_at);
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' already exists",
                input.user_id.as_ref()
            )));
        }

        let event = StreamWrite::new(
            &read_streams,
            input.user_id.stream_id(),
            ContentEvent::UserCreated {
                user_id: input.user_id,
                email: input.email,
                role: input.role,
                created_at: chrono::Utc::now(),
            },
        )?;

        Ok(vec![event])
    }
}

// ============================================================================
// Example Execution
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize EventCore with in-memory store
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    println!("🚀 EventCore Content Management System Example");
    println!("===============================================\n");

    // Step 1: Create users with different roles
    println!("📝 Creating users...");
    
    let author_id = types::UserId::try_new("alice-writer".to_string()).unwrap();
    let editor_id = types::UserId::try_new("bob-editor".to_string()).unwrap();
    
    create_user(&executor, &author_id, "alice@example.com", types::UserRole::Author).await?;
    create_user(&executor, &editor_id, "bob@example.com", types::UserRole::Editor).await?;
    
    println!("✅ Created author: {}", author_id.as_ref());
    println!("✅ Created editor: {}", editor_id.as_ref());

    // Step 2: Author creates content
    println!("\n📄 Creating content...");
    
    let content_id = types::ContentId::try_new("my-first-article".to_string()).unwrap();
    let title = types::ContentTitle::try_new("Introduction to EventCore".to_string()).unwrap();

    let create_command = CreateContentCommand {
        content_id: content_id.clone(),
        title: title.clone(),
        author_id: author_id.clone(),
    };

    executor.execute(&create_command, create_command, ExecutionOptions::default()).await?;
    println!("✅ Created content: {} by {}", content_id.as_ref(), author_id.as_ref());

    // Step 3: Author updates content body
    println!("\n✏️  Updating content...");
    
    let body = types::ContentBody::try_new(
        "EventCore is a powerful multi-stream event sourcing library that enables atomic operations across multiple entities...".to_string()
    ).unwrap();

    let update_command = UpdateContentCommand {
        content_id: content_id.clone(),
        new_body: body,
        updated_by: author_id.clone(),
    };

    executor.execute(&update_command, update_command, ExecutionOptions::default()).await?;
    println!("✅ Updated content body");

    // Step 4: Try to publish as author (should fail - only editors can publish)
    println!("\n❌ Attempting to publish as author (should fail)...");
    
    let publish_attempt = PublishContentCommand {
        content_id: content_id.clone(),
        published_by: author_id.clone(),
    };

    match executor.execute(&publish_attempt, publish_attempt, ExecutionOptions::default()).await {
        Ok(_) => println!("❌ ERROR: Author should not be able to publish!"),
        Err(err) => println!("✅ Correctly blocked: {}", err),
    }

    // Step 5: Editor publishes content (multi-stream operation)
    println!("\n🚀 Publishing content as editor...");
    
    let publish_command = PublishContentCommand {
        content_id: content_id.clone(),
        published_by: editor_id.clone(),
    };

    let result = executor.execute(&publish_command, publish_command, ExecutionOptions::default()).await?;
    println!("✅ Content published successfully!");
    println!("   📊 Events written: {}", result.events_written.len());
    println!("   🔗 Streams affected: {:?}", result.stream_versions.keys().collect::<Vec<_>>());

    // Step 6: Demonstrate the audit trail
    println!("\n📊 Audit trail created:");
    println!("   - Content lifecycle tracked");  
    println!("   - User permissions verified");
    println!("   - Publishing action logged with metadata");

    println!("\n🎉 Example completed successfully!");
    println!("\n💡 Key EventCore features demonstrated:");
    println!("   ✅ Type-driven domain modeling with validation");
    println!("   ✅ Business rule enforcement in commands");
    println!("   ✅ Multi-stream atomic operations");
    println!("   ✅ State reconstruction from events");
    println!("   ✅ Error handling with meaningful messages");
    println!("   ✅ Cross-entity consistency boundaries");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_content_creation_workflow() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        // Create author
        let author_id = types::UserId::try_new("test-author".to_string()).unwrap();
        create_user(&executor, &author_id, "author@test.com", types::UserRole::Author).await.unwrap();

        // Create content
        let content_id = types::ContentId::try_new("test-content".to_string()).unwrap();
        let title = types::ContentTitle::try_new("Test Article".to_string()).unwrap();
        
        let create_command = CreateContentCommand {
            content_id: content_id.clone(),
            title,
            author_id: author_id.clone(),
        };

        let result = executor.execute(&create_command, create_command, ExecutionOptions::default()).await;
        assert!(result.is_ok());

        // Update content
        let body = types::ContentBody::try_new("Test content body".to_string()).unwrap();
        let update_command = UpdateContentCommand {
            content_id,
            new_body: body,
            updated_by: author_id,
        };

        let result = executor.execute(&update_command, update_command, ExecutionOptions::default()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publishing_requires_editor_role() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        // Create author and content
        let author_id = types::UserId::try_new("test-author".to_string()).unwrap();
        create_user(&executor, &author_id, "author@test.com", types::UserRole::Author).await.unwrap();

        let content_id = types::ContentId::try_new("test-content".to_string()).unwrap();
        let title = types::ContentTitle::try_new("Test Article".to_string()).unwrap();
        
        let create_command = CreateContentCommand {
            content_id: content_id.clone(),
            title,
            author_id: author_id.clone(),
        };

        executor.execute(&create_command, create_command, ExecutionOptions::default()).await.unwrap();

        // Try to publish as author (should fail)
        let publish_command = PublishContentCommand {
            content_id,
            published_by: author_id,
        };

        let result = executor.execute(&publish_command, publish_command, ExecutionOptions::default()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot publish content"));
    }

    #[tokio::test] 
    async fn test_successful_publishing_workflow() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        // Create author and editor
        let author_id = types::UserId::try_new("test-author".to_string()).unwrap();
        let editor_id = types::UserId::try_new("test-editor".to_string()).unwrap();
        
        create_user(&executor, &author_id, "author@test.com", types::UserRole::Author).await.unwrap();
        create_user(&executor, &editor_id, "editor@test.com", types::UserRole::Editor).await.unwrap();

        // Create content with body
        let content_id = types::ContentId::try_new("test-content".to_string()).unwrap();
        let title = types::ContentTitle::try_new("Test Article".to_string()).unwrap();
        
        let create_command = CreateContentCommand {
            content_id: content_id.clone(),
            title,
            author_id: author_id.clone(),
        };

        executor.execute(&create_command, create_command, ExecutionOptions::default()).await.unwrap();

        let body = types::ContentBody::try_new("Test content body".to_string()).unwrap();
        let update_command = UpdateContentCommand {
            content_id: content_id.clone(),
            new_body: body,
            updated_by: author_id,
        };

        executor.execute(&update_command, update_command, ExecutionOptions::default()).await.unwrap();

        // Publish as editor (should succeed)
        let publish_command = PublishContentCommand {
            content_id,
            published_by: editor_id,
        };

        let result = executor.execute(&publish_command, publish_command, ExecutionOptions::default()).await;
        assert!(result.is_ok());
        
        // Verify multiple events were written (content + audit)
        let events_written = result.unwrap().events_written.len();
        assert_eq!(events_written, 2); // ContentPublished + ActionAudited
    }
}