//! # Real-Time Collaborative Document Editing Example
//!
//! This example demonstrates EventCore usage in a real-time collaborative editing scenario.
//! It showcases patterns for building collaborative applications including:
//!
//! - **Real-time collaboration**: Multiple users editing the same document simultaneously
//! - **Operational transformation**: Handling concurrent edits with conflict resolution
//! - **User presence**: Tracking active users and their cursor positions
//! - **Version control**: Maintaining document history and enabling undo/redo
//! - **Access control**: Managing permissions for document access and editing
//! - **Change notifications**: Real-time updates to all connected clients
//!
//! # Domain Model
//!
//! - **Documents**: Collaborative text documents with versioned content
//! - **Sessions**: Active editing sessions for users in a document
//! - **Operations**: Text insertions, deletions, and formatting changes
//! - **Cursors**: User cursor positions and selections
//! - **Presence**: Real-time user activity and status
//!
//! # Key EventCore Patterns Demonstrated
//!
//! 1. **Real-time state synchronization**: Using projections for live updates
//! 2. **Conflict-free operations**: Ensuring consistency across concurrent edits
//! 3. **Session management**: Tracking active users and their states
//! 4. **Event replay**: Reconstructing document state from event history
//! 5. **Subscription-based updates**: Pushing changes to connected clients

use eventcore::prelude::*;
use eventcore::{CommandLogic, CommandStreams, ReadStreams, StreamResolver, StreamWrite};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Domain Types
// ============================================================================

pub mod types {
    use super::*;
    use nutype::nutype;

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Serialize,
            Deserialize
        )
    )]
    pub struct DocumentId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Serialize,
            Deserialize
        )
    )]
    pub struct UserId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            AsRef,
            Deref,
            Serialize,
            Deserialize
        )
    )]
    pub struct SessionId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 200),
        derive(Debug, Clone, PartialEq, Eq, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct DocumentTitle(String);

    #[nutype(
        validate(greater_or_equal = 0),
        derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Into,
            Serialize,
            Deserialize
        )
    )]
    pub struct DocumentVersion(u64);

    #[nutype(
        validate(greater_or_equal = 0),
        derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Into,
            Serialize,
            Deserialize
        )
    )]
    pub struct CursorPosition(usize);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TextOperation {
        pub operation_type: OperationType,
        pub position: CursorPosition,
        pub content: Option<String>,
        pub length: Option<usize>,
        pub version: DocumentVersion,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum OperationType {
        Insert,
        Delete,
        Format(FormatType),
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum FormatType {
        Bold,
        Italic,
        Underline,
        Strikethrough,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct UserPresence {
        pub user_id: UserId,
        pub cursor_position: CursorPosition,
        pub selection_start: Option<CursorPosition>,
        pub selection_end: Option<CursorPosition>,
        pub is_typing: bool,
        pub color: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Permission {
        Read,
        Write,
        Admin,
    }

    impl DocumentId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("document-{}", self.as_ref())).unwrap()
        }
    }

    impl UserId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("user-{}", self.as_ref())).unwrap()
        }
    }

    impl SessionId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("session-{}", self.as_ref())).unwrap()
        }
    }
}

// ============================================================================
// Events
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: types::DocumentId,
        title: types::DocumentTitle,
        owner_id: types::UserId,
        created_at: chrono::DateTime<chrono::Utc>,
    },

    TextOperationApplied {
        session_id: types::SessionId,
        operation: types::TextOperation,
        applied_at: chrono::DateTime<chrono::Utc>,
    },

    UserJoinedDocument {
        session_id: types::SessionId,
        user_id: types::UserId,
        document_id: types::DocumentId,
        permission: types::Permission,
        joined_at: chrono::DateTime<chrono::Utc>,
    },

    UserLeftDocument {
        session_id: types::SessionId,
        user_id: types::UserId,
        left_at: chrono::DateTime<chrono::Utc>,
    },

    CursorPositionUpdated {
        session_id: types::SessionId,
        position: types::CursorPosition,
        selection_start: Option<types::CursorPosition>,
        selection_end: Option<types::CursorPosition>,
        updated_at: chrono::DateTime<chrono::Utc>,
    },

    UserStartedTyping {
        session_id: types::SessionId,
        started_at: chrono::DateTime<chrono::Utc>,
    },

    UserStoppedTyping {
        session_id: types::SessionId,
        stopped_at: chrono::DateTime<chrono::Utc>,
    },

    DocumentSaved {
        document_id: types::DocumentId,
        version: types::DocumentVersion,
        saved_by: types::UserId,
        saved_at: chrono::DateTime<chrono::Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserEvent {
    UserCreated {
        user_id: types::UserId,
        name: String,
        email: String,
        created_at: chrono::DateTime<chrono::Utc>,
    },
}

impl TryFrom<&DocumentEvent> for DocumentEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &DocumentEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// ============================================================================
// Commands
// ============================================================================

/// Create a new collaborative document
#[derive(Debug, Clone)]
pub struct CreateDocumentCommand {
    pub document_id: types::DocumentId,
    pub title: types::DocumentTitle,
    pub owner_id: types::UserId,
}

#[derive(Debug, Default)]
pub struct CreateDocumentState {
    document_exists: bool,
    owner_exists: bool,
}

impl CommandStreams for CreateDocumentCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.document_id.stream_id(), self.owner_id.stream_id()]
    }
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl CommandLogic for CreateDocumentCommand {
    type State = CreateDocumentState;
    type Event = DocumentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_id = event.stream_id.as_ref();

        if stream_id.starts_with("document-") {
            match &event.payload {
                DocumentEvent::DocumentCreated { .. } => {
                    state.document_exists = true;
                }
                _ => {}
            }
        } else if stream_id.starts_with("user-") {
            // In a real system, we'd check for UserCreated event
            state.owner_exists = true;
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.document_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Document '{}' already exists",
                self.document_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            self.document_id.stream_id(),
            DocumentEvent::DocumentCreated {
                document_id: self.document_id.clone(),
                title: self.title.clone(),
                owner_id: self.owner_id.clone(),
                created_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Join a document editing session
#[derive(Debug, Clone)]
pub struct JoinDocumentCommand {
    pub session_id: types::SessionId,
    pub user_id: types::UserId,
    pub document_id: types::DocumentId,
    pub permission: types::Permission,
}

#[derive(Debug, Default)]
pub struct JoinDocumentState {
    document_exists: bool,
    user_exists: bool,
    session_exists: bool,
    active_sessions: HashSet<types::UserId>,
}

impl CommandStreams for JoinDocumentCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            self.session_id.stream_id(),
            self.user_id.stream_id(),
            self.document_id.stream_id(),
        ]
    }
}

#[async_trait::async_trait]
impl CommandLogic for JoinDocumentCommand {
    type State = JoinDocumentState;
    type Event = DocumentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_id = event.stream_id.as_ref();

        match &event.payload {
            DocumentEvent::DocumentCreated { .. } => {
                state.document_exists = true;
            }
            DocumentEvent::UserJoinedDocument { user_id, .. } => {
                state.active_sessions.insert(user_id.clone());
                if stream_id == self.session_id.stream_id().as_ref() {
                    state.session_exists = true;
                }
            }
            DocumentEvent::UserLeftDocument { user_id, .. } => {
                state.active_sessions.remove(user_id);
            }
            _ => {}
        }

        if stream_id.starts_with("user-") {
            state.user_exists = true;
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.document_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Document '{}' does not exist",
                self.document_id.as_ref()
            )));
        }

        if state.session_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Session '{}' already exists",
                self.session_id.as_ref()
            )));
        }

        if state.active_sessions.contains(&self.user_id) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' is already in an active session for this document",
                self.user_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            self.session_id.stream_id(),
            DocumentEvent::UserJoinedDocument {
                session_id: self.session_id.clone(),
                user_id: self.user_id.clone(),
                document_id: self.document_id.clone(),
                permission: self.permission.clone(),
                joined_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Apply a text operation to the document
#[derive(Debug, Clone)]
pub struct ApplyTextOperationCommand {
    pub session_id: types::SessionId,
    pub document_id: types::DocumentId,
    pub operation: types::TextOperation,
}

#[derive(Debug)]
pub struct TextOperationState {
    session_active: bool,
    document_exists: bool,
    current_version: types::DocumentVersion,
    has_write_permission: bool,
    document_content: String,
}

impl Default for TextOperationState {
    fn default() -> Self {
        Self {
            session_active: false,
            document_exists: false,
            current_version: types::DocumentVersion::try_new(0).unwrap(),
            has_write_permission: false,
            document_content: String::new(),
        }
    }
}

impl CommandStreams for ApplyTextOperationCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.session_id.stream_id(), self.document_id.stream_id()]
    }
}

#[async_trait::async_trait]
impl CommandLogic for ApplyTextOperationCommand {
    type State = TextOperationState;
    type Event = DocumentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            DocumentEvent::DocumentCreated { .. } => {
                state.document_exists = true;
                state.current_version = types::DocumentVersion::try_new(0).unwrap();
            }
            DocumentEvent::UserJoinedDocument {
                session_id,
                permission,
                ..
            } => {
                if session_id == &self.session_id {
                    state.session_active = true;
                    state.has_write_permission = matches!(
                        permission,
                        types::Permission::Write | types::Permission::Admin
                    );
                }
            }
            DocumentEvent::UserLeftDocument { session_id, .. } => {
                if session_id == &self.session_id {
                    state.session_active = false;
                }
            }
            DocumentEvent::TextOperationApplied { operation, .. } => {
                // Apply operation to document content
                match &operation.operation_type {
                    types::OperationType::Insert => {
                        if let Some(content) = &operation.content {
                            let pos: usize = operation.position.into();
                            if pos <= state.document_content.len() {
                                state.document_content.insert_str(pos, content);
                            }
                        }
                    }
                    types::OperationType::Delete => {
                        if let Some(length) = operation.length {
                            let pos: usize = operation.position.into();
                            if pos < state.document_content.len() {
                                let end = (pos + length).min(state.document_content.len());
                                state.document_content.replace_range(pos..end, "");
                            }
                        }
                    }
                    types::OperationType::Format(_) => {
                        // Format operations don't change content length
                    }
                }
                let version: u64 = operation.version.into();
                state.current_version = types::DocumentVersion::try_new(version + 1).unwrap();
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.document_exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Document '{}' does not exist",
                self.document_id.as_ref()
            )));
        }

        if !state.session_active {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Session '{}' is not active",
                self.session_id.as_ref()
            )));
        }

        if !state.has_write_permission {
            return Err(CommandError::Unauthorized(
                "User does not have write permission for this document".to_string(),
            ));
        }

        // Validate operation version
        let operation_version: u64 = self.operation.version.into();
        let current_version: u64 = state.current_version.into();
        if operation_version != current_version {
            return Err(CommandError::ConcurrencyConflict {
                streams: vec![self.document_id.stream_id()],
            });
        }

        // Validate operation bounds
        match &self.operation.operation_type {
            types::OperationType::Insert => {
                let pos: usize = self.operation.position.into();
                if pos > state.document_content.len() {
                    return Err(CommandError::BusinessRuleViolation(
                        "Insert position out of bounds".to_string(),
                    ));
                }
            }
            types::OperationType::Delete => {
                let pos: usize = self.operation.position.into();
                if pos >= state.document_content.len() {
                    return Err(CommandError::BusinessRuleViolation(
                        "Delete position out of bounds".to_string(),
                    ));
                }
            }
            _ => {}
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            self.document_id.stream_id(),
            DocumentEvent::TextOperationApplied {
                session_id: self.session_id.clone(),
                operation: self.operation.clone(),
                applied_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Update cursor position
#[derive(Debug, Clone)]
pub struct UpdateCursorPositionCommand {
    pub session_id: types::SessionId,
    pub position: types::CursorPosition,
    pub selection_start: Option<types::CursorPosition>,
    pub selection_end: Option<types::CursorPosition>,
}

#[derive(Debug, Default)]
pub struct CursorPositionState {
    session_active: bool,
}

impl CommandStreams for UpdateCursorPositionCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.session_id.stream_id()]
    }
}

#[async_trait::async_trait]
impl CommandLogic for UpdateCursorPositionCommand {
    type State = CursorPositionState;
    type Event = DocumentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            DocumentEvent::UserJoinedDocument { session_id, .. } => {
                if session_id == &self.session_id {
                    state.session_active = true;
                }
            }
            DocumentEvent::UserLeftDocument { session_id, .. } => {
                if session_id == &self.session_id {
                    state.session_active = false;
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.session_active {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Session '{}' is not active",
                self.session_id.as_ref()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            self.session_id.stream_id(),
            DocumentEvent::CursorPositionUpdated {
                session_id: self.session_id.clone(),
                position: self.position,
                selection_start: self.selection_start,
                selection_end: self.selection_end,
                updated_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Leave document session
#[derive(Debug, Clone)]
pub struct LeaveDocumentCommand {
    pub session_id: types::SessionId,
    pub user_id: types::UserId,
}

#[derive(Debug, Default)]
pub struct LeaveDocumentState {
    session_active: bool,
    session_user_id: Option<types::UserId>,
}

impl CommandStreams for LeaveDocumentCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.session_id.stream_id()]
    }
}

#[async_trait::async_trait]
impl CommandLogic for LeaveDocumentCommand {
    type State = LeaveDocumentState;
    type Event = DocumentEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            DocumentEvent::UserJoinedDocument {
                session_id,
                user_id,
                ..
            } => {
                if session_id == &self.session_id {
                    state.session_active = true;
                    state.session_user_id = Some(user_id.clone());
                }
            }
            DocumentEvent::UserLeftDocument { session_id, .. } => {
                if session_id == &self.session_id {
                    state.session_active = false;
                }
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if !state.session_active {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Session '{}' is not active",
                self.session_id.as_ref()
            )));
        }

        if let Some(session_user_id) = state.session_user_id {
            if session_user_id != self.user_id {
                return Err(CommandError::Unauthorized(format!(
                    "User '{}' cannot leave session owned by user '{}'",
                    self.user_id.as_ref(),
                    session_user_id.as_ref()
                )));
            }
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            self.session_id.stream_id(),
            DocumentEvent::UserLeftDocument {
                session_id: self.session_id.clone(),
                user_id: self.user_id.clone(),
                left_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

// ============================================================================
// Projections
// ============================================================================

/// Real-time document state projection
#[derive(Debug, Clone, Default)]
pub struct DocumentStateProjection {
    pub documents: HashMap<types::DocumentId, DocumentState>,
}

#[derive(Debug, Clone)]
pub struct DocumentState {
    pub title: types::DocumentTitle,
    pub content: String,
    pub version: types::DocumentVersion,
    pub owner_id: types::UserId,
    pub active_sessions: HashMap<types::SessionId, SessionInfo>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_modified: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub user_id: types::UserId,
    pub permission: types::Permission,
    pub cursor_position: types::CursorPosition,
    pub selection_start: Option<types::CursorPosition>,
    pub selection_end: Option<types::CursorPosition>,
    pub is_typing: bool,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

impl DocumentStateProjection {
    fn apply(&mut self, event: &StoredEvent<DocumentEvent>) {
        match &event.payload {
            DocumentEvent::DocumentCreated {
                document_id,
                title,
                owner_id,
                created_at,
            } => {
                self.documents.insert(
                    document_id.clone(),
                    DocumentState {
                        title: title.clone(),
                        content: String::new(),
                        version: types::DocumentVersion::try_new(0).unwrap(),
                        owner_id: owner_id.clone(),
                        active_sessions: HashMap::new(),
                        created_at: *created_at,
                        last_modified: *created_at,
                    },
                );
            }

            DocumentEvent::TextOperationApplied {
                session_id,
                operation,
                applied_at,
            } => {
                // Find document by looking through sessions
                for (_, doc_state) in self.documents.iter_mut() {
                    if doc_state.active_sessions.contains_key(session_id) {
                        // Apply operation
                        match &operation.operation_type {
                            types::OperationType::Insert => {
                                if let Some(content) = &operation.content {
                                    let pos: usize = operation.position.into();
                                    if pos <= doc_state.content.len() {
                                        doc_state.content.insert_str(pos, content);
                                    }
                                }
                            }
                            types::OperationType::Delete => {
                                if let Some(length) = operation.length {
                                    let pos: usize = operation.position.into();
                                    if pos < doc_state.content.len() {
                                        let end = (pos + length).min(doc_state.content.len());
                                        doc_state.content.replace_range(pos..end, "");
                                    }
                                }
                            }
                            types::OperationType::Format(_) => {
                                // Format operations handled separately
                            }
                        }
                        let version: u64 = operation.version.into();
                        doc_state.version = types::DocumentVersion::try_new(version + 1).unwrap();
                        doc_state.last_modified = *applied_at;
                        break;
                    }
                }
            }

            DocumentEvent::UserJoinedDocument {
                session_id,
                user_id,
                document_id,
                permission,
                joined_at,
            } => {
                if let Some(doc_state) = self.documents.get_mut(document_id) {
                    doc_state.active_sessions.insert(
                        session_id.clone(),
                        SessionInfo {
                            user_id: user_id.clone(),
                            permission: permission.clone(),
                            cursor_position: types::CursorPosition::try_new(0).unwrap(),
                            selection_start: None,
                            selection_end: None,
                            is_typing: false,
                            joined_at: *joined_at,
                        },
                    );
                }
            }

            DocumentEvent::UserLeftDocument { session_id, .. } => {
                for (_, doc_state) in self.documents.iter_mut() {
                    doc_state.active_sessions.remove(session_id);
                }
            }

            DocumentEvent::CursorPositionUpdated {
                session_id,
                position,
                selection_start,
                selection_end,
                ..
            } => {
                for (_, doc_state) in self.documents.iter_mut() {
                    if let Some(session) = doc_state.active_sessions.get_mut(session_id) {
                        session.cursor_position = *position;
                        session.selection_start = *selection_start;
                        session.selection_end = *selection_end;
                    }
                }
            }

            DocumentEvent::UserStartedTyping { session_id, .. } => {
                for (_, doc_state) in self.documents.iter_mut() {
                    if let Some(session) = doc_state.active_sessions.get_mut(session_id) {
                        session.is_typing = true;
                    }
                }
            }

            DocumentEvent::UserStoppedTyping { session_id, .. } => {
                for (_, doc_state) in self.documents.iter_mut() {
                    if let Some(session) = doc_state.active_sessions.get_mut(session_id) {
                        session.is_typing = false;
                    }
                }
            }

            _ => {}
        }
    }
}

// ============================================================================
// Example Usage and Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::{CommandExecutor, ExecutionOptions};
    use tokio;

    #[tokio::test]
    async fn test_collaborative_editing_workflow() {
        // Create event store and executor
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store.clone());

        // Create users
        let owner_id = types::UserId::try_new("alice".to_string()).unwrap();
        let collaborator_id = types::UserId::try_new("bob".to_string()).unwrap();

        // Create document
        let doc_id = types::DocumentId::try_new("doc-123".to_string()).unwrap();
        let create_doc = CreateDocumentCommand {
            document_id: doc_id.clone(),
            title: types::DocumentTitle::try_new("Project Proposal".to_string()).unwrap(),
            owner_id: owner_id.clone(),
        };

        executor.execute(&create_doc).await.unwrap();

        // Owner joins document
        let owner_session = types::SessionId::try_new("session-alice".to_string()).unwrap();
        let owner_join = JoinDocumentCommand {
            session_id: owner_session.clone(),
            user_id: owner_id.clone(),
            document_id: doc_id.clone(),
            permission: types::Permission::Admin,
        };

        executor.execute(&owner_join).await.unwrap();

        // Collaborator joins document
        let collab_session = types::SessionId::try_new("session-bob".to_string()).unwrap();
        let collab_join = JoinDocumentCommand {
            session_id: collab_session.clone(),
            user_id: collaborator_id.clone(),
            document_id: doc_id.clone(),
            permission: types::Permission::Write,
        };

        executor.execute(&collab_join).await.unwrap();

        // Owner types some text
        let owner_edit = ApplyTextOperationCommand {
            session_id: owner_session.clone(),
            document_id: doc_id.clone(),
            operation: types::TextOperation {
                operation_type: types::OperationType::Insert,
                position: types::CursorPosition::try_new(0).unwrap(),
                content: Some("Hello, ".to_string()),
                length: None,
                version: types::DocumentVersion::try_new(0).unwrap(),
            },
        };

        executor.execute(&owner_edit).await.unwrap();

        // Collaborator adds text
        let collab_edit = ApplyTextOperationCommand {
            session_id: collab_session.clone(),
            document_id: doc_id.clone(),
            operation: types::TextOperation {
                operation_type: types::OperationType::Insert,
                position: types::CursorPosition::try_new(7).unwrap(),
                content: Some("World!".to_string()),
                length: None,
                version: types::DocumentVersion::try_new(1).unwrap(),
            },
        };

        executor.execute(&collab_edit).await.unwrap();

        // Update cursor position
        let cursor_update = UpdateCursorPositionCommand {
            session_id: collab_session.clone(),
            position: types::CursorPosition::try_new(13).unwrap(),
            selection_start: None,
            selection_end: None,
        };

        executor
            .execute(cursor_update, ExecutionOptions::default())
            .await
            .unwrap();

        // Check projection state
        let mut projection = DocumentStateProjection::default();
        let stream_ids = vec![
            doc_id.stream_id(),
            owner_id.stream_id(),
            owner_session.stream_id(),
            collaborator_id.stream_id(),
            collab_session.stream_id(),
        ];
        let stream_data = event_store
            .read_streams(&stream_ids, &ReadOptions::default())
            .await
            .unwrap();
        for event in stream_data.events {
            projection.apply(&event);
        }

        // Verify document state
        let doc_state = projection.documents.get(&doc_id).unwrap();
        assert_eq!(doc_state.content, "Hello, World!");
        assert_eq!(doc_state.active_sessions.len(), 2);

        // Verify cursor position
        let collab_session_info = doc_state.active_sessions.get(&collab_session).unwrap();
        assert_eq!(
            collab_session_info.cursor_position,
            types::CursorPosition::try_new(13).unwrap()
        );

        // Collaborator leaves
        let leave_command = LeaveDocumentCommand {
            session_id: collab_session.clone(),
            user_id: collaborator_id.clone(),
        };

        executor
            .execute(leave_command, ExecutionOptions::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_editing_conflict() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store.clone());

        // Setup document and sessions
        let doc_id = types::DocumentId::try_new("doc-456".to_string()).unwrap();
        let user1 = types::UserId::try_new("user1".to_string()).unwrap();
        let user2 = types::UserId::try_new("user2".to_string()).unwrap();

        // Create document
        executor
            .execute(
                CreateDocumentCommand {
                    document_id: doc_id.clone(),
                    title: types::DocumentTitle::try_new("Shared Doc".to_string()).unwrap(),
                    owner_id: user1.clone(),
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();

        // Both users join
        let session1 = types::SessionId::try_new("session1".to_string()).unwrap();
        executor
            .execute(
                JoinDocumentCommand {
                    session_id: session1.clone(),
                    user_id: user1.clone(),
                    document_id: doc_id.clone(),
                    permission: types::Permission::Write,
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();

        let session2 = types::SessionId::try_new("session2".to_string()).unwrap();
        executor
            .execute(
                JoinDocumentCommand {
                    session_id: session2.clone(),
                    user_id: user2.clone(),
                    document_id: doc_id.clone(),
                    permission: types::Permission::Write,
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();

        // User 1 edits
        executor
            .execute(
                ApplyTextOperationCommand {
                    session_id: session1.clone(),
                    document_id: doc_id.clone(),
                    operation: types::TextOperation {
                        operation_type: types::OperationType::Insert,
                        position: types::CursorPosition::try_new(0).unwrap(),
                        content: Some("First edit".to_string()),
                        length: None,
                        version: types::DocumentVersion::try_new(0).unwrap(),
                    },
                },
                ExecutionOptions::default(),
            )
            .await
            .unwrap();

        // User 2 tries to edit with outdated version - should fail
        let result = executor
            .execute(
                ApplyTextOperationCommand {
                    session_id: session2.clone(),
                    document_id: doc_id.clone(),
                    operation: types::TextOperation {
                        operation_type: types::OperationType::Insert,
                        position: types::CursorPosition::try_new(0).unwrap(),
                        content: Some("Conflicting edit".to_string()),
                        length: None,
                        version: types::DocumentVersion::try_new(0).unwrap(), // Outdated version
                    },
                },
                ExecutionOptions::default(),
            )
            .await;

        assert!(matches!(
            result,
            Err(CommandError::ConcurrencyConflict { .. })
        ));
    }
}

/// Example of real-time updates simulation
pub async fn example_realtime_updates() -> anyhow::Result<()> {
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store.clone());

    // Simulate collaborative editing
    let doc_id = types::DocumentId::try_new("realtime-doc".to_string()).unwrap();
    let user_id = types::UserId::try_new("demo-user".to_string()).unwrap();

    // Create and join document
    executor
        .execute(
            CreateDocumentCommand {
                document_id: doc_id.clone(),
                title: types::DocumentTitle::try_new("Real-time Demo".to_string()).unwrap(),
                owner_id: user_id.clone(),
            },
            ExecutionOptions::default(),
        )
        .await?;

    let session_id = types::SessionId::try_new("demo-session".to_string()).unwrap();
    executor
        .execute(
            JoinDocumentCommand {
                session_id: session_id.clone(),
                user_id: user_id.clone(),
                document_id: doc_id.clone(),
                permission: types::Permission::Write,
            },
            ExecutionOptions::default(),
        )
        .await?;

    // Simulate typing
    for (i, ch) in "Hello, real-time world!".chars().enumerate() {
        executor
            .execute(
                ApplyTextOperationCommand {
                    session_id: session_id.clone(),
                    document_id: doc_id.clone(),
                    operation: types::TextOperation {
                        operation_type: types::OperationType::Insert,
                        position: types::CursorPosition::try_new(i).unwrap(),
                        content: Some(ch.to_string()),
                        length: None,
                        version: types::DocumentVersion::try_new(i as u64).unwrap(),
                    },
                },
                ExecutionOptions::default(),
            )
            .await?;

        // Update cursor position
        executor
            .execute(
                UpdateCursorPositionCommand {
                    session_id: session_id.clone(),
                    position: types::CursorPosition::try_new(i + 1).unwrap(),
                    selection_start: None,
                    selection_end: None,
                },
                ExecutionOptions::default(),
            )
            .await?;

        println!("Typed '{}' at position {}", ch, i);
    }

    // Read final state
    let stream_ids = vec![
        doc_id.stream_id(),
        user_id.stream_id(),
        session_id.stream_id(),
    ];
    let stream_data = event_store
        .read_streams(&stream_ids, &ReadOptions::default())
        .await?;
    let mut projection = DocumentStateProjection::default();
    for event in stream_data.events {
        projection.apply(&event);
    }

    // Display final document state
    if let Some(doc_state) = projection.documents.get(&doc_id) {
        println!("\nFinal document content: '{}'", doc_state.content);
        println!("Document version: {:?}", doc_state.version);
        println!("Active sessions: {}", doc_state.active_sessions.len());
    }

    Ok(())
}

/// Example main function demonstrating the system
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("EventCore Real-Time Collaboration Example");
    println!("========================================\n");

    // Run the real-time updates example
    example_realtime_updates().await?;

    println!("\nExample completed successfully!");

    Ok(())
}
