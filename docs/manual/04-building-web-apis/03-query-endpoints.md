# Chapter 4.3: Query Endpoints

Query endpoints serve read requests from your projections. Unlike commands that modify state, queries are side-effect free and can be cached, making them perfect for high-performance read operations.

## Query Architecture

```
HTTP Request → Authenticate → Authorize → Query Projection → Format Response
                                                ↑
                                         Read Model Store
```

## Basic Query Pattern

### Simple Query Endpoint

```rust
use axum::{
    extract::{State, Path, Query as QueryParams},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ListTasksQuery {
    #[serde(default)]
    status: Option<TaskStatus>,
    
    #[serde(default)]
    assigned_to: Option<String>,
    
    #[serde(default = "default_page")]
    page: u32,
    
    #[serde(default = "default_page_size")]
    page_size: u32,
}

fn default_page() -> u32 { 1 }
fn default_page_size() -> u32 { 20 }

#[derive(Debug, Serialize)]
struct ListTasksResponse {
    tasks: Vec<TaskSummary>,
    pagination: PaginationInfo,
}

#[derive(Debug, Serialize)]
struct TaskSummary {
    id: String,
    title: String,
    status: TaskStatus,
    assigned_to: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct PaginationInfo {
    page: u32,
    page_size: u32,
    total_items: u64,
    total_pages: u32,
}

async fn list_tasks(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListTasksQuery>,
) -> Result<Json<ListTasksResponse>, ApiError> {
    // Get projection
    let projection = state.projections
        .read()
        .await
        .get::<TaskListProjection>()
        .ok_or_else(|| ApiError::internal("Task projection not available"))?;
    
    // Apply filters
    let mut tasks = projection.get_all_tasks();
    
    if let Some(status) = query.status {
        tasks.retain(|t| t.status == status);
    }
    
    if let Some(assigned_to) = query.assigned_to {
        tasks.retain(|t| t.assigned_to.as_ref() == Some(&assigned_to));
    }
    
    // Calculate pagination
    let total_items = tasks.len() as u64;
    let total_pages = ((total_items as f32) / (query.page_size as f32)).ceil() as u32;
    
    // Apply pagination
    let start = ((query.page - 1) * query.page_size) as usize;
    let end = (start + query.page_size as usize).min(tasks.len());
    let page_tasks = tasks[start..end].to_vec();
    
    Ok(Json(ListTasksResponse {
        tasks: page_tasks.into_iter().map(Into::into).collect(),
        pagination: PaginationInfo {
            page: query.page,
            page_size: query.page_size,
            total_items,
            total_pages,
        },
    }))
}
```

## Advanced Query Patterns

### Filtering and Sorting

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortField {
    CreatedAt,
    UpdatedAt,
    Title,
    Priority,
    DueDate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Deserialize)]
struct AdvancedTaskQuery {
    // Filters
    #[serde(default)]
    status: Option<Vec<TaskStatus>>,
    
    #[serde(default)]
    assigned_to: Option<Vec<String>>,
    
    #[serde(default)]
    created_after: Option<DateTime<Utc>>,
    
    #[serde(default)]
    created_before: Option<DateTime<Utc>>,
    
    #[serde(default)]
    search: Option<String>,
    
    // Sorting
    #[serde(default = "default_sort_field")]
    sort_by: SortField,
    
    #[serde(default = "default_sort_order")]
    sort_order: SortOrder,
    
    // Pagination
    #[serde(default)]
    cursor: Option<String>,
    
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_sort_field() -> SortField { SortField::CreatedAt }
fn default_sort_order() -> SortOrder { SortOrder::Desc }
fn default_limit() -> u32 { 50 }

async fn search_tasks(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<AdvancedTaskQuery>,
) -> Result<Json<CursorPaginatedResponse<TaskSummary>>, ApiError> {
    let projection = state.projections
        .read()
        .await
        .get::<TaskSearchProjection>()
        .ok_or_else(|| ApiError::internal("Search projection not available"))?;
    
    // Build query
    let mut search_query = SearchQuery::new();
    
    if let Some(statuses) = query.status {
        search_query = search_query.with_status_in(statuses);
    }
    
    if let Some(assignees) = query.assigned_to {
        search_query = search_query.with_assignee_in(assignees);
    }
    
    if let Some(after) = query.created_after {
        search_query = search_query.created_after(after);
    }
    
    if let Some(before) = query.created_before {
        search_query = search_query.created_before(before);
    }
    
    if let Some(search_text) = query.search {
        search_query = search_query.with_text_search(search_text);
    }
    
    // Apply sorting
    search_query = match query.sort_by {
        SortField::CreatedAt => search_query.sort_by_created_at(query.sort_order),
        SortField::UpdatedAt => search_query.sort_by_updated_at(query.sort_order),
        SortField::Title => search_query.sort_by_title(query.sort_order),
        SortField::Priority => search_query.sort_by_priority(query.sort_order),
        SortField::DueDate => search_query.sort_by_due_date(query.sort_order),
    };
    
    // Apply cursor pagination
    if let Some(cursor) = query.cursor {
        search_query = search_query.after_cursor(Cursor::decode(&cursor)?);
    }
    
    search_query = search_query.limit(query.limit);
    
    // Execute query
    let results = projection.search(search_query).await?;
    
    Ok(Json(results))
}
```

### Aggregation Queries

```rust
#[derive(Debug, Serialize)]
struct TaskStatistics {
    total_tasks: u64,
    tasks_by_status: HashMap<TaskStatus, u64>,
    tasks_by_assignee: Vec<AssigneeStats>,
    completion_rate: f64,
    average_completion_time: Option<Duration>,
    overdue_tasks: u64,
}

#[derive(Debug, Serialize)]
struct AssigneeStats {
    assignee_id: String,
    assignee_name: String,
    total_tasks: u64,
    completed_tasks: u64,
    in_progress_tasks: u64,
}

async fn get_task_statistics(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<DateRangeQuery>,
) -> Result<Json<TaskStatistics>, ApiError> {
    let projection = state.projections
        .read()
        .await
        .get::<TaskAnalyticsProjection>()
        .ok_or_else(|| ApiError::internal("Analytics projection not available"))?;
    
    let stats = projection.calculate_statistics(
        query.start_date,
        query.end_date,
    ).await?;
    
    Ok(Json(stats))
}

// Time-series data
#[derive(Debug, Serialize)]
struct TimeSeriesData {
    period: String,
    data_points: Vec<DataPoint>,
}

#[derive(Debug, Serialize)]
struct DataPoint {
    timestamp: DateTime<Utc>,
    value: f64,
    metadata: Option<serde_json::Value>,
}

async fn get_task_completion_trend(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<TimeSeriesQuery>,
) -> Result<Json<TimeSeriesData>, ApiError> {
    let projection = state.projections
        .read()
        .await
        .get::<TaskMetricsProjection>()
        .ok_or_else(|| ApiError::internal("Metrics projection not available"))?;
    
    let data = projection.get_completion_trend(
        query.start_date,
        query.end_date,
        query.granularity,
    ).await?;
    
    Ok(Json(data))
}
```

## GraphQL Integration

For complex queries, GraphQL can be more efficient:

```rust
use async_graphql::{
    Context, Object, Schema, EmptyMutation, EmptySubscription,
    ID, Result as GraphQLResult,
};

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn task(&self, ctx: &Context<'_>, id: ID) -> GraphQLResult<Option<Task>> {
        let projection = ctx.data::<Arc<TaskProjection>>()?;
        
        Ok(projection.get_task(&id.to_string()).await?)
    }
    
    async fn tasks(
        &self,
        ctx: &Context<'_>,
        filter: Option<TaskFilter>,
        sort: Option<TaskSort>,
        pagination: Option<PaginationInput>,
    ) -> GraphQLResult<TaskConnection> {
        let projection = ctx.data::<Arc<TaskProjection>>()?;
        
        let query = build_query(filter, sort, pagination);
        let results = projection.query(query).await?;
        
        Ok(TaskConnection::from(results))
    }
    
    async fn user(&self, ctx: &Context<'_>, id: ID) -> GraphQLResult<Option<User>> {
        let projection = ctx.data::<Arc<UserProjection>>()?;
        
        Ok(projection.get_user(&id.to_string()).await?)
    }
}

// GraphQL types
#[derive(async_graphql::SimpleObject)]
struct Task {
    id: ID,
    title: String,
    description: String,
    status: TaskStatus,
    assigned_to: Option<User>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(async_graphql::InputObject)]
struct TaskFilter {
    status: Option<Vec<TaskStatus>>,
    assigned_to: Option<Vec<ID>>,
    created_after: Option<DateTime<Utc>>,
    search: Option<String>,
}

// Axum handler
async fn graphql_handler(
    State(state): State<AppState>,
    user: Option<AuthenticatedUser>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(state.projections.clone())
        .data(user)
        .finish();
    
    schema.execute(req.into_inner()).await.into()
}
```

## Caching Strategies

### Response Caching

```rust
use axum::http::header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH};
use sha2::{Sha256, Digest};

#[derive(Clone)]
struct CacheConfig {
    public_max_age: Duration,
    private_max_age: Duration,
    stale_while_revalidate: Duration,
}

async fn cached_query_handler<F, Fut, T>(
    headers: HeaderMap,
    cache_config: CacheConfig,
    query_fn: F,
) -> Response
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
    T: Serialize,
{
    // Execute query
    let result = match query_fn().await {
        Ok(data) => data,
        Err(e) => return e.into_response(),
    };
    
    // Serialize response
    let body = match serde_json::to_vec(&result) {
        Ok(bytes) => bytes,
        Err(_) => return ApiError::internal("Serialization failed").into_response(),
    };
    
    // Calculate ETag
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let etag = format!("\"{}\"", hex::encode(hasher.finalize()));
    
    // Check If-None-Match
    if let Some(if_none_match) = headers.get(IF_NONE_MATCH) {
        if if_none_match.to_str().ok() == Some(&etag) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
    }
    
    // Build response with caching headers
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json")
        .header(ETAG, &etag)
        .header(
            CACHE_CONTROL,
            format!(
                "public, max-age={}, stale-while-revalidate={}",
                cache_config.public_max_age.as_secs(),
                cache_config.stale_while_revalidate.as_secs()
            )
        )
        .body(Body::from(body))
        .unwrap()
}

// Usage
async fn get_public_statistics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    cached_query_handler(
        headers,
        CacheConfig {
            public_max_age: Duration::from_secs(300), // 5 minutes
            private_max_age: Duration::from_secs(0),
            stale_while_revalidate: Duration::from_secs(60),
        },
        || async {
            let projection = state.projections
                .read()
                .await
                .get::<PublicStatsProjection>()
                .ok_or_else(|| ApiError::internal("Stats not available"))?;
            
            projection.get_current_stats().await
        },
    ).await
}
```

### Query Result Caching

```rust
use moka::future::Cache;

#[derive(Clone)]
struct QueryCache {
    cache: Cache<String, CachedResult>,
}

#[derive(Clone)]
struct CachedResult {
    data: Vec<u8>,
    cached_at: DateTime<Utc>,
    ttl: Duration,
}

impl QueryCache {
    fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
        }
    }
    
    async fn get_or_compute<F, Fut, T>(
        &self,
        key: &str,
        ttl: Duration,
        compute_fn: F,
    ) -> Result<T, ApiError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
        T: Serialize + DeserializeOwned,
    {
        // Check cache
        if let Some(cached) = self.cache.get(key).await {
            if Utc::now() - cached.cached_at < cached.ttl {
                return serde_json::from_slice(&cached.data)
                    .map_err(|_| ApiError::internal("Cache deserialization failed"));
            }
        }
        
        // Compute result
        let result = compute_fn().await?;
        
        // Cache result
        let data = serde_json::to_vec(&result)
            .map_err(|_| ApiError::internal("Cache serialization failed"))?;
        
        self.cache.insert(
            key.to_string(),
            CachedResult {
                data,
                cached_at: Utc::now(),
                ttl,
            }
        ).await;
        
        Ok(result)
    }
}
```

## Real-time Queries with SSE

Server-Sent Events for live updates:

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use tokio_stream::StreamExt;

async fn task_updates_stream(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Sse<impl Stream<Item = Result<Event, ApiError>>> {
    let stream = async_stream::stream! {
        let mut subscription = state.projections
            .read()
            .await
            .get::<TaskProjection>()
            .unwrap()
            .subscribe_to_updates(user.id)
            .await;
        
        while let Some(update) = subscription.next().await {
            let event = match update {
                TaskUpdate::Created(task) => {
                    Event::default()
                        .event("task-created")
                        .json_data(task)
                        .unwrap()
                }
                TaskUpdate::Updated(task) => {
                    Event::default()
                        .event("task-updated")
                        .json_data(task)
                        .unwrap()
                }
                TaskUpdate::Deleted(task_id) => {
                    Event::default()
                        .event("task-deleted")
                        .data(task_id)
                }
            };
            
            yield Ok(event);
        }
    };
    
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive")
    )
}
```

## Query Performance Optimization

### N+1 Query Prevention

```rust
// Bad: N+1 queries
async fn get_tasks_with_assignees_bad(
    projection: &TaskProjection,
) -> Result<Vec<TaskWithAssignee>, ApiError> {
    let tasks = projection.get_all_tasks().await?;
    let mut results = Vec::new();
    
    for task in tasks {
        // This makes a separate query for each task!
        let assignee = if let Some(assignee_id) = &task.assigned_to {
            projection.get_user(assignee_id).await?
        } else {
            None
        };
        
        results.push(TaskWithAssignee {
            task,
            assignee,
        });
    }
    
    Ok(results)
}

// Good: Batch loading
async fn get_tasks_with_assignees_good(
    projection: &TaskProjection,
) -> Result<Vec<TaskWithAssignee>, ApiError> {
    let tasks = projection.get_all_tasks().await?;
    
    // Collect all assignee IDs
    let assignee_ids: HashSet<_> = tasks
        .iter()
        .filter_map(|t| t.assigned_to.as_ref())
        .cloned()
        .collect();
    
    // Load all assignees in one query
    let assignees = projection
        .get_users_by_ids(assignee_ids.into_iter().collect())
        .await?;
    
    // Build results
    let assignee_map: HashMap<_, _> = assignees
        .into_iter()
        .map(|u| (u.id.clone(), u))
        .collect();
    
    Ok(tasks.into_iter().map(|task| {
        let assignee = task.assigned_to
            .as_ref()
            .and_then(|id| assignee_map.get(id))
            .cloned();
        
        TaskWithAssignee { task, assignee }
    }).collect())
}
```

### Query Complexity Limits

```rust
use async_graphql::{extensions::ComplexityLimit, ValidationResult};

struct QueryComplexity;

impl QueryComplexity {
    fn calculate_complexity(query: &GraphQLQuery) -> u32 {
        // Simple heuristic: count fields and multiply by depth
        let field_count = count_fields(query);
        let max_depth = calculate_max_depth(query);
        
        field_count * max_depth
    }
}

// In GraphQL schema
let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
    .extension(ComplexityLimit::new(1000)) // Max complexity
    .finish();

// For REST endpoints
#[derive(Debug)]
struct QueryComplexityGuard {
    max_items: u32,
    max_depth: u32,
}

impl QueryComplexityGuard {
    fn validate(&self, query: &AdvancedTaskQuery) -> Result<(), ApiError> {
        // Check pagination limits
        if query.limit > self.max_items {
            return Err(ApiError::bad_request(
                format!("Limit cannot exceed {}", self.max_items)
            ));
        }
        
        // Check filter complexity
        let filter_count = 
            query.status.as_ref().map(|s| s.len()).unwrap_or(0) +
            query.assigned_to.as_ref().map(|a| a.len()).unwrap_or(0);
        
        if filter_count > 100 {
            return Err(ApiError::bad_request(
                "Too many filter values"
            ));
        }
        
        Ok(())
    }
}
```

## Security Considerations

### Query Authorization

```rust
#[async_trait]
trait QueryAuthorizer {
    async fn can_view_task(&self, user: &AuthenticatedUser, task_id: &str) -> bool;
    async fn can_view_user_tasks(&self, user: &AuthenticatedUser, target_user_id: &str) -> bool;
    async fn can_view_statistics(&self, user: &AuthenticatedUser) -> bool;
}

struct RoleBasedAuthorizer;

#[async_trait]
impl QueryAuthorizer for RoleBasedAuthorizer {
    async fn can_view_task(&self, user: &AuthenticatedUser, task_id: &str) -> bool {
        // Admin can see all
        if user.has_role("admin") {
            return true;
        }
        
        // Others can only see their own tasks or tasks they created
        // Would need to check task details...
        true
    }
    
    async fn can_view_user_tasks(&self, user: &AuthenticatedUser, target_user_id: &str) -> bool {
        // Users can see their own tasks
        if user.id.to_string() == target_user_id {
            return true;
        }
        
        // Managers can see their team's tasks
        user.has_role("manager") || user.has_role("admin")
    }
    
    async fn can_view_statistics(&self, user: &AuthenticatedUser) -> bool {
        user.has_role("manager") || user.has_role("admin")
    }
}

// Use in handlers
async fn get_user_tasks(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<TaskSummary>>, ApiError> {
    // Check authorization
    if !state.authorizer.can_view_user_tasks(&user, &user_id).await {
        return Err(ApiError::forbidden("Cannot view tasks for this user"));
    }
    
    // Continue with query...
}
```

### Rate Limiting

```rust
use governor::{Quota, RateLimiter};

#[derive(Clone)]
struct RateLimitConfig {
    anonymous_quota: Quota,
    authenticated_quota: Quota,
    admin_quota: Quota,
}

async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter<String>>>,
    user: Option<AuthenticatedUser>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let key = match &user {
        Some(u) => u.id.to_string(),
        None => request
            .headers()
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("anonymous")
            .to_string(),
    };
    
    limiter
        .check_key(&key)
        .map_err(|_| ApiError::too_many_requests("Rate limit exceeded"))?;
    
    Ok(next.run(request).await)
}
```

## Testing Query Endpoints

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_pagination() {
        let state = create_test_state_with_tasks(100).await;
        
        // First page
        let response = list_tasks(
            State(state.clone()),
            QueryParams(ListTasksQuery {
                page: 1,
                page_size: 20,
                ..Default::default()
            }),
        ).await.unwrap();
        
        assert_eq!(response.0.tasks.len(), 20);
        assert_eq!(response.0.pagination.total_items, 100);
        assert_eq!(response.0.pagination.total_pages, 5);
        
        // Last page
        let response = list_tasks(
            State(state),
            QueryParams(ListTasksQuery {
                page: 5,
                page_size: 20,
                ..Default::default()
            }),
        ).await.unwrap();
        
        assert_eq!(response.0.tasks.len(), 20);
    }
    
    #[tokio::test]
    async fn test_caching_headers() {
        let state = create_test_state().await;
        
        let response = get_public_statistics(
            State(state),
            HeaderMap::new(),
        ).await;
        
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(ETAG));
        assert!(response.headers().contains_key(CACHE_CONTROL));
        
        let cache_control = response.headers()
            .get(CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap();
        
        assert!(cache_control.contains("max-age=300"));
    }
}
```

## Best Practices

1. **Use projections** - Don't query event streams directly
2. **Paginate results** - Never return unbounded lists
3. **Cache aggressively** - Read queries are perfect for caching
4. **Validate query parameters** - Prevent resource exhaustion
5. **Monitor performance** - Track slow queries
6. **Use appropriate protocols** - REST for simple, GraphQL for complex
7. **Implement authorization** - Check permissions for all queries
8. **Version your API** - Queries can evolve independently

## Summary

Query endpoints in EventCore applications:

- ✅ **Projection-based** - Read from optimized projections
- ✅ **Performant** - Caching and optimization built-in
- ✅ **Flexible** - Support REST, GraphQL, and real-time
- ✅ **Secure** - Authorization and rate limiting
- ✅ **Testable** - Easy to test in isolation

Key patterns:
1. Read from projections, not event streams
2. Implement proper pagination
3. Cache responses appropriately
4. Validate and limit query complexity
5. Authorize access to data
6. Monitor query performance

Next, let's explore [Authentication and Authorization](./04-authentication.md) →