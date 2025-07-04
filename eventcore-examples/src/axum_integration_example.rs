//! # Axum Web Framework Integration Example
#![allow(missing_docs)]
//!
//! This example demonstrates how to integrate `EventCore` with the Axum web framework,
//! providing a `RESTful` API for a simple task management system using event sourcing.
//!
//! ## Key Integration Points:
//!
//! 1. **Shared State**: `EventCore` components (`EventStore`, `CommandExecutor`) are wrapped
//!    in Arc and passed to Axum handlers via application state
//! 2. **Command Pattern**: HTTP requests are converted to `EventCore` commands
//! 3. **Projections**: Read models are updated in real-time and served via API endpoints
//! 4. **Error Handling**: `EventCore` errors are mapped to appropriate HTTP responses
//!
//! ## Running the Example:
//!
//! ```bash
//! # Start PostgreSQL if using postgres backend
//! docker-compose up -d
//!
//! # Run the example
//! cargo run --example axum_integration
//! ```
//!
//! Then interact with the API:
//! ```bash
//! # Create a task
//! curl -X POST http://localhost:3000/tasks \
//!   -H "Content-Type: application/json" \
//!   -d '{"title": "Learn EventCore", "description": "Build amazing event-sourced systems"}'
//!
//! # List all tasks
//! curl http://localhost:3000/tasks
//!
//! # Complete a task
//! curl -X PUT http://localhost:3000/tasks/{task_id}/complete
//! ```

use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, put},
    Router,
};
use eventcore::{
    CommandError, CommandExecutor, CommandLogic, EventStore, ReadOptions, ReadStreams, StoredEvent,
    StreamId, StreamResolver, StreamWrite,
};
use eventcore_macros::Command;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// Domain Types - Using nutype for validation at API boundaries
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 200),
    derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AsRef)
)]
pub struct TaskTitle(String);

#[nutype(
    sanitize(trim),
    validate(len_char_max = 1000),
    derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AsRef)
)]
pub struct TaskDescription(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
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

// Events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TaskEvent {
    Created {
        task_id: TaskId,
        title: TaskTitle,
        description: TaskDescription,
    },
    Completed {
        task_id: TaskId,
    },
    Updated {
        task_id: TaskId,
        title: TaskTitle,
        description: TaskDescription,
    },
}

// Required for EventCore's executor
impl TryFrom<&Self> for TaskEvent {
    type Error = std::convert::Infallible;

    fn try_from(value: &Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// Required for type conversion with serde_json::Value
impl TryFrom<&serde_json::Value> for TaskEvent {
    type Error = serde_json::Error;

    fn try_from(value: &serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone())
    }
}

impl From<TaskEvent> for serde_json::Value {
    fn from(event: TaskEvent) -> Self {
        serde_json::to_value(event).expect("TaskEvent serialization should not fail")
    }
}

// Commands
#[derive(Debug, Clone, Command)]
#[stream("tasks")]
pub struct CreateTask {
    pub task_id: TaskId,
    pub title: TaskTitle,
    pub description: TaskDescription,
}

#[derive(Debug, Default)]
pub struct CreateTaskState;

#[async_trait]
impl CommandLogic for CreateTask {
    type State = CreateTaskState;
    type Event = TaskEvent;

    fn apply(&self, _state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
        // No state needed for creation
    }

    async fn handle(
        &self,
        _read_streams: ReadStreams<Self::StreamSet>,
        _state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        let event = TaskEvent::Created {
            task_id: self.task_id,
            title: self.title.clone(),
            description: self.description.clone(),
        };

        Ok(vec![StreamWrite::new(
            &_read_streams,
            StreamId::from_static("tasks"),
            event,
        )?])
    }
}

#[derive(Debug, Clone, Command)]
pub struct CompleteTask {
    pub task_id: TaskId,
    #[stream]
    pub task_stream: StreamId,
}

#[derive(Debug, Default)]
pub struct CompleteTaskState {
    exists: bool,
    completed: bool,
}

#[async_trait]
impl CommandLogic for CompleteTask {
    type State = CompleteTaskState;
    type Event = TaskEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            TaskEvent::Created { .. } => state.exists = true,
            TaskEvent::Completed { .. } => state.completed = true,
            TaskEvent::Updated { .. } => {}
        }
    }

    async fn handle(
        &self,
        _read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> Result<Vec<StreamWrite<Self::StreamSet, Self::Event>>, CommandError> {
        if !state.exists {
            return Err(CommandError::ValidationFailed("Task not found".to_string()));
        }

        if state.completed {
            return Err(CommandError::ValidationFailed(
                "Task already completed".to_string(),
            ));
        }

        let event = TaskEvent::Completed {
            task_id: self.task_id,
        };

        Ok(vec![StreamWrite::new(
            &_read_streams,
            self.task_stream.clone(),
            event,
        )?])
    }
}

// Read Model / Projection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReadModel {
    pub id: TaskId,
    pub title: TaskTitle,
    pub description: TaskDescription,
    pub completed: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

// In-memory projection store for simplicity
#[derive(Debug, Default)]
pub struct TaskProjection {
    tasks: std::collections::HashMap<TaskId, TaskReadModel>,
}

impl TaskProjection {
    pub fn apply_event(&mut self, event: &StoredEvent<TaskEvent>) {
        match &event.payload {
            TaskEvent::Created {
                task_id,
                title,
                description,
            } => {
                self.tasks.insert(
                    *task_id,
                    TaskReadModel {
                        id: *task_id,
                        title: title.clone(),
                        description: description.clone(),
                        completed: false,
                        created_at: event.timestamp.into(),
                        completed_at: None,
                    },
                );
            }
            TaskEvent::Completed { task_id } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.completed = true;
                    task.completed_at = Some(event.timestamp.into());
                }
            }
            TaskEvent::Updated {
                task_id,
                title,
                description,
            } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.title = title.clone();
                    task.description = description.clone();
                }
            }
        }
    }

    pub fn get_all(&self) -> Vec<TaskReadModel> {
        self.tasks.values().cloned().collect()
    }

    pub fn get(&self, id: &TaskId) -> Option<TaskReadModel> {
        self.tasks.get(id).cloned()
    }
}

// Axum Application State
#[derive(Clone)]
pub struct AppState {
    event_store: Arc<eventcore_memory::InMemoryEventStore<serde_json::Value>>,
    executor: Arc<CommandExecutor<eventcore_memory::InMemoryEventStore<serde_json::Value>>>,
    projection: Arc<RwLock<TaskProjection>>,
}

// API Request/Response Types
#[derive(Debug, Deserialize)]
#[allow(missing_docs)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct CreateTaskResponse {
    pub id: TaskId,
}

#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct ErrorResponse {
    pub error: String,
}

// Axum Handlers
#[allow(missing_docs)]
pub async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<CreateTaskResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Parse and validate input at API boundary
    let title = TaskTitle::try_new(request.title).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let description = TaskDescription::try_new(request.description).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let task_id = TaskId::new();
    let command = CreateTask {
        task_id,
        title,
        description,
    };

    // Execute command
    match state
        .executor
        .execute(command, eventcore::ExecutionOptions::default())
        .await
    {
        Ok(_versions) => {
            // Read the newly created event to update projection
            let stream_data = state
                .event_store
                .read_streams(&[StreamId::from_static("tasks")], &ReadOptions::default())
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: e.to_string(),
                        }),
                    )
                })?;

            // Update projection with the new event
            if let Some(last_event) = stream_data.events.last() {
                if let Ok(task_event) =
                    serde_json::from_value::<TaskEvent>(last_event.payload.clone())
                {
                    let stored_event = StoredEvent {
                        event_id: last_event.event_id,
                        stream_id: last_event.stream_id.clone(),
                        event_version: last_event.event_version,
                        timestamp: last_event.timestamp,
                        payload: task_event,
                        metadata: last_event.metadata.clone(),
                    };
                    state.projection.write().await.apply_event(&stored_event);
                }
            }

            Ok((
                StatusCode::CREATED,
                Json(CreateTaskResponse { id: task_id }),
            ))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

#[allow(missing_docs)]
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<TaskReadModel>> {
    let projection = state.projection.read().await;
    Json(projection.get_all())
}

#[allow(missing_docs)]
pub async fn get_task(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskReadModel>, StatusCode> {
    let projection = state.projection.read().await;
    projection
        .get(&TaskId(task_id))
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[allow(missing_docs)]
pub async fn complete_task(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let task_id = TaskId(task_id);
    let task_stream = StreamId::from_static("tasks");

    let command = CompleteTask {
        task_id,
        task_stream: task_stream.clone(),
    };

    match state
        .executor
        .execute(command, eventcore::ExecutionOptions::default())
        .await
    {
        Ok(_versions) => {
            // Read the task stream to get the latest event
            let stream_data = state
                .event_store
                .read_streams(&[task_stream], &ReadOptions::default())
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: e.to_string(),
                        }),
                    )
                })?;

            // Update projection with any new events
            {
                let mut projection = state.projection.write().await;
                for event in stream_data.events {
                    if let Ok(task_event) =
                        serde_json::from_value::<TaskEvent>(event.payload.clone())
                    {
                        let stored_event = StoredEvent {
                            event_id: event.event_id,
                            stream_id: event.stream_id.clone(),
                            event_version: event.event_version,
                            timestamp: event.timestamp,
                            payload: task_event,
                            metadata: event.metadata.clone(),
                        };
                        projection.apply_event(&stored_event);
                    }
                }
            }

            Ok(StatusCode::NO_CONTENT)
        }
        Err(CommandError::ValidationFailed(msg)) => {
            Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

// Application factory
#[allow(missing_docs)]
pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/tasks", post(create_task).get(list_tasks))
        .route("/tasks/:id", get(get_task))
        .route("/tasks/:id/complete", put(complete_task))
        .with_state(state)
}

// Rebuild projection from event store
#[allow(missing_docs)]
pub async fn rebuild_projection(
    event_store: &eventcore_memory::InMemoryEventStore<serde_json::Value>,
) -> Result<TaskProjection, Box<dyn std::error::Error>> {
    let mut projection = TaskProjection::default();
    let stream_id = StreamId::from_static("tasks");

    // Read all events from the tasks stream
    let stream_data = event_store
        .read_streams(&[stream_id], &ReadOptions::default())
        .await?;

    // Apply each event to rebuild the projection
    for event in stream_data.events {
        if let Ok(task_event) = serde_json::from_value::<TaskEvent>(event.payload.clone()) {
            let stored_event = StoredEvent {
                event_id: event.event_id,
                stream_id: event.stream_id.clone(),
                event_version: event.event_version,
                timestamp: event.timestamp,
                payload: task_event,
                metadata: event.metadata.clone(),
            };
            projection.apply_event(&stored_event);
        }
    }

    Ok(projection)
}

// Example main function
// Note: This example is provided for documentation purposes.
// In a real application, you would need to properly handle the type conversions
// between your domain events and the event store's event type.
//
// The key challenge is that EventCore requires proper type conversions between
// your domain events (TaskEvent) and the event store's generic event type.
//
// For a complete working example, consider:
// 1. Creating a unified event enum that includes all your domain events
// 2. Using that enum as the event store's type parameter
// 3. Implementing proper From/TryFrom conversions
//
// See the banking and e-commerce examples in the eventcore-examples directory
// for complete, working implementations.

fn main() {
    println!("This example demonstrates EventCore integration with Axum.");
    println!("See the source code for the complete implementation.");
    println!("For a working example, refer to other examples in eventcore-examples.");
}
