# Chapter 4.4: Authentication and Authorization

Security is critical for event-sourced systems. This chapter covers authentication (who you are) and authorization (what you can do) patterns for EventCore APIs.

## Authentication Strategies

### JWT Authentication

JSON Web Tokens are stateless and work well with EventCore:

```rust
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,          // Subject (user ID)
    exp: usize,           // Expiration time
    iat: usize,           // Issued at
    roles: Vec<String>,   // User roles
    permissions: Vec<String>, // Specific permissions
}

#[derive(Clone)]
struct JwtConfig {
    secret: String,
    issuer: String,
    audience: String,
    access_token_duration: Duration,
    refresh_token_duration: Duration,
}

impl JwtConfig {
    fn create_access_token(&self, user: &User) -> Result<String, ApiError> {
        let now = Utc::now();
        let exp = now + self.access_token_duration;

        let claims = Claims {
            sub: user.id.to_string(),
            exp: exp.timestamp() as usize,
            iat: now.timestamp() as usize,
            roles: user.roles.clone(),
            permissions: user.permissions.clone(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_ref()),
        )
        .map_err(|_| ApiError::internal("Failed to create token"))
    }

    fn validate_token(&self, token: &str) -> Result<Claims, ApiError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);

        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_ref()),
            &validation,
        )
        .map(|data| data.claims)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                ApiError::unauthorized("Token expired")
            }
            _ => ApiError::unauthorized("Invalid token"),
        })
    }
}
```

### Login Endpoint

```rust
#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    // Validate credentials
    let email = Email::try_new(request.email)
        .map_err(|_| ApiError::bad_request("Invalid email"))?;

    // Execute authentication command
    let command = AuthenticateUser {
        email: email.clone(),
        password: Password::from(request.password),
    };

    let result = state.executor
        .execute(&command)
        .await
        .map_err(|_| ApiError::unauthorized("Invalid credentials"))?;

    // Get user from projection
    let user = state.projections
        .read()
        .await
        .get::<UserProjection>()
        .unwrap()
        .get_user_by_email(&email)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Invalid credentials"))?;

    // Create tokens
    let access_token = state.jwt_config.create_access_token(&user)?;
    let refresh_token = state.jwt_config.create_refresh_token(&user)?;

    // Store refresh token (for revocation)
    let store_command = StoreRefreshToken {
        user_id: user.id.clone(),
        token_hash: hash_token(&refresh_token),
        expires_at: Utc::now() + state.jwt_config.refresh_token_duration,
    };

    state.executor.execute(&store_command).await?;

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_in: state.jwt_config.access_token_duration.as_secs(),
    }))
}
```

### Authentication Middleware

```rust
use axum::{
    extract::{FromRequestParts, Request},
    middleware::{self, Next},
    response::Response,
};

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: UserId,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Get JWT config from extensions (set by middleware)
        let jwt_config = parts
            .extensions
            .get::<JwtConfig>()
            .ok_or_else(|| ApiError::internal("JWT config not found"))?;

        // Extract token from Authorization header
        let token = extract_bearer_token(&parts.headers)?;

        // Validate token
        let claims = jwt_config.validate_token(token)?;

        Ok(AuthenticatedUser {
            id: UserId::try_new(claims.sub)?,
            roles: claims.roles,
            permissions: claims.permissions,
        })
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::unauthorized("Missing or invalid Authorization header"))
}

// Optional authentication extractor
pub struct OptionalAuth(pub Option<AuthenticatedUser>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalAuth
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuth(
            AuthenticatedUser::from_request_parts(parts, state)
                .await
                .ok()
        ))
    }
}
```

## Authorization Patterns

### Role-Based Access Control (RBAC)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Role {
    Admin,
    Manager,
    Employee,
    Guest,
}

impl AuthenticatedUser {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(&role.to_string())
    }

    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|role| self.has_role(role))
    }

    pub fn has_all_roles(&self, roles: &[&str]) -> bool {
        roles.iter().all(|role| self.has_role(role))
    }
}

// Authorization guard
async fn require_role(
    user: &AuthenticatedUser,
    role: &str,
) -> Result<(), ApiError> {
    if !user.has_role(role) {
        return Err(ApiError::forbidden(
            format!("Requires {} role", role)
        ));
    }
    Ok(())
}

// In handlers
async fn admin_endpoint(
    user: AuthenticatedUser,
    // other params...
) -> Result<Json<AdminData>, ApiError> {
    require_role(&user, "admin").await?;

    // Admin-only logic...
}
```

### Permission-Based Access Control

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Permission {
    // Task permissions
    CreateTask,
    ReadTask,
    UpdateTask,
    DeleteTask,
    AssignTask,

    // User permissions
    CreateUser,
    ReadUser,
    UpdateUser,
    DeleteUser,

    // Admin permissions
    ViewAnalytics,
    ManageSystem,
}

impl AuthenticatedUser {
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(&permission.to_string())
    }

    pub fn can(&self, action: Permission) -> bool {
        self.has_permission(&action.to_string())
    }
}

// Permission checking in handlers
async fn create_task_handler(
    user: AuthenticatedUser,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<CreateTaskResponse>, ApiError> {
    if !user.can(Permission::CreateTask) {
        return Err(ApiError::forbidden("Cannot create tasks"));
    }

    // Create task...
}
```

### Resource-Based Access Control

```rust
#[async_trait]
trait ResourceAuthorizer {
    async fn can_read(&self, user: &AuthenticatedUser, resource_id: &str) -> bool;
    async fn can_write(&self, user: &AuthenticatedUser, resource_id: &str) -> bool;
    async fn can_delete(&self, user: &AuthenticatedUser, resource_id: &str) -> bool;
}

struct TaskAuthorizer {
    projection: Arc<TaskProjection>,
}

#[async_trait]
impl ResourceAuthorizer for TaskAuthorizer {
    async fn can_read(&self, user: &AuthenticatedUser, task_id: &str) -> bool {
        // Admins can read all
        if user.has_role("admin") {
            return true;
        }

        // Check if user owns or is assigned to task
        if let Ok(Some(task)) = self.projection.get_task(task_id).await {
            return task.created_by == user.id ||
                   task.assigned_to == Some(user.id.clone());
        }

        false
    }

    async fn can_write(&self, user: &AuthenticatedUser, task_id: &str) -> bool {
        // Similar logic for write permissions
        if user.has_role("admin") || user.has_role("manager") {
            return true;
        }

        // Check ownership or assignment
        if let Ok(Some(task)) = self.projection.get_task(task_id).await {
            return task.assigned_to == Some(user.id.clone());
        }

        false
    }

    async fn can_delete(&self, user: &AuthenticatedUser, task_id: &str) -> bool {
        // Only admins and creators can delete
        if user.has_role("admin") {
            return true;
        }

        if let Ok(Some(task)) = self.projection.get_task(task_id).await {
            return task.created_by == user.id;
        }

        false
    }
}
```

## Command Authorization

Embed authorization in commands:

```rust
#[derive(Command, Clone)]
struct UpdateTask {
    #[stream]
    task_id: StreamId,

    title: Option<TaskTitle>,
    description: Option<TaskDescription>,

    // Who is making the change
    updated_by: UserId,
}

impl CommandLogic for UpdateTask {
    // ... other implementations

    async fn handle(
        &self,
        stream_declarations: StreamDeclarations<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check authorization within command
        require!(
            state.can_user_update_task(&self.updated_by),
            "User {} cannot update task {}",
            self.updated_by,
            self.task_id
        );

        // Proceed with update...
    }
}

// State includes authorization data
impl TaskState {
    fn can_user_update_task(&self, user_id: &UserId) -> bool {
        // Task creator can always update
        if self.created_by == *user_id {
            return true;
        }

        // Assigned user can update
        if self.assigned_to == Some(user_id.clone()) {
            return true;
        }

        // Check roles (would need to be passed in state)
        false
    }
}
```

## API Key Authentication

For service-to-service communication:

```rust
#[derive(Debug, Clone)]
struct ApiKey {
    key: String,
    service_name: String,
    permissions: Vec<String>,
    rate_limit: Option<u32>,
}

#[async_trait]
impl<S> FromRequestParts<S> for ApiKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("Missing API key"))?;

        // Look up API key (from cache/database)
        let api_key = validate_api_key(key).await?;

        Ok(api_key)
    }
}

async fn validate_api_key(key: &str) -> Result<ApiKey, ApiError> {
    // Hash the key for lookup
    let key_hash = hash_api_key(key);

    // Look up in projection/cache
    let api_key = get_api_key_by_hash(&key_hash)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Invalid API key"))?;

    // Check if expired
    if api_key.expires_at < Utc::now() {
        return Err(ApiError::unauthorized("API key expired"));
    }

    Ok(api_key)
}
```

## OAuth2 Integration

For third-party authentication:

```rust
use oauth2::{
    AuthorizationCode, AuthUrl, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, RedirectUrl, TokenResponse, TokenUrl,
};

#[derive(Clone)]
struct OAuth2Config {
    client_id: ClientId,
    client_secret: ClientSecret,
    auth_url: AuthUrl,
    token_url: TokenUrl,
    redirect_url: RedirectUrl,
}

async fn oauth_login(
    State(oauth): State<OAuth2Config>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Redirect, ApiError> {
    let client = BasicClient::new(
        oauth.client_id,
        Some(oauth.client_secret),
        oauth.auth_url,
        Some(oauth.token_url),
    )
    .set_redirect_uri(oauth.redirect_url);

    // Generate PKCE challenge
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Generate authorization URL
    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Store CSRF token and PKCE verifier (in session/cache)
    store_oauth_state(&csrf_token, &pkce_verifier).await?;

    Ok(Redirect::to(auth_url.as_str()))
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Result<Json<LoginResponse>, ApiError> {
    // Verify CSRF token
    let (stored_csrf, pkce_verifier) = get_oauth_state(&params.state).await?;

    if stored_csrf != params.state {
        return Err(ApiError::bad_request("Invalid state parameter"));
    }

    // Exchange code for token
    let token_result = exchange_code_for_token(
        &state.oauth_config,
        &params.code,
        &pkce_verifier,
    ).await?;

    // Get user info from provider
    let user_info = fetch_user_info(&token_result.access_token()).await?;

    // Create or update user in EventCore
    let command = CreateOrUpdateOAuthUser {
        provider: "github".to_string(),
        provider_user_id: user_info.id,
        email: user_info.email,
        name: user_info.name,
    };

    state.executor.execute(&command).await?;

    // Create JWT tokens
    let user = get_user_by_email(&user_info.email).await?;
    let access_token = state.jwt_config.create_access_token(&user)?;

    Ok(Json(LoginResponse {
        access_token,
        // ... other fields
    }))
}
```

## Session Management

Track active sessions:

```rust
#[derive(Command, Clone)]
struct CreateSession {
    #[stream]
    user_id: StreamId,

    #[stream]
    session_id: StreamId,

    ip_address: IpAddr,
    user_agent: String,
    expires_at: DateTime<Utc>,
}

#[derive(Command, Clone)]
struct RevokeSession {
    #[stream]
    session_id: StreamId,

    #[stream]
    user_id: StreamId,

    reason: RevocationReason,
}

// Session validation middleware
async fn validate_session(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let session_id = extract_session_id(&request)?;

    // Check if session is valid
    let session = state.projections
        .read()
        .await
        .get::<SessionProjection>()
        .unwrap()
        .get_session(&session_id)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Invalid session"))?;

    // Verify session belongs to user
    if session.user_id != user.id {
        return Err(ApiError::unauthorized("Session mismatch"));
    }

    // Check expiration
    if session.expires_at < Utc::now() {
        return Err(ApiError::unauthorized("Session expired"));
    }

    // Check if revoked
    if session.revoked {
        return Err(ApiError::unauthorized("Session revoked"));
    }

    Ok(next.run(request).await)
}
```

## Security Headers

Add security headers to all responses:

```rust
async fn security_headers_middleware(
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();

    // Prevent clickjacking
    headers.insert(
        "X-Frame-Options",
        HeaderValue::from_static("DENY"),
    );

    // XSS protection
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );

    // CSP
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static("default-src 'self'"),
    );

    // HSTS
    headers.insert(
        "Strict-Transport-Security",
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );

    response
}
```

## Testing Authentication

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_token(user_id: &str, roles: Vec<&str>) -> String {
        let claims = Claims {
            sub: user_id.to_string(),
            exp: (Utc::now() + Duration::hours(1)).timestamp() as usize,
            iat: Utc::now().timestamp() as usize,
            roles: roles.into_iter().map(|s| s.to_string()).collect(),
            permissions: vec![],
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        ).unwrap()
    }

    #[tokio::test]
    async fn test_authentication_required() {
        let app = create_test_app();

        // No token
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_role_authorization() {
        let app = create_test_app();

        // User token without admin role
        let token = create_test_token("user123", vec!["user"]);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/users")
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
```

## Best Practices

1. **Use HTTPS always** - Never send tokens over unencrypted connections
2. **Short token lifetimes** - Access tokens should expire quickly
3. **Refresh tokens** - Use refresh tokens for long-lived sessions
4. **Store hashes** - Never store plaintext tokens or passwords
5. **Audit everything** - Log all authentication/authorization events
6. **Principle of least privilege** - Grant minimal necessary permissions
7. **Defense in depth** - Layer multiple security mechanisms
8. **Regular reviews** - Audit permissions and access regularly

## Summary

Authentication and authorization in EventCore:

- ✅ **Flexible strategies** - JWT, API keys, OAuth2
- ✅ **Strong typing** - Type-safe user and permission models
- ✅ **Event sourced** - Authentication events provide audit trail
- ✅ **Performance** - Caching for fast authorization checks
- ✅ **Testable** - Easy to test security rules

Key patterns:

1. Authenticate early in the request pipeline
2. Embed authorization in commands
3. Use projections for fast permission lookups
4. Audit all security events
5. Test security thoroughly

Next, let's explore [API Versioning](./05-api-versioning.md) →
