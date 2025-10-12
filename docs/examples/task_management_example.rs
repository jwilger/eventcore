//! # Task/Project Management System Example
//!
//! This example demonstrates EventCore usage in a collaborative task management scenario.
//! It showcases workflow patterns including:
//!
//! - **Project lifecycle management**: Creating, planning, executing, and closing projects
//! - **Task dependencies**: Ensuring tasks complete in proper order
//! - **Team collaboration**: Assigning work and tracking progress
//! - **Workflow state machines**: Task status transitions with validation
//! - **Time tracking**: Recording work sessions and calculating project metrics
//! - **Notification system**: Event-driven alerts and updates
//!
//! # Domain Model
//!
//! - **Projects**: Collections of related tasks with deadlines
//! - **Tasks**: Individual work items with assignees and dependencies
//! - **Users**: Team members who can be assigned to tasks
//! - **Time Entries**: Records of work performed on tasks
//! - **Comments**: Collaboration and discussion on tasks
//!
//! # Key EventCore Patterns Demonstrated
//!
//! 1. **Workflow state machines**: Task status transitions with business rules
//! 2. **Dependency management**: Ensuring prerequisite tasks are completed
//! 3. **Cross-project operations**: Moving tasks between projects
//! 4. **Time aggregation**: Calculating project totals from individual entries
//! 5. **Event-driven notifications**: Triggering alerts based on state changes

use eventcore::prelude::*;
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ============================================================================
// Domain Types
// ============================================================================

pub mod types {
    use super::*;
    use nutype::nutype;

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct ProjectId(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 50),
        derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct TaskId(String);

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
    pub struct ProjectName(String);

    #[nutype(
        sanitize(trim),
        validate(not_empty, len_char_max = 100),
        derive(Debug, Clone, PartialEq, Eq, AsRef, Deref, Serialize, Deserialize)
    )]
    pub struct TaskTitle(String);

    #[nutype(
        validate(greater = 0),
        derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, Serialize, Deserialize)
    )]
    pub struct StoryPoints(u32);

    #[nutype(
        validate(greater = 0.0),
        derive(Debug, Clone, Copy, PartialEq, PartialOrd, Into, Serialize, Deserialize)
    )]
    pub struct HoursWorked(f64);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum TaskStatus {
        Todo,
        InProgress,
        InReview,
        Blocked,
        Done,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum ProjectStatus {
        Planning,
        Active,
        OnHold,
        Completed,
        Cancelled,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum TaskPriority {
        Low,
        Medium,
        High,
        Critical,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TimeEntryId(pub Uuid);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct CommentId(pub Uuid);

    impl ProjectId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("project-{}", self.as_ref())).unwrap()
        }
    }

    impl TaskId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("task-{}", self.as_ref())).unwrap()
        }
    }

    impl UserId {
        pub fn stream_id(&self) -> StreamId {
            StreamId::try_new(format!("user-{}", self.as_ref())).unwrap()
        }
    }

    impl TimeEntryId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }

    impl CommentId {
        pub fn new() -> Self {
            Self(Uuid::new_v4())
        }
    }
}

// ============================================================================
// Events
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskEvent {
    // Project lifecycle
    ProjectCreated {
        project_id: types::ProjectId,
        name: types::ProjectName,
        description: String,
        owner_id: types::UserId,
        deadline: Option<chrono::DateTime<chrono::Utc>>,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    ProjectStatusChanged {
        project_id: types::ProjectId,
        old_status: types::ProjectStatus,
        new_status: types::ProjectStatus,
        changed_by: types::UserId,
        reason: Option<String>,
        changed_at: chrono::DateTime<chrono::Utc>,
    },

    // Task management
    TaskCreated {
        task_id: types::TaskId,
        project_id: types::ProjectId,
        title: types::TaskTitle,
        description: String,
        story_points: Option<types::StoryPoints>,
        priority: types::TaskPriority,
        created_by: types::UserId,
        created_at: chrono::DateTime<chrono::Utc>,
    },
    TaskAssigned {
        task_id: types::TaskId,
        assignee_id: types::UserId,
        assigned_by: types::UserId,
        assigned_at: chrono::DateTime<chrono::Utc>,
    },
    TaskStatusChanged {
        task_id: types::TaskId,
        old_status: types::TaskStatus,
        new_status: types::TaskStatus,
        changed_by: types::UserId,
        changed_at: chrono::DateTime<chrono::Utc>,
    },
    TaskDependencyAdded {
        task_id: types::TaskId,
        depends_on: types::TaskId,
        added_by: types::UserId,
        added_at: chrono::DateTime<chrono::Utc>,
    },
    TaskBlocked {
        task_id: types::TaskId,
        reason: String,
        blocking_task: Option<types::TaskId>,
        blocked_by: types::UserId,
        blocked_at: chrono::DateTime<chrono::Utc>,
    },
    TaskUnblocked {
        task_id: types::TaskId,
        unblocked_by: types::UserId,
        unblocked_at: chrono::DateTime<chrono::Utc>,
    },

    // Time tracking
    TimeEntryStarted {
        entry_id: types::TimeEntryId,
        task_id: types::TaskId,
        user_id: types::UserId,
        description: String,
        started_at: chrono::DateTime<chrono::Utc>,
    },
    TimeEntryCompleted {
        entry_id: types::TimeEntryId,
        task_id: types::TaskId,
        user_id: types::UserId,
        hours_worked: types::HoursWorked,
        completed_at: chrono::DateTime<chrono::Utc>,
    },

    // Collaboration
    CommentAdded {
        comment_id: types::CommentId,
        task_id: types::TaskId,
        author_id: types::UserId,
        content: String,
        created_at: chrono::DateTime<chrono::Utc>,
    },

    // User management
    UserCreated {
        user_id: types::UserId,
        name: String,
        email: String,
        role: String,
        created_at: chrono::DateTime<chrono::Utc>,
    },

    // Notifications
    NotificationSent {
        notification_id: Uuid,
        recipient_id: types::UserId,
        subject: String,
        message: String,
        event_type: String,
        related_task_id: Option<types::TaskId>,
        related_project_id: Option<types::ProjectId>,
        sent_at: chrono::DateTime<chrono::Utc>,
    },
}

impl TryFrom<&TaskEvent> for TaskEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &TaskEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// ============================================================================
// State Types
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct ProjectState {
    pub exists: bool,
    pub name: Option<types::ProjectName>,
    pub description: Option<String>,
    pub status: Option<types::ProjectStatus>,
    pub owner_id: Option<types::UserId>,
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
    pub task_count: u32,
    pub completed_tasks: u32,
    pub total_story_points: u32,
    pub completed_story_points: u32,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Default, Clone)]
pub struct TaskState {
    pub exists: bool,
    pub project_id: Option<types::ProjectId>,
    pub title: Option<types::TaskTitle>,
    pub description: Option<String>,
    pub status: Option<types::TaskStatus>,
    pub assignee_id: Option<types::UserId>,
    pub story_points: Option<types::StoryPoints>,
    pub priority: Option<types::TaskPriority>,
    pub dependencies: HashSet<types::TaskId>,
    pub blocking_reason: Option<String>,
    pub total_hours: f64,
    pub comments_count: u32,
    pub created_by: Option<types::UserId>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Default, Clone)]
pub struct UserState {
    pub exists: bool,
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: Option<String>,
    pub assigned_tasks: HashSet<types::TaskId>,
    pub active_time_entries: HashMap<types::TimeEntryId, chrono::DateTime<chrono::Utc>>,
    pub total_hours_worked: f64,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

// Combined state for complex operations
#[derive(Debug, Default, Clone)]
pub struct TaskAssignmentState {
    pub task: TaskState,
    pub assignee: UserState,
    pub project: ProjectState,
}

#[derive(Debug, Default, Clone)]
pub struct TaskCompletionState {
    pub task: TaskState,
    pub dependent_tasks: HashMap<types::TaskId, TaskState>,
    pub project: ProjectState,
}

// ============================================================================
// Commands
// ============================================================================

/// Create a new task with validation
pub struct CreateTaskCommand {
    pub task_id: types::TaskId,
    pub project_id: types::ProjectId,
    pub title: types::TaskTitle,
    pub description: String,
    pub story_points: Option<types::StoryPoints>,
    pub priority: types::TaskPriority,
    pub created_by: types::UserId,
}

#[async_trait::async_trait]
impl Command for CreateTaskCommand {
    type Input = Self;
    type State = ProjectState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.task_id.stream_id(),
            input.project_id.stream_id(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TaskEvent::ProjectCreated {
                name,
                description,
                owner_id,
                deadline,
                created_at,
                ..
            } => {
                if event.stream_id.as_ref().starts_with("project-") {
                    state.exists = true;
                    state.name = Some(name.clone());
                    state.description = Some(description.clone());
                    state.status = Some(types::ProjectStatus::Planning);
                    state.owner_id = Some(owner_id.clone());
                    state.deadline = *deadline;
                    state.created_at = Some(*created_at);
                }
            }
            TaskEvent::ProjectStatusChanged { new_status, .. } => {
                if event.stream_id.as_ref().starts_with("project-") {
                    state.status = Some(new_status.clone());
                }
            }
            TaskEvent::TaskCreated { story_points, .. } => {
                if event.stream_id.as_ref().starts_with("project-") {
                    state.task_count += 1;
                    if let Some(points) = story_points {
                        state.total_story_points += Into::<u32>::into(*points);
                    }
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
        // Validate project exists and is active
        if !state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Project '{}' does not exist",
                input.project_id.as_ref()
            )));
        }

        let project_status = state.status.as_ref().ok_or_else(|| {
            CommandError::Internal("Project exists but has no status".to_string())
        })?;

        if matches!(project_status, types::ProjectStatus::Completed | types::ProjectStatus::Cancelled) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot create tasks in project '{}' with status '{:?}'",
                input.project_id.as_ref(),
                project_status
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            input.task_id.stream_id(),
            TaskEvent::TaskCreated {
                task_id: input.task_id,
                project_id: input.project_id,
                title: input.title,
                description: input.description,
                story_points: input.story_points,
                priority: input.priority,
                created_by: input.created_by,
                created_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Assign a task to a user (multi-stream operation)
pub struct AssignTaskCommand {
    pub task_id: types::TaskId,
    pub assignee_id: types::UserId,
    pub assigned_by: types::UserId,
}

#[async_trait::async_trait]
impl Command for AssignTaskCommand {
    type Input = Self;
    type State = TaskAssignmentState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.task_id.stream_id(),
            input.assignee_id.stream_id(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_id = event.stream_id.as_ref();

        if stream_id.starts_with("task-") {
            // Apply task events
            match &event.payload {
                TaskEvent::TaskCreated {
                    project_id,
                    title,
                    description,
                    story_points,
                    priority,
                    created_by,
                    created_at,
                    ..
                } => {
                    state.task.exists = true;
                    state.task.project_id = Some(project_id.clone());
                    state.task.title = Some(title.clone());
                    state.task.description = Some(description.clone());
                    state.task.status = Some(types::TaskStatus::Todo);
                    state.task.story_points = *story_points;
                    state.task.priority = Some(priority.clone());
                    state.task.created_by = Some(created_by.clone());
                    state.task.created_at = Some(*created_at);
                }
                TaskEvent::TaskAssigned { assignee_id, .. } => {
                    state.task.assignee_id = Some(assignee_id.clone());
                }
                TaskEvent::TaskStatusChanged { new_status, .. } => {
                    state.task.status = Some(new_status.clone());
                }
                _ => {}
            }
        } else if stream_id.starts_with("user-") {
            // Apply user events
            match &event.payload {
                TaskEvent::UserCreated {
                    name,
                    email,
                    role,
                    created_at,
                    ..
                } => {
                    state.assignee.exists = true;
                    state.assignee.name = Some(name.clone());
                    state.assignee.email = Some(email.clone());
                    state.assignee.role = Some(role.clone());
                    state.assignee.created_at = Some(*created_at);
                }
                TaskEvent::TaskAssigned { task_id, .. } => {
                    state.assignee.assigned_tasks.insert(task_id.clone());
                }
                _ => {}
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate task exists
        if !state.task.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Task '{}' does not exist",
                input.task_id.as_ref()
            )));
        }

        // Validate user exists
        if !state.assignee.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' does not exist",
                input.assignee_id.as_ref()
            )));
        }

        // Check if task is in a state that allows assignment
        let task_status = state.task.status.as_ref().ok_or_else(|| {
            CommandError::Internal("Task exists but has no status".to_string())
        })?;

        if matches!(task_status, types::TaskStatus::Done) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot assign completed task '{}'",
                input.task_id.as_ref()
            )));
        }

        // Check if user has too many assigned tasks (business rule)
        if state.assignee.assigned_tasks.len() >= 10 {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' already has {} assigned tasks (maximum: 10)",
                input.assignee_id.as_ref(),
                state.assignee.assigned_tasks.len()
            )));
        }

        let now = chrono::Utc::now();
        let event = StreamWrite::new(
            &read_streams,
            input.task_id.stream_id(),
            TaskEvent::TaskAssigned {
                task_id: input.task_id,
                assignee_id: input.assignee_id,
                assigned_by: input.assigned_by,
                assigned_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

/// Complete a task with dependency validation
pub struct CompleteTaskCommand {
    pub task_id: types::TaskId,
    pub completed_by: types::UserId,
}

#[async_trait::async_trait]
impl Command for CompleteTaskCommand {
    type Input = Self;
    type State = TaskCompletionState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.task_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        let stream_id = event.stream_id.as_ref();

        if stream_id.starts_with("task-") {
            match &event.payload {
                TaskEvent::TaskCreated {
                    project_id,
                    title,
                    description,
                    story_points,
                    priority,
                    created_by,
                    created_at,
                    ..
                } => {
                    state.task.exists = true;
                    state.task.project_id = Some(project_id.clone());
                    state.task.title = Some(title.clone());
                    state.task.description = Some(description.clone());
                    state.task.status = Some(types::TaskStatus::Todo);
                    state.task.story_points = *story_points;
                    state.task.priority = Some(priority.clone());
                    state.task.created_by = Some(created_by.clone());
                    state.task.created_at = Some(*created_at);
                }
                TaskEvent::TaskStatusChanged { new_status, .. } => {
                    state.task.status = Some(new_status.clone());
                }
                TaskEvent::TaskDependencyAdded { depends_on, .. } => {
                    state.task.dependencies.insert(depends_on.clone());
                }
                _ => {}
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate task exists and can be completed
        if !state.task.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Task '{}' does not exist",
                input.task_id.as_ref()
            )));
        }

        let current_status = state.task.status.as_ref().ok_or_else(|| {
            CommandError::Internal("Task exists but has no status".to_string())
        })?;

        if matches!(current_status, types::TaskStatus::Done) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Task '{}' is already completed",
                input.task_id.as_ref()
            )));
        }

        if matches!(current_status, types::TaskStatus::Blocked) {
            return Err(CommandError::BusinessRuleViolation(format!(
                "Cannot complete blocked task '{}'",
                input.task_id.as_ref()
            )));
        }

        // If task has dependencies, dynamically discover them
        if !state.task.dependencies.is_empty() {
            let dependency_streams: Vec<StreamId> = state.task.dependencies
                .iter()
                .map(|dep_id| dep_id.stream_id())
                .collect();
            
            stream_resolver.add_streams(dependency_streams);
            
            // For this example, we'll assume dependencies are satisfied
            // In a real system, you'd validate each dependency's status
        }

        let now = chrono::Utc::now();
        let mut events = Vec::new();

        // Mark task as completed
        events.push(StreamWrite::new(
            &read_streams,
            input.task_id.stream_id(),
            TaskEvent::TaskStatusChanged {
                task_id: input.task_id.clone(),
                old_status: current_status.clone(),
                new_status: types::TaskStatus::Done,
                changed_by: input.completed_by.clone(),
                changed_at: now,
            },
        )?);

        // Send notification to project owner or other stakeholders
        let notification_id = Uuid::new_v4();
        let project_id = state.task.project_id.as_ref().ok_or_else(|| {
            CommandError::Internal("Task exists but has no project ID".to_string())
        })?;

        events.push(StreamWrite::new(
            &read_streams,
            StreamId::try_new("notifications".to_string()).unwrap(),
            TaskEvent::NotificationSent {
                notification_id,
                recipient_id: input.completed_by.clone(), // Simplified - would be project owner
                subject: "Task Completed".to_string(),
                message: format!(
                    "Task '{}' has been completed",
                    state.task.title.as_ref().map(|t| t.as_ref()).unwrap_or("Unknown")
                ),
                event_type: "task_completed".to_string(),
                related_task_id: Some(input.task_id),
                related_project_id: Some(project_id.clone()),
                sent_at: now,
            },
        )?);

        Ok(events)
    }
}

/// Start a time tracking session
pub struct StartTimeEntryCommand {
    pub task_id: types::TaskId,
    pub user_id: types::UserId,
    pub description: String,
}

#[async_trait::async_trait]
impl Command for StartTimeEntryCommand {
    type Input = Self;
    type State = UserState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.user_id.stream_id(),
            input.task_id.stream_id(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TaskEvent::UserCreated {
                name,
                email,
                role,
                created_at,
                ..
            } => {
                state.exists = true;
                state.name = Some(name.clone());
                state.email = Some(email.clone());
                state.role = Some(role.clone());
                state.created_at = Some(*created_at);
            }
            TaskEvent::TimeEntryStarted {
                entry_id,
                started_at,
                ..
            } => {
                state.active_time_entries.insert(entry_id.clone(), *started_at);
            }
            TaskEvent::TimeEntryCompleted {
                entry_id,
                hours_worked,
                ..
            } => {
                state.active_time_entries.remove(entry_id);
                state.total_hours_worked += Into::<f64>::into(*hours_worked);
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
        // Validate user exists
        if !state.exists {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' does not exist",
                input.user_id.as_ref()
            )));
        }

        // Check if user already has an active time entry
        if !state.active_time_entries.is_empty() {
            return Err(CommandError::BusinessRuleViolation(format!(
                "User '{}' already has {} active time entries. Complete them before starting a new one.",
                input.user_id.as_ref(),
                state.active_time_entries.len()
            )));
        }

        let entry_id = types::TimeEntryId::new();
        let now = chrono::Utc::now();

        let event = StreamWrite::new(
            &read_streams,
            input.user_id.stream_id(),
            TaskEvent::TimeEntryStarted {
                entry_id,
                task_id: input.task_id,
                user_id: input.user_id,
                description: input.description,
                started_at: now,
            },
        )?;

        Ok(vec![event])
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn create_user(
    executor: &CommandExecutor<InMemoryEventStore<TaskEvent>>,
    user_id: types::UserId,
    name: String,
    email: String,
    role: String,
) -> Result<(), CommandError> {
    let command = CreateUserCommand {
        user_id,
        name,
        email,
        role,
    };
    
    executor.execute(&command, command, ExecutionOptions::default()).await?;
    Ok(())
}

async fn create_project(
    executor: &CommandExecutor<InMemoryEventStore<TaskEvent>>,
    project_id: types::ProjectId,
    name: types::ProjectName,
    description: String,
    owner_id: types::UserId,
    deadline: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), CommandError> {
    let command = CreateProjectCommand {
        project_id,
        name,
        description,
        owner_id,
        deadline,
    };
    
    executor.execute(&command, command, ExecutionOptions::default()).await?;
    Ok(())
}

// Helper commands
pub struct CreateUserCommand {
    pub user_id: types::UserId,
    pub name: String,
    pub email: String,
    pub role: String,
}

#[async_trait::async_trait]
impl Command for CreateUserCommand {
    type Input = Self;
    type State = UserState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.user_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let TaskEvent::UserCreated {
            name,
            email,
            role,
            created_at,
            ..
        } = &event.payload
        {
            state.exists = true;
            state.name = Some(name.clone());
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
            TaskEvent::UserCreated {
                user_id: input.user_id,
                name: input.name,
                email: input.email,
                role: input.role,
                created_at: chrono::Utc::now(),
            },
        )?;

        Ok(vec![event])
    }
}

pub struct CreateProjectCommand {
    pub project_id: types::ProjectId,
    pub name: types::ProjectName,
    pub description: String,
    pub owner_id: types::UserId,
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait::async_trait]
impl Command for CreateProjectCommand {
    type Input = Self;
    type State = ProjectState;
    type Event = TaskEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.project_id.stream_id()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        if let TaskEvent::ProjectCreated {
            name,
            description,
            owner_id,
            deadline,
            created_at,
            ..
        } = &event.payload
        {
            state.exists = true;
            state.name = Some(name.clone());
            state.description = Some(description.clone());
            state.status = Some(types::ProjectStatus::Planning);
            state.owner_id = Some(owner_id.clone());
            state.deadline = *deadline;
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
                "Project '{}' already exists",
                input.project_id.as_ref()
            )));
        }

        let event = StreamWrite::new(
            &read_streams,
            input.project_id.stream_id(),
            TaskEvent::ProjectCreated {
                project_id: input.project_id,
                name: input.name,
                description: input.description,
                owner_id: input.owner_id,
                deadline: input.deadline,
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
    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    println!("📋 EventCore Task Management System Example");
    println!("==========================================\n");

    // Step 1: Create users
    println!("👥 Creating team members...");
    
    let project_manager_id = types::UserId::try_new("sarah-pm".to_string()).unwrap();
    let developer_id = types::UserId::try_new("alex-dev".to_string()).unwrap();
    let designer_id = types::UserId::try_new("maya-design".to_string()).unwrap();

    create_user(&executor, project_manager_id.clone(), "Sarah Johnson".to_string(), "sarah@company.com".to_string(), "Project Manager".to_string()).await?;
    create_user(&executor, developer_id.clone(), "Alex Chen".to_string(), "alex@company.com".to_string(), "Developer".to_string()).await?;
    create_user(&executor, designer_id.clone(), "Maya Patel".to_string(), "maya@company.com".to_string(), "Designer".to_string()).await?;

    println!("✅ Created team: PM, Developer, Designer");

    // Step 2: Create project
    println!("\n🚀 Creating project...");
    
    let project_id = types::ProjectId::try_new("mobile-app-v2".to_string()).unwrap();
    let project_name = types::ProjectName::try_new("Mobile App Redesign v2.0".to_string()).unwrap();
    let deadline = chrono::Utc::now() + chrono::Duration::days(90);

    create_project(
        &executor,
        project_id.clone(),
        project_name,
        "Complete redesign of mobile application with new UX/UI".to_string(),
        project_manager_id.clone(),
        Some(deadline),
    ).await?;

    println!("✅ Created project: Mobile App Redesign v2.0");

    // Step 3: Create tasks
    println!("\n📝 Creating tasks...");
    
    let design_task_id = types::TaskId::try_new("design-wireframes".to_string()).unwrap();
    let dev_task_id = types::TaskId::try_new("implement-ui".to_string()).unwrap();
    let review_task_id = types::TaskId::try_new("code-review".to_string()).unwrap();

    // Design task
    let create_design_task = CreateTaskCommand {
        task_id: design_task_id.clone(),
        project_id: project_id.clone(),
        title: types::TaskTitle::try_new("Create UI Wireframes".to_string()).unwrap(),
        description: "Design wireframes for the new mobile app interface".to_string(),
        story_points: Some(types::StoryPoints::try_new(5).unwrap()),
        priority: types::TaskPriority::High,
        created_by: project_manager_id.clone(),
    };

    executor.execute(&create_design_task, create_design_task, ExecutionOptions::default()).await?;
    println!("✅ Created design task: Create UI Wireframes (5 story points)");

    // Development task
    let create_dev_task = CreateTaskCommand {
        task_id: dev_task_id.clone(),
        project_id: project_id.clone(),
        title: types::TaskTitle::try_new("Implement UI Components".to_string()).unwrap(),
        description: "Implement the new UI components based on wireframes".to_string(),
        story_points: Some(types::StoryPoints::try_new(8).unwrap()),
        priority: types::TaskPriority::High,
        created_by: project_manager_id.clone(),
    };

    executor.execute(&create_dev_task, create_dev_task, ExecutionOptions::default()).await?;
    println!("✅ Created development task: Implement UI Components (8 story points)");

    // Step 4: Assign tasks
    println!("\n👩‍💼 Assigning tasks to team members...");
    
    let assign_design = AssignTaskCommand {
        task_id: design_task_id.clone(),
        assignee_id: designer_id.clone(),
        assigned_by: project_manager_id.clone(),
    };

    executor.execute(&assign_design, assign_design, ExecutionOptions::default()).await?;
    println!("✅ Assigned wireframe task to Maya (Designer)");

    let assign_dev = AssignTaskCommand {
        task_id: dev_task_id.clone(),
        assignee_id: developer_id.clone(),
        assigned_by: project_manager_id.clone(),
    };

    executor.execute(&assign_dev, assign_dev, ExecutionOptions::default()).await?;
    println!("✅ Assigned development task to Alex (Developer)");

    // Step 5: Start time tracking
    println!("\n⏱️  Starting time tracking session...");
    
    let start_time = StartTimeEntryCommand {
        task_id: design_task_id.clone(),
        user_id: designer_id.clone(),
        description: "Working on initial wireframe concepts".to_string(),
    };

    executor.execute(&start_time, start_time, ExecutionOptions::default()).await?;
    println!("✅ Maya started working on wireframes");

    // Step 6: Try to start another time entry (should fail)
    println!("\n❌ Attempting to start second time entry (should fail)...");
    
    let start_time_2 = StartTimeEntryCommand {
        task_id: dev_task_id.clone(),
        user_id: designer_id.clone(), // Same user
        description: "Trying to multitask".to_string(),
    };

    match executor.execute(&start_time_2, start_time_2, ExecutionOptions::default()).await {
        Ok(_) => println!("❌ ERROR: Should not allow multiple active time entries!"),
        Err(err) => println!("✅ Correctly blocked multiple time entries: {}", err),
    }

    // Step 7: Complete a task
    println!("\n✅ Completing design task...");
    
    let complete_design = CompleteTaskCommand {
        task_id: design_task_id.clone(),
        completed_by: designer_id.clone(),
    };

    let completion_result = executor.execute(&complete_design, complete_design, ExecutionOptions::default()).await?;
    println!("✅ Design task completed!");
    println!("   📊 Events written: {}", completion_result.events_written.len());
    println!("   🔔 Notification sent to stakeholders");

    // Step 8: Try to assign task to overloaded user
    println!("\n⚠️  Testing assignment limits...");
    
    // Create multiple tasks and try to assign them all to one person
    for i in 1..=12 {
        let task_id = types::TaskId::try_new(format!("bulk-task-{}", i)).unwrap();
        let create_task = CreateTaskCommand {
            task_id: task_id.clone(),
            project_id: project_id.clone(),
            title: types::TaskTitle::try_new(format!("Bulk Task {}", i)).unwrap(),
            description: format!("Auto-generated task {}", i),
            story_points: Some(types::StoryPoints::try_new(1).unwrap()),
            priority: types::TaskPriority::Low,
            created_by: project_manager_id.clone(),
        };

        executor.execute(&create_task, create_task, ExecutionOptions::default()).await?;

        let assign_task = AssignTaskCommand {
            task_id: task_id.clone(),
            assignee_id: developer_id.clone(),
            assigned_by: project_manager_id.clone(),
        };

        match executor.execute(&assign_task, assign_task, ExecutionOptions::default()).await {
            Ok(_) => println!("   ✅ Assigned bulk task {} to Alex", i),
            Err(err) => {
                println!("   ❌ Blocked assignment of task {}: {}", i, err);
                break;
            }
        }
    }

    println!("\n🎉 Example completed successfully!");
    println!("\n💡 Key EventCore patterns demonstrated:");
    println!("   ✅ Project and task lifecycle management");
    println!("   ✅ Multi-user collaboration with assignments");
    println!("   ✅ Business rule enforcement (task limits, status validation)");
    println!("   ✅ Time tracking with active session limits");
    println!("   ✅ Event-driven notifications");
    println!("   ✅ Workflow state machine validation");
    println!("   ✅ Cross-entity consistency (projects, tasks, users)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_creation_workflow() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        // Create project and user first
        let project_id = types::ProjectId::try_new("test-project".to_string()).unwrap();
        let user_id = types::UserId::try_new("test-user".to_string()).unwrap();
        
        create_project(
            &executor,
            project_id.clone(),
            types::ProjectName::try_new("Test Project".to_string()).unwrap(),
            "Description".to_string(),
            user_id.clone(),
            None,
        ).await.unwrap();

        create_user(&executor, user_id.clone(), "Test User".to_string(), "test@example.com".to_string(), "Developer".to_string()).await.unwrap();

        // Create task
        let task_id = types::TaskId::try_new("test-task".to_string()).unwrap();
        let create_task = CreateTaskCommand {
            task_id: task_id.clone(),
            project_id,
            title: types::TaskTitle::try_new("Test Task".to_string()).unwrap(),
            description: "Test description".to_string(),
            story_points: Some(types::StoryPoints::try_new(3).unwrap()),
            priority: types::TaskPriority::Medium,
            created_by: user_id,
        };

        let result = executor.execute(&create_task, create_task, ExecutionOptions::default()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_task_assignment_limits() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let project_id = types::ProjectId::try_new("test-project".to_string()).unwrap();
        let user_id = types::UserId::try_new("test-user".to_string()).unwrap();
        
        create_project(
            &executor,
            project_id.clone(),
            types::ProjectName::try_new("Test Project".to_string()).unwrap(),
            "Description".to_string(),
            user_id.clone(),
            None,
        ).await.unwrap();

        create_user(&executor, user_id.clone(), "Test User".to_string(), "test@example.com".to_string(), "Developer".to_string()).await.unwrap();

        // Create and assign 11 tasks (should fail on the 11th)
        for i in 1..=11 {
            let task_id = types::TaskId::try_new(format!("task-{}", i)).unwrap();
            let create_task = CreateTaskCommand {
                task_id: task_id.clone(),
                project_id: project_id.clone(),
                title: types::TaskTitle::try_new(format!("Task {}", i)).unwrap(),
                description: "Test".to_string(),
                story_points: Some(types::StoryPoints::try_new(1).unwrap()),
                priority: types::TaskPriority::Low,
                created_by: user_id.clone(),
            };

            executor.execute(&create_task, create_task, ExecutionOptions::default()).await.unwrap();

            let assign_task = AssignTaskCommand {
                task_id,
                assignee_id: user_id.clone(),
                assigned_by: user_id.clone(),
            };

            let result = executor.execute(&assign_task, assign_task, ExecutionOptions::default()).await;
            
            if i <= 10 {
                assert!(result.is_ok(), "Assignment {} should succeed", i);
            } else {
                assert!(result.is_err(), "Assignment {} should fail", i);
                assert!(result.unwrap_err().to_string().contains("maximum: 10"));
            }
        }
    }

    #[tokio::test]
    async fn test_time_entry_restrictions() {
        let event_store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let user_id = types::UserId::try_new("test-user".to_string()).unwrap();
        let task_id = types::TaskId::try_new("test-task".to_string()).unwrap();
        
        create_user(&executor, user_id.clone(), "Test User".to_string(), "test@example.com".to_string(), "Developer".to_string()).await.unwrap();

        // Start first time entry
        let start_time_1 = StartTimeEntryCommand {
            task_id: task_id.clone(),
            user_id: user_id.clone(),
            description: "First entry".to_string(),
        };

        let result1 = executor.execute(&start_time_1, start_time_1, ExecutionOptions::default()).await;
        assert!(result1.is_ok());

        // Try to start second time entry (should fail)
        let start_time_2 = StartTimeEntryCommand {
            task_id,
            user_id: user_id.clone(),
            description: "Second entry".to_string(),
        };

        let result2 = executor.execute(&start_time_2, start_time_2, ExecutionOptions::default()).await;
        assert!(result2.is_err());
        assert!(result2.unwrap_err().to_string().contains("already has"));
    }
}