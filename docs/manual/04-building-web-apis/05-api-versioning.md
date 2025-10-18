# Chapter 4.5: API Versioning

APIs evolve over time. This chapter covers strategies for versioning your EventCore APIs while maintaining backward compatibility and providing a smooth migration path for clients.

## Versioning Strategies

### URL Path Versioning

The most explicit and commonly used approach:

```rust
use axum::{Router, routing::post};

fn create_versioned_routes() -> Router {
    Router::new()
        // Version 1 endpoints
        .nest("/api/v1", v1_routes())
        // Version 2 endpoints
        .nest("/api/v2", v2_routes())
        // Latest version alias (optional)
        .nest("/api/latest", v2_routes())
}

fn v1_routes() -> Router {
    Router::new()
        .route("/tasks", post(v1::create_task))
        .route("/tasks/:id", get(v1::get_task))
        .route("/tasks/:id/assign", post(v1::assign_task))
}

fn v2_routes() -> Router {
    Router::new()
        .route("/tasks", post(v2::create_task))
        .route("/tasks/:id", get(v2::get_task))
        .route("/tasks/:id/assign", post(v2::assign_task))
        // New in v2
        .route("/tasks/:id/subtasks", get(v2::get_subtasks))
        .route("/tasks/bulk", post(v2::bulk_create_tasks))
}
```

### Header-Based Versioning

More RESTful but less discoverable:

```rust
use axum::{
    extract::{FromRequestParts, Request},
    http::HeaderValue,
};

#[derive(Debug, Clone, Copy)]
enum ApiVersion {
    V1,
    V2,
}

impl Default for ApiVersion {
    fn default() -> Self {
        ApiVersion::V2 // Latest version
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for ApiVersion
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let version = parts
            .headers
            .get("API-Version")
            .and_then(|v| v.to_str().ok())
            .map(|v| match v {
                "1" | "v1" => ApiVersion::V1,
                "2" | "v2" => ApiVersion::V2,
                _ => ApiVersion::default(),
            })
            .unwrap_or_default();

        Ok(version)
    }
}

// Use in handlers
async fn create_task(
    version: ApiVersion,
    Json(request): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    match version {
        ApiVersion::V1 => v1::create_task_handler(request).await,
        ApiVersion::V2 => v2::create_task_handler(request).await,
    }
}
```

### Content Type Versioning

Using vendor-specific media types:

```rust
#[derive(Debug, Clone)]
enum ContentVersion {
    V1,
    V2,
}

impl ContentVersion {
    fn from_content_type(content_type: &str) -> Self {
        if content_type.contains("vnd.eventcore.v1+json") {
            ContentVersion::V1
        } else if content_type.contains("vnd.eventcore.v2+json") {
            ContentVersion::V2
        } else {
            ContentVersion::V2 // Default to latest
        }
    }

    fn to_content_type(&self) -> &'static str {
        match self {
            ContentVersion::V1 => "application/vnd.eventcore.v1+json",
            ContentVersion::V2 => "application/vnd.eventcore.v2+json",
        }
    }
}
```

## Request/Response Evolution

### Backward Compatible Changes

These changes don't require a new version:

```rust
// Original V1 request
#[derive(Debug, Deserialize)]
struct CreateTaskRequestV1 {
    title: String,
    description: String,
}

// Backward compatible V1 with optional field
#[derive(Debug, Deserialize)]
struct CreateTaskRequestV1Enhanced {
    title: String,
    description: String,
    #[serde(default)]
    priority: Option<Priority>, // New optional field
}

// Response expansion is also backward compatible
#[derive(Debug, Serialize)]
struct TaskResponseV1 {
    id: String,
    title: String,
    description: String,
    created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<Priority>, // New optional field
}
```

### Breaking Changes

These require a new API version:

```rust
mod v1 {
    #[derive(Debug, Deserialize)]
    struct CreateTaskRequest {
        title: String,
        description: String,
        assigned_to: String, // Single assignee
    }
}

mod v2 {
    #[derive(Debug, Deserialize)]
    struct CreateTaskRequest {
        title: String,
        description: String,
        assigned_to: Vec<String>, // Breaking: Now multiple assignees
        #[serde(default)]
        tags: Vec<String>, // New field
    }
}

// Adapter to support both versions
async fn create_task_adapter(
    version: ApiVersion,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<TaskResponse>, ApiError> {
    match version {
        ApiVersion::V1 => {
            let request: v1::CreateTaskRequest = serde_json::from_value(value)?;
            // Convert V1 to internal command
            let command = CreateTask {
                title: request.title,
                description: request.description,
                assigned_to: vec![request.assigned_to], // Adapt single to vec
                tags: vec![], // Default for V1
            };
            execute_create_task(command).await
        }
        ApiVersion::V2 => {
            let request: v2::CreateTaskRequest = serde_json::from_value(value)?;
            let command = CreateTask {
                title: request.title,
                description: request.description,
                assigned_to: request.assigned_to,
                tags: request.tags,
            };
            execute_create_task(command).await
        }
    }
}
```

## Command Versioning

Version commands to handle different API versions:

```rust
// Internal command representation (latest version)
#[derive(Command, Clone)]
struct CreateTask {
    #[stream]
    task_id: StreamId,

    title: TaskTitle,
    description: TaskDescription,
    assigned_to: Vec<UserId>,
    tags: Vec<Tag>,
    priority: Priority,
}

// Version-specific command builders
mod command_builders {
    use super::*;

    pub fn from_v1_request(req: v1::CreateTaskRequest) -> Result<CreateTask, ApiError> {
        Ok(CreateTask {
            task_id: StreamId::from(format!("task-{}", TaskId::new())),
            title: TaskTitle::try_new(req.title)?,
            description: TaskDescription::try_new(req.description)?,
            assigned_to: vec![UserId::try_new(req.assigned_to)?],
            tags: vec![], // V1 doesn't support tags
            priority: Priority::Normal, // Default for V1
        })
    }

    pub fn from_v2_request(req: v2::CreateTaskRequest) -> Result<CreateTask, ApiError> {
        Ok(CreateTask {
            task_id: StreamId::from(format!("task-{}", TaskId::new())),
            title: TaskTitle::try_new(req.title)?,
            description: TaskDescription::try_new(req.description)?,
            assigned_to: req.assigned_to
                .into_iter()
                .map(|a| UserId::try_new(a))
                .collect::<Result<Vec<_>, _>>()?,
            tags: req.tags
                .into_iter()
                .map(|t| Tag::try_new(t))
                .collect::<Result<Vec<_>, _>>()?,
            priority: req.priority.unwrap_or(Priority::Normal),
        })
    }
}
```

## Response Transformation

Transform internal data to version-specific responses:

```rust
// Internal projection data
#[derive(Debug, Clone)]
struct TaskData {
    id: TaskId,
    title: String,
    description: String,
    assigned_to: Vec<UserId>,
    tags: Vec<Tag>,
    priority: Priority,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    subtasks: Vec<SubtaskData>, // Added in V2
}

// Response transformers
mod response_transformers {
    use super::*;

    pub fn to_v1_response(task: TaskData) -> v1::TaskResponse {
        v1::TaskResponse {
            id: task.id.to_string(),
            title: task.title,
            description: task.description,
            assigned_to: task.assigned_to.first()
                .map(|u| u.to_string())
                .unwrap_or_default(), // V1 only supports single assignee
            created_at: task.created_at,
            updated_at: task.updated_at,
        }
    }

    pub fn to_v2_response(task: TaskData) -> v2::TaskResponse {
        v2::TaskResponse {
            id: task.id.to_string(),
            title: task.title,
            description: task.description,
            assigned_to: task.assigned_to
                .into_iter()
                .map(|u| u.to_string())
                .collect(),
            tags: task.tags
                .into_iter()
                .map(|t| t.to_string())
                .collect(),
            priority: task.priority,
            created_at: task.created_at,
            updated_at: task.updated_at,
            subtask_count: task.subtasks.len(),
            _links: v2::Links {
                self_: format!("/api/v2/tasks/{}", task.id),
                subtasks: format!("/api/v2/tasks/{}/subtasks", task.id),
            },
        }
    }
}
```

## Deprecation Strategy

Communicate deprecation clearly:

```rust
async fn deprecated_middleware(
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    // Add deprecation headers
    response.headers_mut().insert(
        "Sunset",
        HeaderValue::from_static("Sat, 31 Dec 2024 23:59:59 GMT"),
    );

    response.headers_mut().insert(
        "Deprecation",
        HeaderValue::from_static("true"),
    );

    response.headers_mut().insert(
        "Link",
        HeaderValue::from_static(
            "</api/v2/docs>; rel=\"successor-version\""
        ),
    );

    response
}

// Apply to V1 routes
let v1_routes = Router::new()
    .route("/tasks", post(v1::create_task))
    .layer(middleware::from_fn(deprecated_middleware));
```

### Deprecation Notices in Responses

```rust
#[derive(Debug, Serialize)]
struct DeprecatedResponse<T> {
    #[serde(flatten)]
    data: T,
    _deprecation: DeprecationNotice,
}

#[derive(Debug, Serialize)]
struct DeprecationNotice {
    message: &'static str,
    sunset_date: &'static str,
    migration_guide: &'static str,
}

impl<T> DeprecatedResponse<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            _deprecation: DeprecationNotice {
                message: "This API version is deprecated",
                sunset_date: "2024-12-31",
                migration_guide: "https://docs.eventcore.io/migration/v1-to-v2",
            },
        }
    }
}
```

## Version Discovery

Help clients discover available versions:

```rust
#[derive(Debug, Serialize)]
struct ApiVersionInfo {
    version: String,
    status: VersionStatus,
    deprecated: bool,
    sunset_date: Option<String>,
    endpoints: Vec<EndpointInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum VersionStatus {
    Stable,
    Beta,
    Deprecated,
    Sunset,
}

async fn get_api_versions() -> Json<Vec<ApiVersionInfo>> {
    Json(vec![
        ApiVersionInfo {
            version: "v1".to_string(),
            status: VersionStatus::Deprecated,
            deprecated: true,
            sunset_date: Some("2024-12-31".to_string()),
            endpoints: vec![
                EndpointInfo {
                    path: "/api/v1/tasks",
                    methods: vec!["GET", "POST"],
                },
                // ... other endpoints
            ],
        },
        ApiVersionInfo {
            version: "v2".to_string(),
            status: VersionStatus::Stable,
            deprecated: false,
            sunset_date: None,
            endpoints: vec![
                EndpointInfo {
                    path: "/api/v2/tasks",
                    methods: vec!["GET", "POST"],
                },
                EndpointInfo {
                    path: "/api/v2/tasks/bulk",
                    methods: vec!["POST"],
                },
                // ... other endpoints
            ],
        },
    ])
}
```

## Migration Support

Help clients migrate between versions:

```rust
// Migration endpoint that accepts V1 format and returns V2
async fn migrate_task_format(
    Json(v1_task): Json<v1::TaskResponse>,
) -> Result<Json<v2::TaskResponse>, ApiError> {
    // Transform V1 to V2 format
    let v2_task = v2::TaskResponse {
        id: v1_task.id,
        title: v1_task.title,
        description: v1_task.description,
        assigned_to: vec![v1_task.assigned_to], // Convert single to array
        tags: vec![], // Default empty
        priority: Priority::Normal, // Default
        created_at: v1_task.created_at,
        updated_at: v1_task.updated_at,
        subtask_count: 0, // Default
        _links: v2::Links {
            self_: format!("/api/v2/tasks/{}", v1_task.id),
            subtasks: format!("/api/v2/tasks/{}/subtasks", v1_task.id),
        },
    };

    Ok(Json(v2_task))
}

// Bulk migration endpoint
async fn migrate_tasks_bulk(
    Json(request): Json<BulkMigrationRequest>,
) -> Result<Json<BulkMigrationResponse>, ApiError> {
    let mut migrated = Vec::new();
    let mut errors = Vec::new();

    for task_id in request.task_ids {
        match migrate_single_task(&task_id).await {
            Ok(task) => migrated.push(task),
            Err(e) => errors.push(MigrationError {
                task_id,
                error: e.to_string(),
            }),
        }
    }

    Ok(Json(BulkMigrationResponse {
        migrated_count: migrated.len(),
        error_count: errors.len(),
        errors: if errors.is_empty() { None } else { Some(errors) },
    }))
}
```

## Testing Multiple Versions

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_v1_compatibility() {
        let app = create_app();

        // V1 request format
        let v1_request = serde_json::json!({
            "title": "Test Task",
            "description": "Test Description",
            "assigned_to": "user123"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(v1_request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // Verify deprecation headers
        assert_eq!(
            response.headers().get("Deprecation").unwrap(),
            "true"
        );
    }

    #[tokio::test]
    async fn test_v2_enhancements() {
        let app = create_app();

        // V2 request with new features
        let v2_request = serde_json::json!({
            "title": "Test Task",
            "description": "Test Description",
            "assigned_to": ["user123", "user456"],
            "tags": ["urgent", "backend"],
            "priority": "high"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v2/tasks")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(v2_request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body: v2::TaskResponse = serde_json::from_slice(
            &hyper::body::to_bytes(response.into_body()).await.unwrap()
        ).unwrap();

        assert_eq!(body.assigned_to.len(), 2);
        assert_eq!(body.tags.len(), 2);
    }

    #[tokio::test]
    async fn test_version_negotiation() {
        let app = create_app();

        // Test header-based versioning
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tasks/123")
                    .header("API-Version", "v1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return V1 format
        let body: v1::TaskResponse = serde_json::from_slice(
            &hyper::body::to_bytes(response.into_body()).await.unwrap()
        ).unwrap();

        assert!(body.assigned_to.is_string()); // V1 uses string
    }
}
```

## Documentation

Generate version-specific documentation:

```rust
use utoipa::{OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(
    paths(
        v1::create_task,
        v1::get_task,
    ),
    components(
        schemas(v1::CreateTaskRequest, v1::TaskResponse)
    ),
    tags(
        (name = "tasks", description = "Task management API v1")
    ),
    info(
        title = "EventCore API v1",
        version = "1.0.0",
        description = "Legacy API version - deprecated"
    )
)]
struct ApiDocV1;

#[derive(OpenApi)]
#[openapi(
    paths(
        v2::create_task,
        v2::get_task,
        v2::bulk_create_tasks,
    ),
    components(
        schemas(v2::CreateTaskRequest, v2::TaskResponse)
    ),
    tags(
        (name = "tasks", description = "Task management API v2")
    ),
    info(
        title = "EventCore API v2",
        version = "2.0.0",
        description = "Current stable API version"
    )
)]
struct ApiDocV2;

// Serve version-specific docs
async fn serve_api_docs(version: ApiVersion) -> impl IntoResponse {
    match version {
        ApiVersion::V1 => Json(ApiDocV1::openapi()),
        ApiVersion::V2 => Json(ApiDocV2::openapi()),
    }
}
```

## Best Practices

1. **Plan for versioning from day one** - Even if you start with v1
2. **Use semantic versioning** - Major.Minor.Patch
3. **Maintain backward compatibility** - When possible
4. **Communicate changes clearly** - Use headers and documentation
5. **Set deprecation timelines** - Give clients time to migrate
6. **Version at the right level** - Not every change needs a new version
7. **Test all versions** - Maintain test suites for each supported version
8. **Monitor version usage** - Track which versions clients use

## Summary

API versioning in EventCore applications:

- ✅ **Multiple strategies** - URL, header, content-type versioning
- ✅ **Smooth migration** - Tools to help clients upgrade
- ✅ **Clear deprecation** - Sunset dates and migration guides
- ✅ **Version discovery** - Clients can explore available versions
- ✅ **Backward compatibility** - Maintain old versions gracefully

Key patterns:

1. Choose a versioning strategy and stick to it
2. Transform between versions at API boundaries
3. Keep internal representations version-agnostic
4. Communicate deprecation clearly
5. Provide migration tools and guides
6. Test all supported versions

Congratulations! You've completed Part 4. Continue to [Part 5: Advanced Topics](../05-advanced-topics/README.md) →
