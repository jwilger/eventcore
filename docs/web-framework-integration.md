# Web Framework Integration Guide

This guide demonstrates how to integrate EventCore with popular Rust web frameworks to build event-sourced web applications.

## Overview

EventCore is designed to work seamlessly with any Rust web framework. The key integration points are:

1. **Shared State**: EventCore components (EventStore, CommandExecutor) are thread-safe and can be shared across request handlers
2. **Command Pattern**: HTTP requests are converted to EventCore commands for processing
3. **Projections**: Read models are maintained and served through API endpoints
4. **Error Handling**: EventCore errors are mapped to appropriate HTTP responses

## Axum Integration

[Axum](https://github.com/tokio-rs/axum) is a modern, ergonomic web framework built on top of Tower and Hyper.

### Example: Task Management API

See the complete example at [`eventcore-examples/src/axum_integration_example.rs`](../eventcore-examples/src/axum_integration_example.rs).

#### Key Integration Points

```rust
// 1. Application State
#[derive(Clone)]
pub struct AppState<ES: EventStore> {
    event_store: Arc<ES>,
    executor: Arc<CommandExecutor<ES>>,
    projection: Arc<RwLock<TaskProjection>>,
}

// 2. HTTP Handler
pub async fn create_task<ES: EventStore>(
    State(state): State<AppState<ES>>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<CreateTaskResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Parse and validate input at API boundary
    let title = TaskTitle::try_new(request.title)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e.to_string() })))?;
    
    // Create and execute command
    let command = CreateTask { title, /* ... */ };
    
    match state.executor.execute(command).await {
        Ok(events) => {
            // Update projection
            let mut projection = state.projection.write().await;
            for event in events {
                projection.apply_event(&event);
            }
            Ok((StatusCode::CREATED, Json(response)))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(error)))
    }
}

// 3. Router Configuration
pub fn create_app<ES: EventStore>(state: AppState<ES>) -> Router {
    Router::new()
        .route("/tasks", post(create_task::<ES>).get(list_tasks::<ES>))
        .route("/tasks/:id", get(get_task::<ES>))
        .route("/tasks/:id/complete", put(complete_task::<ES>))
        .with_state(state)
}
```

### Running the Example

```bash
# Start dependencies
docker-compose up -d

# Run the example
cargo run --example axum_integration

# In another terminal, interact with the API:
# Create a task
curl -X POST http://localhost:3000/tasks \
  -H "Content-Type: application/json" \
  -d '{"title": "Learn EventCore", "description": "Build event-sourced systems"}'

# List tasks
curl http://localhost:3000/tasks

# Complete a task
curl -X PUT http://localhost:3000/tasks/{task_id}/complete
```

## Common Integration Patterns

### 1. Command Validation

Always validate input at the API boundary using EventCore's type-driven approach:

```rust
// Use nutype for validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 200),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct TaskTitle(String);

// In handler
let title = TaskTitle::try_new(request.title)
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e.to_string() })))?;
```

### 2. Error Mapping

Map EventCore errors to appropriate HTTP status codes:

```rust
match state.executor.execute(command).await {
    Ok(events) => Ok((StatusCode::CREATED, Json(response))),
    Err(CommandError::ValidationFailed(msg)) => {
        Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })))
    }
    Err(CommandError::Unauthorized(msg)) => {
        Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: msg })))
    }
    Err(CommandError::StreamNotFound(_)) => {
        Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Resource not found".into() })))
    }
    Err(e) => {
        Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))
    }
}
```

### 3. Projection Management

Options for managing projections in web applications:

#### Option 1: In-Memory Projections (Simple)
```rust
// Suitable for single-instance applications
pub struct AppState<ES: EventStore> {
    projection: Arc<RwLock<TaskProjection>>,
}

// Update after each command
let mut projection = state.projection.write().await;
for event in events {
    projection.apply_event(&event);
}
```

#### Option 2: Background Subscription (Scalable)
```rust
// Run projection updates in background
tokio::spawn(async move {
    let mut subscription = event_store.subscribe(position).await?;
    while let Some(event) = subscription.next().await {
        projection_store.update(&event).await?;
    }
});

// Read from projection store in handlers
let tasks = projection_store.query_tasks(filter).await?;
```

#### Option 3: CQRS with Separate Read Model
```rust
// Use EventCore's CQRS support
use eventcore::cqrs::{ProjectionRunner, ReadModelStore};

// Configure projection runner on startup
let runner = ProjectionRunner::new(event_store, read_model_store);
runner.start(vec![task_projection]).await?;

// Query read models in handlers
let tasks = read_model_store.query("tasks", filter).await?;
```

### 4. Authentication & Authorization

Integrate authentication with commands:

```rust
#[derive(Command)]
pub struct CreateTask {
    pub user_id: UserId,  // From auth middleware
    pub title: TaskTitle,
    // ...
}

// In handler
pub async fn create_task(
    auth: AuthUser,  // Extracted by middleware
    State(state): State<AppState<ES>>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse> {
    let command = CreateTask {
        user_id: auth.id,
        title: TaskTitle::try_new(request.title)?,
        // ...
    };
    // ...
}
```

### 5. Pagination and Filtering

For read endpoints, implement efficient pagination:

```rust
pub async fn list_tasks(
    State(state): State<AppState<ES>>,
    Query(params): Query<ListParams>,
) -> Result<Json<PaginatedResponse<TaskReadModel>>> {
    let projection = state.projection.read().await;
    
    let total = projection.count_matching(&params.filter);
    let items = projection.query(
        &params.filter,
        params.offset,
        params.limit,
        &params.sort,
    );
    
    Ok(Json(PaginatedResponse {
        items,
        total,
        offset: params.offset,
        limit: params.limit,
    }))
}
```

## Best Practices

### 1. Separation of Concerns

- **Commands**: Handle business logic and validation
- **Handlers**: Deal with HTTP concerns (parsing, status codes, serialization)
- **Projections**: Maintain read models optimized for queries

### 2. Idempotency

Use EventCore's event IDs for idempotent operations:

```rust
pub struct CreateTaskRequest {
    pub idempotency_key: Option<Uuid>,
    pub title: String,
    pub description: String,
}

// In command
let event_id = request.idempotency_key
    .map(|key| EventId::from_uuid(key))
    .unwrap_or_else(|| EventId::new());
```

### 3. Performance Optimization

- Use connection pooling for the event store
- Consider caching frequently accessed projections
- Implement pagination for list endpoints
- Use EventCore's batch operations where applicable

### 4. Error Handling

- Log errors with appropriate context
- Return user-friendly error messages
- Don't expose internal details in production
- Use correlation IDs for request tracing

### 5. Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use eventcore_memory::MemoryEventStore;

    #[tokio::test]
    async fn test_create_task_api() {
        // Setup test server
        let event_store = Arc::new(MemoryEventStore::new());
        let state = create_test_state(event_store);
        let app = create_app(state);
        let server = TestServer::new(app).unwrap();
        
        // Test create task
        let response = server
            .post("/tasks")
            .json(&CreateTaskRequest {
                title: "Test Task".into(),
                description: "Test Description".into(),
            })
            .await;
            
        assert_eq!(response.status_code(), StatusCode::CREATED);
        
        // Verify response
        let body: CreateTaskResponse = response.json();
        assert!(!body.id.to_string().is_empty());
    }
}
```

## Integration with Other Frameworks

### Actix-web

Similar patterns apply to Actix-web with minor differences in handler signatures and state management:

```rust
use actix_web::{web, App, HttpResponse, HttpServer};

async fn create_task(
    state: web::Data<AppState<impl EventStore>>,
    request: web::Json<CreateTaskRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    // Similar implementation...
}

HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(app_state.clone()))
        .route("/tasks", web::post().to(create_task))
})
```

### Rocket

Rocket uses request guards for state and validation:

```rust
#[post("/tasks", data = "<request>")]
async fn create_task(
    state: &State<AppState<impl EventStore>>,
    request: Json<CreateTaskRequest>,
) -> Result<Created<Json<CreateTaskResponse>>, Status> {
    // Similar implementation...
}
```

## Advanced Topics

### WebSocket Support

For real-time updates, combine EventCore subscriptions with WebSockets:

```rust
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState<ES>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState<ES>) {
    let subscription = state.event_store.subscribe_all(None).await.unwrap();
    
    // Forward events to WebSocket
    while let Some(event) = subscription.next().await {
        let msg = serde_json::to_string(&event).unwrap();
        socket.send(Message::Text(msg)).await.ok();
    }
}
```

### GraphQL Integration

EventCore works well with GraphQL servers like async-graphql:

```rust
use async_graphql::{Context, Object, Result};

#[Object]
impl TaskQuery {
    async fn tasks(&self, ctx: &Context<'_>) -> Result<Vec<Task>> {
        let state = ctx.data::<AppState<ES>>()?;
        let projection = state.projection.read().await;
        Ok(projection.get_all())
    }
}

#[Object]
impl TaskMutation {
    async fn create_task(
        &self,
        ctx: &Context<'_>,
        title: String,
        description: String,
    ) -> Result<Task> {
        let state = ctx.data::<AppState<ES>>()?;
        // Execute command...
    }
}
```

## Deployment Considerations

### Health Checks

Implement health endpoints that verify EventCore components:

```rust
async fn health_check(State(state): State<AppState<ES>>) -> impl IntoResponse {
    // Check event store connectivity
    match state.event_store.health_check().await {
        Ok(_) => (StatusCode::OK, "Healthy"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "Unhealthy"),
    }
}
```

### Metrics

Expose EventCore metrics through your web framework:

```rust
use prometheus::{Encoder, TextEncoder};

async fn metrics() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    
    Response::builder()
        .header("Content-Type", encoder.format_type())
        .body(buffer)
        .unwrap()
}
```

## Conclusion

EventCore's design makes it straightforward to integrate with any Rust web framework. The key is to:

1. Use EventCore's type-driven approach for domain modeling
2. Convert HTTP requests to commands at the boundary
3. Maintain projections for efficient queries
4. Handle errors appropriately for your API consumers

For complete working examples, see the `eventcore-examples` directory in the EventCore repository.