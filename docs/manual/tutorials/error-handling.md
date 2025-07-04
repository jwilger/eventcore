# Tutorial: Handling Errors Properly

Error handling in EventCore is designed to be comprehensive, actionable, and type-safe. This tutorial covers best practices for handling different types of errors and building resilient event-sourced systems.

## Error Philosophy

EventCore follows these error handling principles:

1. **Make errors explicit** - Use Result types, never panic in business logic
2. **Provide actionable information** - Include context that helps developers fix issues  
3. **Categorize errors by handling strategy** - Different error types require different responses
4. **Enhanced diagnostics** - Use miette for rich, user-friendly error messages

## Prerequisites

```toml
[dependencies]
eventcore = "0.1"
eventcore-memory = "0.1"
async-trait = "0.1"
miette = "5.0"  # For enhanced error reporting
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
```

## Error Categories in EventCore

### 1. Command Errors (`CommandError`)

These represent business logic violations and execution failures:

```rust
use eventcore::prelude::*;
use eventcore::miette::{Diagnostic, Report};

// Different types of command errors and how to handle them
async fn handle_command_errors() -> Result<(), Box<dyn std::error::Error>> {
    match execute_some_command().await {
        Err(CommandError::BusinessRuleViolation(msg)) => {
            // Business rule violations should usually NOT be retried
            eprintln!("❌ Business rule violated: {}", msg);
            // Log for analytics, notify user, etc.
            Err(msg.into())
        }
        
        Err(CommandError::ConcurrencyConflict { streams }) => {
            // Concurrency conflicts CAN be retried with backoff
            eprintln!("⚠️  Concurrency conflict on streams: {:?}", streams);
            eprintln!("💡 This is usually resolved by retrying");
            Err("Concurrency conflict - please retry".into())
        }
        
        Err(CommandError::InvalidStreamAccess { stream, declared_streams }) => {
            // Programming errors - should be fixed in code, not retried
            eprintln!("❌ PROGRAMMING ERROR: Invalid stream access");
            eprintln!("   Attempted to access: {}", stream);
            eprintln!("   Declared streams: {:?}", declared_streams);
            eprintln!("   💡 Fix: Add '{}' to your command's read_streams() method", stream);
            Err("Programming error - invalid stream access".into())
        }
        
        Err(CommandError::StreamNotDeclared { stream, command_type }) => {
            // Another programming error
            eprintln!("❌ PROGRAMMING ERROR: Stream not declared");
            eprintln!("   Stream: {}", stream);
            eprintln!("   Command: {}", command_type);
            eprintln!("   💡 Fix: Add stream to read_streams() method");
            Err("Programming error - stream not declared".into())
        }
        
        Err(CommandError::ValidationFailed(msg)) => {
            // Input validation failure - usually a client error
            eprintln!("❌ Input validation failed: {}", msg);
            eprintln!("💡 Check your input data and try again");
            Err(format!("Invalid input: {}", msg).into())
        }
        
        Err(CommandError::EventStore(store_error)) => {
            // Delegate to event store error handling
            handle_event_store_error(store_error).await
        }
        
        Ok(result) => {
            println!("✅ Command executed successfully");
            Ok(())
        }
    }
}

async fn execute_some_command() -> CommandResult<()> {
    // Placeholder - your actual command execution
    Ok(())
}
```

### 2. Event Store Errors (`EventStoreError`)

These represent storage layer issues:

```rust
async fn handle_event_store_error(error: EventStoreError) -> Result<(), Box<dyn std::error::Error>> {
    match error {
        EventStoreError::ConnectionFailed(msg) => {
            // Network/connection issues - CAN be retried
            eprintln!("🔌 Connection failed: {}", msg);
            eprintln!("💡 This might be temporary - consider retrying");
            Err("Connection failed - try again later".into())
        }
        
        EventStoreError::VersionConflict { expected, actual } => {
            // Optimistic concurrency failure - CAN be retried
            eprintln!("⚔️  Version conflict - expected {}, got {}", expected, actual);
            eprintln!("💡 Another process modified the stream - retry to get latest state");
            Err("Version conflict - please retry".into())
        }
        
        EventStoreError::StreamNotFound(stream_id) => {
            // Might be a business logic issue or race condition
            eprintln!("📭 Stream not found: {}", stream_id);
            eprintln!("💡 Check if the stream should exist or create it first");
            Err(format!("Stream {} not found", stream_id).into())
        }
        
        EventStoreError::SerializationFailed(msg) => {
            // Programming error - event can't be serialized
            eprintln!("❌ PROGRAMMING ERROR: Serialization failed");
            eprintln!("   Error: {}", msg);
            eprintln!("   💡 Fix: Check your event types implement Serialize correctly");
            Err("Programming error - serialization failed".into())
        }
        
        EventStoreError::DeserializationFailed(msg) => {
            // Data corruption or schema evolution issue
            eprintln!("❌ DATA ERROR: Deserialization failed");
            eprintln!("   Error: {}", msg);
            eprintln!("   💡 This might be a schema evolution issue");
            Err("Data error - deserialization failed".into())
        }
        
        EventStoreError::TransactionFailed(msg) => {
            // Database transaction failure - might be retryable
            eprintln!("💾 Transaction failed: {}", msg);
            eprintln!("💡 This might be temporary - consider retrying");
            Err("Transaction failed - try again".into())
        }
        
        EventStoreError::Other(msg) => {
            // Unknown error - log for investigation
            eprintln!("❓ Unknown event store error: {}", msg);
            Err(format!("Event store error: {}", msg).into())
        }
    }
}
```

### 3. Projection Errors (`ProjectionError`)

These occur during read model processing:

```rust
async fn handle_projection_error(error: ProjectionError) -> Result<(), Box<dyn std::error::Error>> {
    match error {
        ProjectionError::ProcessingError(msg) => {
            eprintln!("⚙️  Projection processing error: {}", msg);
            eprintln!("💡 The projection might need to be rebuilt");
            Err("Projection processing failed".into())
        }
        
        ProjectionError::CheckpointError(msg) => {
            eprintln!("📍 Checkpoint error: {}", msg);
            eprintln!("💡 Projection might lose some progress");
            Err("Checkpoint failed".into())
        }
        
        ProjectionError::StateCorruption(msg) => {
            eprintln!("💥 CRITICAL: Projection state corrupted");
            eprintln!("   Error: {}", msg);
            eprintln!("   💡 Projection MUST be rebuilt from scratch");
            Err("Critical: projection state corrupted".into())
        }
    }
}
```

## Retry Strategies

### Basic Exponential Backoff

```rust
use std::time::Duration;
use tokio::time::sleep;

async fn execute_with_basic_retry<T>(
    operation: impl Fn() -> CommandResult<T>,
    max_attempts: u32,
) -> CommandResult<T> {
    let mut attempts = 0;
    
    loop {
        match operation() {
            Ok(result) => return Ok(result),
            Err(error) => {
                attempts += 1;
                
                if attempts >= max_attempts {
                    return Err(error);
                }
                
                // Only retry certain types of errors
                if !should_retry(&error) {
                    return Err(error);
                }
                
                let delay = Duration::from_millis(100 * 2_u64.pow(attempts - 1));
                eprintln!("🔄 Attempt {} failed, retrying in {:?}", attempts, delay);
                sleep(delay).await;
            }
        }
    }
}

fn should_retry(error: &CommandError) -> bool {
    match error {
        CommandError::ConcurrencyConflict { .. } => true,
        CommandError::EventStore(EventStoreError::ConnectionFailed(_)) => true,
        CommandError::EventStore(EventStoreError::VersionConflict { .. }) => true,
        CommandError::EventStore(EventStoreError::TransactionFailed(_)) => true,
        
        // Don't retry business rule violations or programming errors
        CommandError::BusinessRuleViolation(_) => false,
        CommandError::ValidationFailed(_) => false,
        CommandError::InvalidStreamAccess { .. } => false,
        CommandError::StreamNotDeclared { .. } => false,
        
        _ => false,
    }
}
```

### Using EventCore's Built-in Retry

```rust
use eventcore::{RetryConfig, RetryPolicy};

async fn execute_with_eventcore_retry() -> Result<(), Box<dyn std::error::Error>> {
    let event_store = InMemoryEventStore::<BankEvent>::new();
    let executor = CommandExecutor::new(event_store);
    
    // Configure retry behavior
    let retry_config = RetryConfig {
        max_attempts: 5,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(30),
        backoff_multiplier: 2.0,
    };
    
    // Execute with retry
    let result = executor.execute_with_retry(
        &TransferMoney,
        transfer_input,
        retry_config,
        RetryPolicy::ConcurrencyAndTransient,
    ).await?;
    
    println!("✅ Command executed with retry: {} events", result.events_written.len());
    Ok(())
}
```

### Custom Retry Policy

```rust
fn custom_retry_policy(error: &CommandError) -> bool {
    match error {
        // Retry concurrency conflicts
        CommandError::ConcurrencyConflict { .. } => true,
        
        // Retry connection issues
        CommandError::EventStore(EventStoreError::ConnectionFailed(_)) => true,
        
        // Retry transaction failures, but only a few times
        CommandError::EventStore(EventStoreError::TransactionFailed(_)) => true,
        
        // Never retry business rule violations
        CommandError::BusinessRuleViolation(_) => false,
        
        // Never retry programming errors
        CommandError::InvalidStreamAccess { .. } => false,
        CommandError::StreamNotDeclared { .. } => false,
        
        _ => false,
    }
}

async fn execute_with_custom_retry() -> CommandResult<()> {
    let retry_config = RetryConfig::default();
    let retry_policy = RetryPolicy::Custom(custom_retry_policy);
    
    // Use the custom policy
    executor.execute_with_retry(
        &command,
        input,
        retry_config,
        retry_policy,
    ).await
}
```

## Enhanced Error Reporting with Miette

EventCore integrates with miette for beautiful, actionable error messages:

```rust
use miette::{Diagnostic, Report};

// All EventCore errors implement miette::Diagnostic
async fn demonstrate_enhanced_errors() {
    let result = execute_command().await;
    
    if let Err(error) = result {
        // miette provides rich, formatted error output
        let report = Report::new(error);
        eprintln!("{:?}", report);
        
        // The output includes:
        // - Clear error description
        // - Helpful hints for fixing the issue
        // - Context about what went wrong
        // - Suggestions for next steps
    }
}

// Creating custom diagnostic errors
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("Transfer validation failed")]
#[diagnostic(
    code(transfer::validation_failed),
    help("Check that both accounts exist and have sufficient funds"),
    url("https://docs.example.com/transfers#validation")
)]
struct TransferValidationError {
    #[source_code]
    input: String,
    
    #[label("This account doesn't exist")]
    missing_account_span: miette::SourceSpan,
    
    #[label("Insufficient funds here")]
    insufficient_funds_span: Option<miette::SourceSpan>,
}
```

## Error Recovery Patterns

### Circuit Breaker Pattern

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CircuitBreaker {
    failure_count: Arc<AtomicU32>,
    is_open: Arc<AtomicBool>,
    last_failure: Arc<std::sync::Mutex<Option<Instant>>>,
    failure_threshold: u32,
    recovery_timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_count: Arc::new(AtomicU32::new(0)),
            is_open: Arc::new(AtomicBool::new(false)),
            last_failure: Arc::new(std::sync::Mutex::new(None)),
            failure_threshold,
            recovery_timeout,
        }
    }
    
    pub async fn call<T, F, Fut>(&self, operation: F) -> Result<T, CircuitBreakerError<CommandError>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CommandResult<T>>,
    {
        // Check if circuit is open
        if self.is_circuit_open() {
            return Err(CircuitBreakerError::CircuitOpen);
        }
        
        match operation().await {
            Ok(result) => {
                self.on_success();
                Ok(result)
            }
            Err(error) => {
                if should_count_as_failure(&error) {
                    self.on_failure();
                }
                Err(CircuitBreakerError::OperationFailed(error))
            }
        }
    }
    
    fn is_circuit_open(&self) -> bool {
        if !self.is_open.load(Ordering::Relaxed) {
            return false;
        }
        
        // Check if we should try to recover
        if let Ok(last_failure) = self.last_failure.lock() {
            if let Some(last) = *last_failure {
                if last.elapsed() > self.recovery_timeout {
                    self.is_open.store(false, Ordering::Relaxed);
                    self.failure_count.store(0, Ordering::Relaxed);
                    return false;
                }
            }
        }
        
        true
    }
    
    fn on_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        self.is_open.store(false, Ordering::Relaxed);
    }
    
    fn on_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        
        if failures >= self.failure_threshold {
            self.is_open.store(true, Ordering::Relaxed);
            if let Ok(mut last_failure) = self.last_failure.lock() {
                *last_failure = Some(Instant::now());
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open")]
    CircuitOpen,
    #[error("Operation failed: {0}")]
    OperationFailed(E),
}

fn should_count_as_failure(error: &CommandError) -> bool {
    match error {
        // Count connection failures
        CommandError::EventStore(EventStoreError::ConnectionFailed(_)) => true,
        CommandError::EventStore(EventStoreError::TransactionFailed(_)) => true,
        
        // Don't count business logic errors
        CommandError::BusinessRuleViolation(_) => false,
        CommandError::ValidationFailed(_) => false,
        
        _ => true,
    }
}
```

### Bulkhead Pattern

```rust
use tokio::sync::Semaphore;

pub struct BulkheadExecutor {
    command_semaphore: Arc<Semaphore>,
    query_semaphore: Arc<Semaphore>,
}

impl BulkheadExecutor {
    pub fn new(command_permits: usize, query_permits: usize) -> Self {
        Self {
            command_semaphore: Arc::new(Semaphore::new(command_permits)),
            query_semaphore: Arc::new(Semaphore::new(query_permits)),
        }
    }
    
    pub async fn execute_command<C, I>(&self, command: &C, input: I) -> CommandResult<()>
    where
        C: Command<Input = I>,
        I: Send + Sync + Clone,
    {
        let _permit = self.command_semaphore.acquire().await
            .map_err(|_| CommandError::Other("Failed to acquire command permit".to_string()))?;
            
        // Execute command with limited concurrency
        // This protects the system from being overwhelmed
        execute_actual_command(command, input).await
    }
    
    pub async fn execute_query<C, I>(&self, query: &C, input: I) -> CommandResult<()>
    where
        C: Command<Input = I>,
        I: Send + Sync + Clone,
    {
        let _permit = self.query_semaphore.acquire().await
            .map_err(|_| CommandError::Other("Failed to acquire query permit".to_string()))?;
            
        // Execute query with separate resource pool
        execute_actual_command(query, input).await
    }
}

async fn execute_actual_command<C, I>(command: &C, input: I) -> CommandResult<()>
where
    C: Command<Input = I>,
    I: Send + Sync + Clone,
{
    // Placeholder for actual command execution
    Ok(())
}
```

## Monitoring and Observability

### Error Metrics

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;

#[derive(Default)]
pub struct ErrorMetrics {
    business_rule_violations: AtomicU64,
    concurrency_conflicts: AtomicU64,
    connection_failures: AtomicU64,
    validation_failures: AtomicU64,
    programming_errors: AtomicU64,
}

impl ErrorMetrics {
    pub fn record_error(&self, error: &CommandError) {
        match error {
            CommandError::BusinessRuleViolation(_) => {
                self.business_rule_violations.fetch_add(1, Ordering::Relaxed);
            }
            CommandError::ConcurrencyConflict { .. } => {
                self.concurrency_conflicts.fetch_add(1, Ordering::Relaxed);
            }
            CommandError::EventStore(EventStoreError::ConnectionFailed(_)) => {
                self.connection_failures.fetch_add(1, Ordering::Relaxed);
            }
            CommandError::ValidationFailed(_) => {
                self.validation_failures.fetch_add(1, Ordering::Relaxed);
            }
            CommandError::InvalidStreamAccess { .. } |
            CommandError::StreamNotDeclared { .. } => {
                self.programming_errors.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    
    pub fn get_metrics(&self) -> HashMap<String, u64> {
        let mut metrics = HashMap::new();
        metrics.insert("business_rule_violations".to_string(), 
                      self.business_rule_violations.load(Ordering::Relaxed));
        metrics.insert("concurrency_conflicts".to_string(), 
                      self.concurrency_conflicts.load(Ordering::Relaxed));
        metrics.insert("connection_failures".to_string(), 
                      self.connection_failures.load(Ordering::Relaxed));
        metrics.insert("validation_failures".to_string(), 
                      self.validation_failures.load(Ordering::Relaxed));
        metrics.insert("programming_errors".to_string(), 
                      self.programming_errors.load(Ordering::Relaxed));
        metrics
    }
}
```

### Structured Error Logging

```rust
use tracing::{error, warn, info, instrument};
use serde_json::json;

#[instrument(fields(command_type, error_type))]
async fn execute_with_logging<C, I>(
    command: &C, 
    input: I,
    metrics: &ErrorMetrics
) -> CommandResult<()>
where
    C: Command<Input = I>,
    I: Send + Sync + Clone,
{
    let command_type = std::any::type_name::<C>();
    
    match execute_command(command, input).await {
        Ok(result) => {
            info!(
                command_type = command_type,
                events_written = result.events_written.len(),
                "Command executed successfully"
            );
            Ok(())
        }
        Err(error) => {
            let error_type = match &error {
                CommandError::BusinessRuleViolation(_) => "business_rule_violation",
                CommandError::ConcurrencyConflict { .. } => "concurrency_conflict", 
                CommandError::ValidationFailed(_) => "validation_failed",
                CommandError::InvalidStreamAccess { .. } => "invalid_stream_access",
                CommandError::StreamNotDeclared { .. } => "stream_not_declared",
                CommandError::EventStore(_) => "event_store_error",
                _ => "other_error",
            };
            
            // Record metrics
            metrics.record_error(&error);
            
            // Log with appropriate level
            match &error {
                CommandError::BusinessRuleViolation(_) |
                CommandError::ValidationFailed(_) => {
                    // These are expected user errors
                    warn!(
                        command_type = command_type,
                        error_type = error_type,
                        error = %error,
                        "Command failed due to business rules or validation"
                    );
                }
                CommandError::InvalidStreamAccess { .. } |
                CommandError::StreamNotDeclared { .. } => {
                    // These are programming errors - should be fixed
                    error!(
                        command_type = command_type,
                        error_type = error_type,
                        error = %error,
                        "PROGRAMMING ERROR in command implementation"
                    );
                }
                _ => {
                    // Infrastructure or transient errors
                    error!(
                        command_type = command_type,
                        error_type = error_type,
                        error = %error,
                        "Command failed due to infrastructure error"
                    );
                }
            }
            
            Err(error)
        }
    }
}

async fn execute_command<C, I>(_command: &C, _input: I) -> CommandResult<CommandExecutionResult>
where
    C: Command<Input = I>,
    I: Send + Sync + Clone,
{
    // Placeholder
    Ok(CommandExecutionResult { events_written: vec![] })
}

struct CommandExecutionResult {
    events_written: Vec<()>,
}
```

## Testing Error Scenarios

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_business_rule_violation() {
        let result = execute_transfer_with_insufficient_funds().await;
        
        match result {
            Err(CommandError::BusinessRuleViolation(msg)) => {
                assert!(msg.contains("insufficient funds"));
            }
            _ => panic!("Expected BusinessRuleViolation"),
        }
    }
    
    #[tokio::test]
    async fn test_concurrency_conflict_retry() {
        let mut attempts = 0;
        let result = execute_with_basic_retry(|| {
            attempts += 1;
            if attempts < 3 {
                Err(CommandError::ConcurrencyConflict { 
                    streams: vec![StreamId::try_new("test").unwrap()] 
                })
            } else {
                Ok(())
            }
        }, 5).await;
        
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
    }
    
    #[test]
    fn test_error_metrics() {
        let metrics = ErrorMetrics::default();
        
        let error = CommandError::BusinessRuleViolation("test".to_string());
        metrics.record_error(&error);
        
        let counts = metrics.get_metrics();
        assert_eq!(counts.get("business_rule_violations"), Some(&1));
    }
}
```

## Best Practices Summary

1. **Use appropriate error types** - Don't retry business rule violations
2. **Implement exponential backoff** - For transient failures
3. **Monitor error patterns** - Track metrics to identify issues
4. **Use circuit breakers** - Protect against cascading failures
5. **Log errors with context** - Include relevant information for debugging
6. **Test error scenarios** - Ensure your error handling works correctly
7. **Provide actionable error messages** - Help users understand what went wrong and how to fix it

Error handling in EventCore is about building resilient systems that gracefully handle both expected business conditions and unexpected infrastructure failures.