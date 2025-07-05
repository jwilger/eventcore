# Input Validation

Proper input validation prevents injection attacks and data corruption.

## Validation Layers

### 1. API Layer Validation

Validate at the edge before data enters your system:

```rust
use axum::{
    extract::Json,
    http::StatusCode,
    response::IntoResponse,
};
use validator::{Validate, ValidationError};

#[derive(Debug, Deserialize, Validate)]
struct CreateUserRequest {
    #[validate(length(min = 3, max = 50))]
    username: String,
    
    #[validate(email)]
    email: String,
    
    #[validate(length(min = 8), custom = "validate_password_strength")]
    password: String,
    
    #[validate(range(min = 13, max = 120))]
    age: u8,
}

fn validate_password_strength(password: &str) -> Result<(), ValidationError> {
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_lowercase = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_digit(10));
    let has_special = password.chars().any(|c| !c.is_alphanumeric());
    
    if !(has_uppercase && has_lowercase && has_digit && has_special) {
        return Err(ValidationError::new("weak_password"));
    }
    
    Ok(())
}

async fn create_user(
    Json(request): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    // Validation happens automatically during deserialization
    request.validate()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Continue with validated data...
    Ok(StatusCode::CREATED)
}
```

### 2. Domain Type Validation

Use `nutype` for domain-level validation:

```rust
use nutype::nutype;

#[nutype(
    sanitize(trim, lowercase),
    validate(
        len_char_min = 3,
        len_char_max = 50,
        regex = r"^[a-z0-9_]+$"
    ),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct Username(String);

#[nutype(
    sanitize(trim),
    validate(regex = r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct Email(String);

#[nutype(
    validate(greater_or_equal = 0, less_or_equal = 1_000_000),
    derive(Debug, Clone, Copy, Serialize, Deserialize)
)]
pub struct Money(u64); // In cents

// Usage
let username = Username::try_new("JohnDoe123")
    .map_err(|_| "Invalid username")?;
    
let email = Email::try_new("john@example.com")
    .map_err(|_| "Invalid email")?;
```

### 3. Command Validation

Validate business rules in commands:

```rust
use eventcore::{Command, CommandError, require};

#[derive(Debug, Clone)]
struct TransferMoney {
    from_account: AccountId,
    to_account: AccountId,
    amount: Money,
}

impl TransferMoney {
    fn new(
        from: AccountId,
        to: AccountId,
        amount: Money,
    ) -> Result<Self, ValidationError> {
        // Validate at construction
        if from == to {
            return Err(ValidationError::SameAccount);
        }
        
        if amount.is_zero() {
            return Err(ValidationError::ZeroAmount);
        }
        
        Ok(Self {
            from_account: from,
            to_account: to,
            amount,
        })
    }
}

#[async_trait]
impl CommandLogic for TransferMoney {
    async fn handle(&self, state: State) -> CommandResult<Vec<Event>> {
        // Business rule validation
        require!(
            state.from_balance >= self.amount,
            CommandError::InsufficientFunds
        );
        
        require!(
            state.to_account.is_active(),
            CommandError::AccountInactive
        );
        
        require!(
            self.amount <= state.daily_limit_remaining,
            CommandError::DailyLimitExceeded
        );
        
        // Proceed with valid transfer...
        Ok(vec![/* events */])
    }
}
```

## Sanitization Patterns

### HTML/Script Injection Prevention

```rust
use ammonia::clean;

#[nutype(
    sanitize(trim, with = sanitize_html),
    validate(len_char_max = 1000),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct SafeHtml(String);

fn sanitize_html(input: &str) -> String {
    // Remove dangerous HTML/JS
    clean(input)
}

// For plain text fields
#[nutype(
    sanitize(trim, with = escape_html),
    validate(len_char_max = 500),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct DisplayName(String);

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
```

### SQL Injection Prevention

EventCore uses parameterized queries via `sqlx`, but validate data types:

```rust
#[nutype(
    sanitize(trim),
    validate(regex = r"^[a-zA-Z0-9_]+$"), // Alphanumeric + underscore only
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct TableName(String);

#[nutype(
    sanitize(trim),
    validate(regex = r"^[a-zA-Z_][a-zA-Z0-9_]*$"), // Valid identifier
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct ColumnName(String);
```

## Rate Limiting

Protect against abuse:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct RateLimiter {
    limits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    async fn check_rate_limit(&self, key: &str) -> Result<(), RateLimitError> {
        let mut limits = self.limits.lock().await;
        let now = Instant::now();
        let requests = limits.entry(key.to_string()).or_default();
        
        // Remove old requests outside window
        requests.retain(|&time| now.duration_since(time) < self.window);
        
        if requests.len() >= self.max_requests {
            return Err(RateLimitError::TooManyRequests);
        }
        
        requests.push(now);
        Ok(())
    }
}

// Apply to commands
async fn execute_command(
    command: Command,
    user_id: UserId,
    rate_limiter: &RateLimiter,
) -> Result<(), Error> {
    // Rate limit by user
    rate_limiter.check_rate_limit(&user_id.to_string()).await?;
    
    // Rate limit by IP for anonymous operations
    // rate_limiter.check_rate_limit(&ip_address).await?;
    
    executor.execute(command).await
}
```

## File Upload Validation

```rust
use tokio::io::AsyncReadExt;

#[derive(Debug)]
struct FileValidator {
    max_size: usize,
    allowed_types: Vec<String>,
}

impl FileValidator {
    async fn validate_upload(
        &self,
        mut file: impl AsyncRead + Unpin,
        content_type: &str,
    ) -> Result<Vec<u8>, ValidationError> {
        // Check content type
        if !self.allowed_types.contains(&content_type.to_string()) {
            return Err(ValidationError::InvalidFileType);
        }
        
        // Read and check size
        let mut buffer = Vec::new();
        let bytes_read = file
            .take(self.max_size as u64 + 1)
            .read_to_end(&mut buffer)
            .await?;
            
        if bytes_read > self.max_size {
            return Err(ValidationError::FileTooLarge);
        }
        
        // Verify file magic numbers
        if !self.verify_file_signature(&buffer, content_type) {
            return Err(ValidationError::InvalidFileContent);
        }
        
        Ok(buffer)
    }
    
    fn verify_file_signature(&self, data: &[u8], content_type: &str) -> bool {
        match content_type {
            "image/jpeg" => data.starts_with(&[0xFF, 0xD8, 0xFF]),
            "image/png" => data.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
            "application/pdf" => data.starts_with(b"%PDF"),
            _ => true, // Add more as needed
        }
    }
}
```

## Validation Best Practices

1. **Validate Early**: At system boundaries
2. **Fail Fast**: Return errors immediately
3. **Be Specific**: Provide clear error messages
4. **Whitelist, Don't Blacklist**: Define what's allowed
5. **Layer Defense**: Validate at multiple levels
6. **Log Violations**: Track validation failures

## Common Mistakes

- Trusting client-side validation
- Not validating after deserialization
- Weak regex patterns
- Not checking array/collection sizes
- Forgetting to validate optional fields
- Not escaping output data