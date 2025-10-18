# Chapter 4.1: Setting Up HTTP Endpoints

EventCore is framework-agnostic - you can use it with any Rust web framework. This chapter shows how to integrate EventCore with popular frameworks and structure your API.

## Architecture Overview

```
HTTP Request → Web Framework → Command/Query → EventCore → Response
```

Your web layer should be thin, focusing on:

1. **Request parsing** - Convert HTTP to domain types
2. **Authentication** - Verify caller identity
3. **Authorization** - Check permissions
4. **Command/Query execution** - Delegate to EventCore
5. **Response formatting** - Convert results to HTTP

## Axum Integration

Axum is a modern web framework that pairs well with EventCore:

### Setup

```toml
[dependencies]
eventcore = "1.0"
axum = "0.7"
tokio = { version = "1", features = ["full"] }
tower = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Basic Application Structure

```rust
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use eventcore::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

// Application state shared across handlers
#[derive(Clone)]
struct AppState {
    executor: CommandExecutor<PostgresEventStore>,
    projections: Arc<RwLock<ProjectionManager>>,
}

#[tokio::main]
async fn main() {
    // Initialize EventCore
    let event_store = PostgresEventStore::new(
        "postgresql://localhost/eventcore"
    ).await.unwrap();

    let executor = CommandExecutor::new(event_store);
    let projections = Arc::new(RwLock::new(ProjectionManager::new()));

    let state = AppState {
        executor,
        projections,
    };

    // Build routes
    let app = Router::new()
        .route("/api/v1/tasks", post(create_task))
        .route("/api/v1/tasks/:id", get(get_task))
        .route("/api/v1/tasks/:id/assign", post(assign_task))
        .route("/api/v1/tasks/:id/complete", post(complete_task))
        .route("/api/v1/users/:id/tasks", get(get_user_tasks))
        .route("/health", get(health_check))
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}
```

### Command Handler Example

```rust
#[derive(Debug, Deserialize)]
struct CreateTaskRequest {
    title: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct CreateTaskResponse {
    task_id: String,
    message: String,
}

async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<CreateTaskResponse>, ApiError> {
    // Validate input
    let title = TaskTitle::try_new(request.title)
        .map_err(|e| ApiError::validation(e))?;
    let description = TaskDescription::try_new(request.description)
        .map_err(|e| ApiError::validation(e))?;

    // Create command
    let task_id = TaskId::new();
    let command = CreateTask {
        task_id: StreamId::from(format!("task-{}", task_id)),
        title,
        description,
    };

    // Execute command
    state.executor
        .execute(&command)
        .await
        .map_err(|e| ApiError::from_command_error(e))?;

    // Return response
    Ok(Json(CreateTaskResponse {
        task_id: task_id.to_string(),
        message: "Task created successfully".to_string(),
    }))
}
```

### Error Handling

```rust
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
    details: Option<serde_json::Value>,
}

impl ApiError {
    fn validation<E: std::error::Error>(error: E) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
            details: None,
        }
    }

    fn from_command_error(error: CommandError) -> Self {
        match error {
            CommandError::ValidationFailed(msg) => Self {
                status: StatusCode::BAD_REQUEST,
                message: msg,
                details: None,
            },
            CommandError::BusinessRuleViolation(msg) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: msg,
                details: None,
            },
            CommandError::StreamNotFound(_) => Self {
                status: StatusCode::NOT_FOUND,
                message: "Resource not found".to_string(),
                details: None,
            },
            CommandError::ConcurrencyConflict(_) => Self {
                status: StatusCode::CONFLICT,
                message: "Resource was modified by another request".to_string(),
                details: None,
            },
            _ => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "An internal error occurred".to_string(),
                details: None,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "error": {
                "message": self.message,
                "details": self.details,
            }
        });

        (self.status, Json(body)).into_response()
    }
}
```

## Actix Web Integration

Actix Web offers high performance and actor-based architecture:

### Setup

```toml
[dependencies]
eventcore = "1.0"
actix-web = "4"
actix-rt = "2"
```

### Application Structure

```rust
use actix_web::{web, App, HttpServer, HttpResponse, Result};
use eventcore::prelude::*;

struct AppData {
    executor: CommandExecutor<PostgresEventStore>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let event_store = PostgresEventStore::new(
        "postgresql://localhost/eventcore"
    ).await.unwrap();

    let app_data = web::Data::new(AppData {
        executor: CommandExecutor::new(event_store),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_data.clone())
            .service(
                web::scope("/api/v1")
                    .route("/tasks", web::post().to(create_task))
                    .route("/tasks/{id}", web::get().to(get_task))
                    .route("/tasks/{id}/assign", web::post().to(assign_task))
            )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn create_task(
    data: web::Data<AppData>,
    request: web::Json<CreateTaskRequest>,
) -> Result<HttpResponse> {
    // Similar to Axum example
    Ok(HttpResponse::Created().json(CreateTaskResponse {
        task_id: "...",
        message: "...",
    }))
}
```

## Rocket Integration

Rocket provides a declarative, type-safe approach:

### Setup

```toml
[dependencies]
eventcore = "1.0"
rocket = { version = "0.5", features = ["json"] }
```

### Application Structure

```rust
use rocket::{State, serde::json::Json};
use eventcore::prelude::*;

struct AppState {
    executor: CommandExecutor<PostgresEventStore>,
}

#[rocket::post("/tasks", data = "<request>")]
async fn create_task(
    state: &State<AppState>,
    request: Json<CreateTaskRequest>,
) -> Result<Json<CreateTaskResponse>, ApiError> {
    // Implementation similar to Axum
}

#[rocket::launch]
fn rocket() -> _ {
    let event_store = /* initialize */;

    rocket::build()
        .manage(AppState {
            executor: CommandExecutor::new(event_store),
        })
        .mount("/api/v1", rocket::routes![
            create_task,
            get_task,
            assign_task,
        ])
}
```

## Request/Response Design

### Command Requests

Design your API requests to map cleanly to commands:

```rust
// HTTP Request
#[derive(Deserialize)]
struct TransferMoneyRequest {
    from_account: String,
    to_account: String,
    amount: Decimal,
    reference: Option<String>,
}

// Convert to command
impl TryFrom<TransferMoneyRequest> for TransferMoney {
    type Error = ValidationError;

    fn try_from(req: TransferMoneyRequest) -> Result<Self, Self::Error> {
        Ok(TransferMoney {
            from_account: StreamId::try_new(req.from_account)?,
            to_account: StreamId::try_new(req.to_account)?,
            amount: Money::try_from_decimal(req.amount)?,
            reference: req.reference.unwrap_or_default(),
        })
    }
}
```

### Response Design

Return minimal, useful information:

```rust
#[derive(Serialize)]
#[serde(tag = "status")]
enum CommandResponse {
    #[serde(rename = "success")]
    Success {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        resource_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resource_url: Option<String>,
    },
    #[serde(rename = "accepted")]
    Accepted {
        message: String,
        tracking_id: String,
    },
}
```

## Middleware and Interceptors

### Request ID Middleware

Track requests through your system:

```rust
use axum::middleware::{self, Next};
use axum::extract::Request;
use uuid::Uuid;

async fn request_id_middleware(
    mut request: Request,
    next: Next,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4().to_string();

    // Add to request extensions
    request.extensions_mut().insert(RequestId(request_id.clone()));

    // Add to response headers
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "X-Request-ID",
        request_id.parse().unwrap(),
    );

    response
}

// Use in router
let app = Router::new()
    .route("/api/v1/tasks", post(create_task))
    .layer(middleware::from_fn(request_id_middleware));
```

### Timing Middleware

Monitor performance:

```rust
use std::time::Instant;

async fn timing_middleware(
    request: Request,
    next: Next,
) -> impl IntoResponse {
    let start = Instant::now();
    let path = request.uri().path().to_owned();
    let method = request.method().clone();

    let response = next.run(request).await;

    let duration = start.elapsed();
    tracing::info!(
        method = %method,
        path = %path,
        duration_ms = %duration.as_millis(),
        status = %response.status(),
        "Request completed"
    );

    response
}
```

## Configuration

Use environment variables for configuration:

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_port")]
    port: u16,

    #[serde(default = "default_host")]
    host: String,

    database_url: String,

    #[serde(default = "default_max_connections")]
    max_connections: u32,
}

fn default_port() -> u16 { 3000 }
fn default_host() -> String { "0.0.0.0".to_string() }
fn default_max_connections() -> u32 { 20 }

impl Config {
    fn from_env() -> Result<Self, config::ConfigError> {
        let mut cfg = config::Config::default();

        // Load from environment
        cfg.merge(config::Environment::default())?;

        // Load from file if exists
        if std::path::Path::new("config.toml").exists() {
            cfg.merge(config::File::with_name("config"))?;
        }

        cfg.try_into()
    }
}
```

## Health Checks

Expose system health:

```rust
#[derive(Serialize)]
struct HealthResponse {
    status: HealthStatus,
    version: &'static str,
    checks: HashMap<String, CheckResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let mut checks = HashMap::new();

    // Check event store
    match state.executor.event_store().health_check().await {
        Ok(_) => checks.insert("event_store".to_string(), CheckResult::healthy()),
        Err(e) => checks.insert("event_store".to_string(), CheckResult::unhealthy(e)),
    };

    // Check projections
    let projections = state.projections.read().await;
    for (name, health) in projections.health_status() {
        checks.insert(format!("projection_{}", name), health);
    }

    // Overall status
    let status = if checks.values().all(|c| c.is_healthy()) {
        HealthStatus::Healthy
    } else if checks.values().any(|c| c.is_unhealthy()) {
        HealthStatus::Unhealthy
    } else {
        HealthStatus::Degraded
    };

    Json(HealthResponse {
        status,
        version: env!("CARGO_PKG_VERSION"),
        checks,
    })
}
```

## Graceful Shutdown

Handle shutdown gracefully:

```rust
use tokio::signal;

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// In main
let app = /* build app */;

axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
```

## Testing HTTP Endpoints

Test your API endpoints:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_create_task_success() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{
                        "title": "Test Task",
                        "description": "Test Description"
                    }"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body: CreateTaskResponse = serde_json::from_slice(
            &hyper::body::to_bytes(response.into_body()).await.unwrap()
        ).unwrap();

        assert!(!body.task_id.is_empty());
    }

    async fn create_test_app() -> Router {
        let event_store = InMemoryEventStore::new();
        let state = AppState {
            executor: CommandExecutor::new(event_store),
            projections: Arc::new(RwLock::new(ProjectionManager::new())),
        };

        Router::new()
            .route("/api/v1/tasks", post(create_task))
            .with_state(state)
    }
}
```

## Best Practices

1. **Keep handlers thin** - Delegate business logic to commands
2. **Use proper status codes** - 201 for creation, 202 for accepted, etc.
3. **Version your API** - Use URL versioning (/api/v1/)
4. **Document with OpenAPI** - Generate from code when possible
5. **Use correlation IDs** - Track requests across services
6. **Log appropriately** - Info for requests, error for failures
7. **Handle errors gracefully** - Never expose internal details

## Summary

Setting up HTTP endpoints for EventCore:

- ✅ **Framework agnostic** - Works with any Rust web framework
- ✅ **Thin HTTP layer** - Focus on translation, not business logic
- ✅ **Type-safe** - Leverage Rust's type system
- ✅ **Error handling** - Map domain errors to HTTP responses
- ✅ **Testable** - Easy to test endpoints in isolation

Key patterns:

1. Parse and validate requests early
2. Convert to domain commands
3. Execute with EventCore
4. Map results to HTTP responses
5. Handle errors appropriately

Next, let's explore [Command Handlers](./02-command-handlers.md) →
