# Authentication & Authorization

EventCore is authentication-agnostic but provides hooks for integrating your auth system.

## Authentication Integration

### Capturing User Identity

EventCore's metadata system captures user identity for audit trails:

```rust
use eventcore::{CommandExecutor, UserId};

// Execute command with authenticated user
let user_id = UserId::try_new("user@example.com")?;
let result = executor
    .execute_as_user(command, user_id)
    .await?;
```

### Middleware Pattern

Implement authentication as middleware:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

async fn auth_middleware(
    State(auth): State<AuthService>,
    headers: HeaderMap,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract and verify token
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    
    let user = auth
        .verify_token(token)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    
    // Add user to request extensions
    req.extensions_mut().insert(user);
    
    Ok(next.run(req).await)
}
```

## Authorization Patterns

### Stream-Level Authorization

Implement fine-grained access control:

```rust
#[async_trait]
trait StreamAuthorization {
    async fn can_read(&self, user: &User, stream_id: &StreamId) -> bool;
    async fn can_write(&self, user: &User, stream_id: &StreamId) -> bool;
}

struct CommandAuthorizationLayer<A: StreamAuthorization> {
    auth: A,
}

impl<A: StreamAuthorization> CommandAuthorizationLayer<A> {
    async fn authorize_command(
        &self,
        command: &impl Command,
        user: &User,
    ) -> Result<(), AuthError> {
        // Check read permissions
        for stream_id in command.read_streams() {
            if !self.auth.can_read(user, &stream_id).await {
                return Err(AuthError::Forbidden(stream_id));
            }
        }
        
        // Check write permissions
        for stream_id in command.write_streams() {
            if !self.auth.can_write(user, &stream_id).await {
                return Err(AuthError::Forbidden(stream_id));
            }
        }
        
        Ok(())
    }
}
```

### Role-Based Access Control (RBAC)

```rust
#[derive(Debug, Clone)]
enum Role {
    Admin,
    User,
    ReadOnly,
}

#[derive(Debug, Clone)]
struct User {
    id: UserId,
    roles: Vec<Role>,
}

impl User {
    fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }
    
    fn can_execute_command(&self, command_type: &str) -> bool {
        match command_type {
            "CreateAccount" => self.has_role(&Role::Admin),
            "UpdateAccount" => {
                self.has_role(&Role::Admin) || self.has_role(&Role::User)
            }
            "ViewAccount" => true, // All authenticated users
            _ => false,
        }
    }
}
```

### Attribute-Based Access Control (ABAC)

```rust
#[derive(Debug)]
struct AccessContext {
    user: User,
    resource: Resource,
    action: Action,
    environment: Environment,
}

#[async_trait]
trait AccessPolicy {
    async fn evaluate(&self, context: &AccessContext) -> Decision;
}

struct AbacAuthorizer {
    policies: Vec<Box<dyn AccessPolicy>>,
}

impl AbacAuthorizer {
    async fn authorize(&self, context: AccessContext) -> Result<(), AuthError> {
        for policy in &self.policies {
            match policy.evaluate(&context).await {
                Decision::Deny(reason) => {
                    return Err(AuthError::PolicyDenied(reason));
                }
                Decision::Allow => continue,
            }
        }
        Ok(())
    }
}
```

## Projection Security

### Row-Level Security

Filter projections based on user permissions:

```rust
#[async_trait]
impl ReadModelStore for SecureAccountStore {
    async fn get_account(
        &self,
        account_id: &AccountId,
        user: &User,
    ) -> Result<Option<AccountReadModel>> {
        let account = self.inner.get_account(account_id).await?;
        
        // Apply row-level security
        match account {
            Some(acc) if self.user_can_view(&acc, user) => Ok(Some(acc)),
            _ => Ok(None),
        }
    }
    
    async fn list_accounts(
        &self,
        user: &User,
        filter: AccountFilter,
    ) -> Result<Vec<AccountReadModel>> {
        let accounts = self.inner.list_accounts(filter).await?;
        
        // Filter based on permissions
        Ok(accounts
            .into_iter()
            .filter(|acc| self.user_can_view(acc, user))
            .collect())
    }
}
```

### Field-Level Security

Redact sensitive fields:

```rust
impl AccountReadModel {
    fn redact_for_user(&self, user: &User) -> Self {
        let mut redacted = self.clone();
        
        if !user.has_role(&Role::Admin) {
            redacted.ssn = None;
            redacted.tax_id = None;
        }
        
        if !user.has_role(&Role::Financial) {
            redacted.balance = None;
            redacted.credit_limit = None;
        }
        
        redacted
    }
}
```

## Best Practices

1. **Fail Secure**: Default to denying access
2. **Audit Everything**: Log all authorization decisions
3. **Minimize Privileges**: Grant only necessary permissions
4. **Separate Concerns**: Keep auth logic separate from business logic
5. **Token Expiry**: Implement short-lived tokens with refresh
6. **Rate Limiting**: Prevent brute force attacks

## Common Pitfalls

- Not checking permissions on read models
- Forgetting to validate token expiry
- Exposing internal IDs that enable enumeration
- Not rate limiting authentication attempts
- Storing permissions in events (they change over time)