# Chapter 4.2: Command Handlers

Command handlers are the bridge between HTTP requests and your EventCore commands. This chapter covers patterns for building robust, maintainable command handlers.

## Command Handler Architecture

```
HTTP Request
    ↓
Parse & Validate
    ↓
Authenticate & Authorize
    ↓
Create Command
    ↓
Execute Command
    ↓
Format Response
```

## Basic Command Handler Pattern

### The Handler Function

```rust
use axum::{
    extract::{State, Path, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use eventcore::prelude::*;

#[derive(Debug, Deserialize)]
struct AssignTaskRequest {
    assignee_id: String,
}

#[derive(Debug, Serialize)]
struct AssignTaskResponse {
    message: String,
    task_id: String,
    assignee_id: String,
    assigned_at: DateTime<Utc>,
}

async fn assign_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    user: AuthenticatedUser,  // From middleware
    Json(request): Json<AssignTaskRequest>,
) -> Result<Json<AssignTaskResponse>, ApiError> {
    // 1. Parse and validate input
    let task_stream = StreamId::try_new(format!("task-{}", task_id))
        .map_err(|e| ApiError::validation("Invalid task ID"))?;

    let assignee_stream = StreamId::try_new(format!("user-{}", request.assignee_id))
        .map_err(|e| ApiError::validation("Invalid assignee ID"))?;

    // 2. Create command
    let command = AssignTask {
        task_id: task_stream,
        assignee_id: assignee_stream,
        assigned_by: user.id.clone(),
    };

    // 3. Execute with context
    let result = state.executor
        .execute_with_context(
            &command,
            ExecutionContext::new()
                .with_user_id(user.id)
                .with_correlation_id(extract_correlation_id(&request))
        )
        .await
        .map_err(ApiError::from_command_error)?;

    // 4. Format response
    Ok(Json(AssignTaskResponse {
        message: "Task assigned successfully".to_string(),
        task_id: task_id.clone(),
        assignee_id: request.assignee_id,
        assigned_at: Utc::now(),
    }))
}
```

## Authentication and Authorization

### Authentication Middleware

```rust
use axum::{
    extract::{Request, FromRequestParts},
    http::{header, StatusCode},
    response::Response,
    middleware::Next,
};
use jsonwebtoken::{decode, DecodingKey, Validation};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claims {
    sub: String,  // User ID
    exp: usize,   // Expiration time
    roles: Vec<String>,
}

#[derive(Debug, Clone)]
struct AuthenticatedUser {
    id: UserId,
    roles: Vec<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Extract token from Authorization header
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|auth| auth.to_str().ok())
            .and_then(|auth| auth.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::unauthorized("Missing authentication token"))?;

        // Decode and validate token
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(JWT_SECRET.as_ref()),
            &Validation::default(),
        )
        .map_err(|_| ApiError::unauthorized("Invalid authentication token"))?;

        Ok(AuthenticatedUser {
            id: UserId::try_new(token_data.claims.sub)?,
            roles: token_data.claims.roles,
        })
    }
}
```

### Authorization in Handlers

```rust
impl AuthenticatedUser {
    fn has_role(&self, role: &str) -> bool {
        self.roles.contains(&role.to_string())
    }

    fn can_manage_tasks(&self) -> bool {
        self.has_role("admin") || self.has_role("manager")
    }

    fn can_assign_tasks(&self) -> bool {
        self.has_role("admin") || self.has_role("manager") || self.has_role("lead")
    }
}

async fn delete_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<StatusCode, ApiError> {
    // Check authorization
    if !user.can_manage_tasks() {
        return Err(ApiError::forbidden("Insufficient permissions to delete tasks"));
    }

    let command = DeleteTask {
        task_id: StreamId::try_new(format!("task-{}", task_id))?,
        deleted_by: user.id,
    };

    state.executor.execute(&command).await?;

    Ok(StatusCode::NO_CONTENT)
}
```

## Input Validation

### Request Validation

```rust
use validator::{Validate, ValidationError};

#[derive(Debug, Deserialize, Validate)]
struct CreateProjectRequest {
    #[validate(length(min = 3, max = 100))]
    name: String,

    #[validate(length(max = 1000))]
    description: Option<String>,

    #[validate(email)]
    owner_email: String,

    #[validate(range(min = 1, max = 365))]
    duration_days: u32,

    #[validate(custom = "validate_start_date")]
    start_date: Option<DateTime<Utc>>,
}

fn validate_start_date(date: &DateTime<Utc>) -> Result<(), ValidationError> {
    if *date < Utc::now() {
        return Err(ValidationError::new("Start date cannot be in the past"));
    }
    Ok(())
}

async fn create_project(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(request): Json<CreateProjectRequest>,
) -> Result<Json<CreateProjectResponse>, ApiError> {
    // Validate request
    request.validate()
        .map_err(|e| ApiError::validation_errors(e))?;

    // Create command with validated data
    let command = CreateProject {
        project_id: StreamId::from(format!("project-{}", ProjectId::new())),
        name: ProjectName::try_new(request.name)?,
        description: request.description
            .map(|d| ProjectDescription::try_new(d))
            .transpose()?,
        owner: UserId::try_new(request.owner_email)?,
        duration: Duration::days(request.duration_days as i64),
        start_date: request.start_date.unwrap_or_else(Utc::now),
        created_by: user.id,
    };

    // Execute and return response
    // ...
}
```

### Custom Validation Rules

```rust
mod validators {
    use super::*;

    pub fn validate_business_hours(time: &NaiveTime) -> Result<(), ValidationError> {
        const BUSINESS_START: NaiveTime = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        const BUSINESS_END: NaiveTime = NaiveTime::from_hms_opt(17, 0, 0).unwrap();

        if *time < BUSINESS_START || *time > BUSINESS_END {
            return Err(ValidationError::new("Outside business hours"));
        }
        Ok(())
    }

    pub fn validate_future_date(date: &NaiveDate) -> Result<(), ValidationError> {
        if *date <= Local::now().naive_local().date() {
            return Err(ValidationError::new("Date must be in the future"));
        }
        Ok(())
    }

    pub fn validate_currency_code(code: &str) -> Result<(), ValidationError> {
        const VALID_CURRENCIES: &[&str] = &["USD", "EUR", "GBP", "JPY"];

        if !VALID_CURRENCIES.contains(&code) {
            return Err(ValidationError::new("Invalid currency code"));
        }
        Ok(())
    }
}
```

## Idempotency

Ensure commands can be safely retried:

### Idempotency Keys

```rust
use axum::extract::FromRequest;

#[derive(Debug, Clone)]
struct IdempotencyKey(String);

#[async_trait]
impl<S> FromRequestParts<S> for IdempotencyKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("Idempotency-Key")
            .and_then(|v| v.to_str().ok())
            .map(|s| IdempotencyKey(s.to_string()))
            .ok_or_else(|| ApiError::bad_request("Idempotency-Key header required"))
    }
}

// Store for idempotency
#[derive(Clone)]
struct IdempotencyStore {
    cache: Arc<RwLock<HashMap<String, CachedResponse>>>,
}

#[derive(Clone)]
struct CachedResponse {
    status: StatusCode,
    body: Vec<u8>,
    created_at: DateTime<Utc>,
}

async fn idempotent_handler<F, Fut>(
    key: IdempotencyKey,
    store: State<IdempotencyStore>,
    handler: F,
) -> Response
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Response>,
{
    // Check cache
    let cache = store.cache.read().await;
    if let Some(cached) = cache.get(&key.0) {
        // Return cached response
        return Response::builder()
            .status(cached.status)
            .body(Body::from(cached.body.clone()))
            .unwrap();
    }
    drop(cache);

    // Execute handler
    let response = handler().await;

    // Cache successful responses
    if response.status().is_success() {
        let (parts, body) = response.into_parts();
        let body_bytes = hyper::body::to_bytes(body).await.unwrap().to_vec();

        let mut cache = store.cache.write().await;
        cache.insert(key.0, CachedResponse {
            status: parts.status,
            body: body_bytes.clone(),
            created_at: Utc::now(),
        });

        Response::from_parts(parts, Body::from(body_bytes))
    } else {
        response
    }
}
```

### Command-Level Idempotency

```rust
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,

    #[stream]
    to_account: StreamId,

    amount: Money,

    // Idempotency key embedded in command
    transfer_id: TransferId,
}

impl CommandLogic for TransferMoney {
    // ... other implementations

    fn handle(&self, mut state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Check if transfer already processed
        if state.processed_transfers.contains(&self.transfer_id) {
            // Already processed - no new events
            return Ok(NewEvents::default());
        }

        state.processed_transfers.insert(self.transfer_id);

        Ok(NewEvents::from(vec![
            BankEvent::TransferProcessed {
                transfer_id: self.transfer_id,
                amount: self.amount,
            },
            // ... other events
        ]))
    }
}
```

## Error Response Formatting

Provide consistent, helpful error responses:

```rust
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetails,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorDetails {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    field_errors: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
}

impl ApiError {
    fn to_response(&self, request_id: Option<String>) -> Response {
        let (status, error_details) = match self {
            ApiError::Validation { errors } => (
                StatusCode::BAD_REQUEST,
                ErrorDetails {
                    code: "VALIDATION_ERROR".to_string(),
                    message: "Invalid request data".to_string(),
                    field_errors: Some(errors.clone()),
                    help: Some("Check the field_errors for specific validation issues".to_string()),
                }
            ),
            ApiError::BusinessRule { message } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorDetails {
                    code: "BUSINESS_RULE_VIOLATION".to_string(),
                    message: message.clone(),
                    field_errors: None,
                    help: None,
                }
            ),
            ApiError::NotFound { resource } => (
                StatusCode::NOT_FOUND,
                ErrorDetails {
                    code: "RESOURCE_NOT_FOUND".to_string(),
                    message: format!("{} not found", resource),
                    field_errors: None,
                    help: None,
                }
            ),
            ApiError::Conflict { message } => (
                StatusCode::CONFLICT,
                ErrorDetails {
                    code: "CONFLICT".to_string(),
                    message: message.clone(),
                    field_errors: None,
                    help: Some("The resource was modified. Please refresh and try again.".to_string()),
                }
            ),
            // ... other error types
        };

        let response = ErrorResponse {
            error: error_details,
            request_id,
        };

        (status, Json(response)).into_response()
    }
}
```

## Batch Command Handlers

Handle multiple commands efficiently:

```rust
#[derive(Debug, Deserialize)]
struct BatchRequest<T> {
    operations: Vec<T>,
    #[serde(default)]
    stop_on_error: bool,
}

#[derive(Debug, Serialize)]
struct BatchResponse<T> {
    results: Vec<BatchResult<T>>,
    successful: usize,
    failed: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status")]
enum BatchResult<T> {
    #[serde(rename = "success")]
    Success { result: T },
    #[serde(rename = "error")]
    Error { error: ErrorDetails },
}

async fn batch_create_tasks(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(batch): Json<BatchRequest<CreateTaskRequest>>,
) -> Result<Json<BatchResponse<CreateTaskResponse>>, ApiError> {
    let mut results = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for request in batch.operations {
        match create_single_task(&state, &user, request).await {
            Ok(response) => {
                successful += 1;
                results.push(BatchResult::Success { result: response });
            }
            Err(error) => {
                failed += 1;
                results.push(BatchResult::Error {
                    error: error.to_error_details()
                });

                if batch.stop_on_error {
                    break;
                }
            }
        }
    }

    Ok(Json(BatchResponse {
        results,
        successful,
        failed,
    }))
}
```

## Async Command Processing

For long-running commands:

```rust
#[derive(Debug, Serialize)]
struct AsyncCommandResponse {
    tracking_id: String,
    status_url: String,
    message: String,
}

async fn import_large_dataset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(request): Json<ImportDatasetRequest>,
) -> Result<Json<AsyncCommandResponse>, ApiError> {
    // Validate request
    request.validate()?;

    // Create tracking ID
    let tracking_id = TrackingId::new();

    // Queue command for async processing
    let command = ImportDataset {
        dataset_id: StreamId::from(format!("dataset-{}", DatasetId::new())),
        source_url: request.source_url,
        import_options: request.options,
        initiated_by: user.id,
        tracking_id: tracking_id.clone(),
    };

    // Submit to background queue
    state.command_queue
        .submit(command)
        .await
        .map_err(|_| ApiError::service_unavailable("Import service temporarily unavailable"))?;

    // Return tracking information
    Ok(Json(AsyncCommandResponse {
        tracking_id: tracking_id.to_string(),
        status_url: format!("/api/v1/imports/{}/status", tracking_id),
        message: "Import queued for processing".to_string(),
    }))
}

// Status endpoint
async fn get_import_status(
    State(state): State<AppState>,
    Path(tracking_id): Path<String>,
) -> Result<Json<ImportStatus>, ApiError> {
    let status = state.import_tracker
        .get_status(&TrackingId::try_new(tracking_id)?)
        .await?
        .ok_or_else(|| ApiError::not_found("Import"))?;

    Ok(Json(status))
}
```

## Command Handler Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_testing::prelude::*;

    #[tokio::test]
    async fn test_assign_task_authorization() {
        let state = create_test_state().await;

        // User without permission
        let user = AuthenticatedUser {
            id: UserId::try_new("user@example.com").unwrap(),
            roles: vec!["member".to_string()],
        };

        let request = AssignTaskRequest {
            assignee_id: "assignee@example.com".to_string(),
        };

        let result = assign_task(
            State(state),
            Path("task-123".to_string()),
            user,
            Json(request),
        ).await;

        assert!(matches!(
            result,
            Err(ApiError::Forbidden { .. })
        ));
    }

    #[tokio::test]
    async fn test_idempotent_transfer() {
        let state = create_test_state().await;
        let transfer_id = TransferId::new();

        let request = TransferMoneyRequest {
            from_account: "account-1".to_string(),
            to_account: "account-2".to_string(),
            amount: 100.0,
            transfer_id: transfer_id.to_string(),
        };

        // First call
        let response1 = transfer_money(
            State(state.clone()),
            Json(request.clone()),
        ).await.unwrap();

        // Second call with same transfer_id
        let response2 = transfer_money(
            State(state),
            Json(request),
        ).await.unwrap();

        // Should return same response
        assert_eq!(response1.0.transfer_id, response2.0.transfer_id);
        assert_eq!(response1.0.status, response2.0.status);
    }
}
```

## Monitoring and Metrics

Track command handler performance:

```rust
use prometheus::{IntCounter, Histogram, register_int_counter, register_histogram};

lazy_static! {
    static ref COMMAND_COUNTER: IntCounter = register_int_counter!(
        "api_commands_total",
        "Total number of commands processed"
    ).unwrap();

    static ref COMMAND_DURATION: Histogram = register_histogram!(
        "api_command_duration_seconds",
        "Command processing duration"
    ).unwrap();

    static ref COMMAND_ERRORS: IntCounter = register_int_counter!(
        "api_command_errors_total",
        "Total number of command errors"
    ).unwrap();
}

async fn measured_handler<F, Fut, T>(
    command_type: &str,
    handler: F,
) -> Result<T, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
{
    COMMAND_COUNTER.inc();
    let timer = COMMAND_DURATION.start_timer();

    let result = handler().await;

    timer.observe_duration();

    if result.is_err() {
        COMMAND_ERRORS.inc();
    }

    // Log with structured data
    match &result {
        Ok(_) => {
            tracing::info!(
                command_type = %command_type,
                duration_ms = %timer.stop_and_record() * 1000.0,
                "Command completed successfully"
            );
        }
        Err(e) => {
            tracing::error!(
                command_type = %command_type,
                error = %e,
                "Command failed"
            );
        }
    }

    result
}
```

## Best Practices

1. **Validate early** - Check inputs before creating commands
2. **Use strong types** - Convert strings to domain types ASAP
3. **Handle all errors** - Map domain errors to appropriate HTTP responses
4. **Be idempotent** - Design for safe retries
5. **Authenticate first** - Verify identity before any processing
6. **Authorize actions** - Check permissions for each operation
7. **Log appropriately** - Include context for debugging
8. **Monitor everything** - Track success rates and latencies

## Summary

Command handlers in EventCore APIs:

- ✅ **Type-safe** - Leverage Rust's type system
- ✅ **Validated** - Check inputs thoroughly
- ✅ **Authenticated** - Know who's making requests
- ✅ **Authorized** - Enforce permissions
- ✅ **Idempotent** - Safe to retry
- ✅ **Monitored** - Track performance and errors

Key patterns:

1. Parse and validate input
2. Check authentication and authorization
3. Create strongly-typed commands
4. Execute with proper context
5. Handle errors gracefully
6. Return appropriate responses

Next, let's explore [Query Endpoints](./03-query-endpoints.md) →
