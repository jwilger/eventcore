# Chapter 7.3: Error Reference

This chapter provides a comprehensive reference for all EventCore error types, error codes, and troubleshooting guidance. Use this reference to understand and resolve errors in your EventCore applications.

## Error Categories

EventCore errors are organized into several categories based on their origin and nature:

1. **Command Errors** - Errors during command execution
2. **Event Store Errors** - Errors from event store operations
3. **Projection Errors** - Errors in projection processing
4. **Validation Errors** - Input validation failures
5. **Configuration Errors** - Configuration and setup issues
6. **Network Errors** - Network and connectivity issues
7. **Serialization Errors** - Data serialization/deserialization issues

## Command Errors

### CommandError

The primary error type for command execution failures.

```rust
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Validation failed: {message}")]
    ValidationFailed { message: String },

    #[error("Business rule violation: {rule} - {message}")]
    BusinessRuleViolation { rule: String, message: String },

    #[error("Concurrency conflict on streams: {streams:?}")]
    ConcurrencyConflict { streams: Vec<StreamId> },

    #[error("Stream not found: {stream_id}")]
    StreamNotFound { stream_id: StreamId },

    #[error("Unauthorized: {permission}")]
    Unauthorized { permission: String },

    #[error("Timeout after {duration:?}")]
    Timeout { duration: Duration },

    #[error("Stream access denied: cannot write to {stream_id}")]
    StreamAccessDenied { stream_id: StreamId },

    #[error("Maximum discovery iterations exceeded: {iterations}")]
    MaxIterationsExceeded { iterations: usize },

    #[error("Event store error: {source}")]
    EventStoreError {
        #[from]
        source: EventStoreError
    },

    #[error("Serialization error: {message}")]
    SerializationError { message: String },

    #[error("Internal error: {message}")]
    InternalError { message: String },
}
```

#### Error Codes and Solutions

**CE001: ValidationFailed**

```
Error: Validation failed: StreamId cannot be empty
Code: CE001
```

**Cause:** Input validation failed during command construction or execution.
**Solution:**

- Check input parameters for correct format and constraints
- Ensure all required fields are provided
- Verify string lengths and format requirements

**CE002: BusinessRuleViolation**

```
Error: Business rule violation: insufficient_balance - Account balance $100.00 is less than transfer amount $150.00
Code: CE002
```

**Cause:** Business logic constraints were violated.
**Solution:**

- Review business rules and ensure command logic respects them
- Check application state before executing commands
- Implement proper validation in command handlers

**CE003: ConcurrencyConflict**

```
Error: Concurrency conflict on streams: ["account-123", "account-456"]
Code: CE003
```

**Cause:** Multiple commands attempted to modify the same streams simultaneously.
**Solution:**

- Implement retry logic with exponential backoff
- Consider command design to reduce conflicts
- Use optimistic concurrency control patterns

**CE004: StreamNotFound**

```
Error: Stream not found: account-nonexistent
Code: CE004
```

**Cause:** Command attempted to read from a stream that doesn't exist.
**Solution:**

- Verify stream IDs are correct
- Check if the resource exists before referencing it
- Implement proper error handling for missing resources

**CE005: Unauthorized**

```
Error: Unauthorized: write_account_events
Code: CE005
```

**Cause:** Insufficient permissions to execute the command.
**Solution:**

- Verify user authentication and authorization
- Check role-based access control configuration
- Ensure proper security context is set

**CE006: Timeout**

```
Error: Timeout after 30s
Code: CE006
```

**Cause:** Command execution exceeded configured timeout.
**Solution:**

- Check system performance and database connectivity
- Increase timeout configuration if appropriate
- Optimize command logic and database queries

**CE007: StreamAccessDenied**

```
Error: Stream access denied: cannot write to protected-stream-123
Code: CE007
```

**Cause:** Command attempted to write to a stream it didn't declare.
**Solution:**

- Add the stream to the command's `read_streams()` method
- Verify command design follows EventCore stream access patterns
- Check for typos in stream ID generation

**CE008: MaxIterationsExceeded**

```
Error: Maximum discovery iterations exceeded: 10
Code: CE008
```

**Cause:** Stream discovery loop exceeded configured maximum iterations.
**Solution:**

- Review command logic for potential infinite discovery loops
- Increase `max_discovery_iterations` if legitimate
- Optimize stream discovery patterns

### Command Execution Flow Errors

These errors occur during specific phases of command execution:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ExecutionPhaseError {
    #[error("Stream reading failed: {message}")]
    StreamReadError { message: String },

    #[error("State reconstruction failed: {message}")]
    StateReconstructionError { message: String },

    #[error("Command handling failed: {message}")]
    CommandHandlingError { message: String },

    #[error("Event writing failed: {message}")]
    EventWritingError { message: String },

    #[error("Stream discovery failed: {message}")]
    StreamDiscoveryError { message: String },
}
```

## Event Store Errors

### EventStoreError

Errors from event store operations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("Version conflict: expected {expected:?}, got {actual}")]
    VersionConflict {
        expected: ExpectedVersion,
        actual: EventVersion,
    },

    #[error("Stream not found: {stream_id}")]
    StreamNotFound { stream_id: StreamId },

    #[error("Connection failed: {message}")]
    ConnectionFailed { message: String },

    #[error("Database error: {source}")]
    DatabaseError {
        #[from]
        source: sqlx::Error,
    },

    #[error("Serialization error: {message}")]
    SerializationError { message: String },

    #[error("Transaction failed: {message}")]
    TransactionError { message: String },

    #[error("Migration error: {message}")]
    MigrationError { message: String },

    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },

    #[error("Timeout error: operation timed out after {duration:?}")]
    TimeoutError { duration: Duration },

    #[error("Connection pool exhausted")]
    ConnectionPoolExhausted,

    #[error("Invalid event data: {message}")]
    InvalidEventData { message: String },
}
```

#### Error Codes and Solutions

**ES001: VersionConflict**

```
Error: Version conflict: expected Exact(5), got 7
Code: ES001
```

**Cause:** Optimistic concurrency control detected concurrent modification.
**Solution:**

- Implement retry logic in command execution
- Consider command design to reduce conflicts
- Use appropriate `ExpectedVersion` strategy

**ES002: ConnectionFailed**

```
Error: Connection failed: Failed to connect to database at postgresql://localhost/eventcore
Code: ES002
```

**Cause:** Unable to establish database connection.
**Solution:**

- Verify database is running and accessible
- Check connection string configuration
- Verify network connectivity and firewall rules

**ES003: ConnectionPoolExhausted**

```
Error: Connection pool exhausted
Code: ES003
```

**Cause:** All database connections in the pool are in use.
**Solution:**

- Increase `max_connections` in pool configuration
- Check for connection leaks in application code
- Monitor connection usage patterns

**ES004: TransactionError**

```
Error: Transaction failed: serialization failure
Code: ES004
```

**Cause:** Database transaction could not be completed due to conflicts.
**Solution:**

- Implement transaction retry logic
- Review transaction isolation levels
- Consider reducing transaction scope

**ES005: MigrationError**

```
Error: Migration error: Migration 20231201_001_create_events failed
Code: ES005
```

**Cause:** Database migration failed during startup.
**Solution:**

- Check database permissions
- Verify migration scripts are valid
- Review database schema state

### PostgreSQL-Specific Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum PostgresError {
    #[error("Unique constraint violation: {constraint}")]
    UniqueConstraintViolation { constraint: String },

    #[error("Foreign key constraint violation: {constraint}")]
    ForeignKeyViolation { constraint: String },

    #[error("Check constraint violation: {constraint}")]
    CheckConstraintViolation { constraint: String },

    #[error("Deadlock detected: {message}")]
    DeadlockDetected { message: String },

    #[error("Query timeout: query exceeded {timeout:?}")]
    QueryTimeout { timeout: Duration },

    #[error("Connection limit exceeded")]
    ConnectionLimitExceeded,
}
```

## Projection Errors

### ProjectionError

Errors from projection operations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("Projection not found: {name}")]
    NotFound { name: String },

    #[error("Projection already exists: {name}")]
    AlreadyExists { name: String },

    #[error("Event processing failed: {message}")]
    ProcessingFailed { message: String },

    #[error("Checkpoint save failed: {message}")]
    CheckpointFailed { message: String },

    #[error("Rebuild failed: {message}")]
    RebuildFailed { message: String },

    #[error("Subscription error: {message}")]
    SubscriptionError { message: String },

    #[error("State corruption detected: {message}")]
    StateCorruption { message: String },

    #[error("Projection timeout: {projection} timed out after {duration:?}")]
    Timeout { projection: String, duration: Duration },

    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },
}
```

#### Error Codes and Solutions

**PR001: ProcessingFailed**

```
Error: Event processing failed: Failed to apply UserCreated event
Code: PR001
```

**Cause:** Projection failed to process an event.
**Solution:**

- Check projection logic for errors
- Verify event format matches expectations
- Implement proper error handling in projections

**PR002: CheckpointFailed**

```
Error: Checkpoint save failed: Database connection lost
Code: PR002
```

**Cause:** Unable to save projection checkpoint.
**Solution:**

- Check database connectivity
- Verify checkpoint storage configuration
- Implement checkpoint retry logic

**PR003: RebuildFailed**

```
Error: Rebuild failed: Out of memory during rebuild
Code: PR003
```

**Cause:** Projection rebuild encountered an error.
**Solution:**

- Increase memory allocation for rebuild operations
- Implement incremental rebuild strategies
- Check for memory leaks in projection code

**PR004: StateCorruption**

```
Error: State corruption detected: Checksum mismatch
Code: PR004
```

**Cause:** Projection state integrity check failed.
**Solution:**

- Rebuild projection from beginning
- Investigate potential data corruption causes
- Verify checkpoint storage integrity

## Validation Errors

### ValidationError

Input validation errors from the `nutype` validation system.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Required field is empty: {field}")]
    Empty { field: String },

    #[error("Value too long: {field} length {length} exceeds maximum {max}")]
    TooLong { field: String, length: usize, max: usize },

    #[error("Value too short: {field} length {length} below minimum {min}")]
    TooShort { field: String, length: usize, min: usize },

    #[error("Invalid format: {field} does not match expected format")]
    InvalidFormat { field: String },

    #[error("Invalid range: {field} value {value} outside range [{min}, {max}]")]
    OutOfRange { field: String, value: String, min: String, max: String },

    #[error("Predicate failed: {field} failed validation rule")]
    PredicateFailed { field: String },

    #[error("Parse error: {field} could not be parsed - {message}")]
    ParseError { field: String, message: String },
}
```

#### Error Codes and Solutions

**VE001: Empty**

```
Error: Required field is empty: stream_id
Code: VE001
```

**Cause:** Required field was empty or contained only whitespace.
**Solution:**

- Ensure all required fields have values
- Check for null or empty string inputs
- Verify string trimming behavior

**VE002: TooLong**

```
Error: Value too long: stream_id length 300 exceeds maximum 255
Code: VE002
```

**Cause:** Input value exceeded maximum length constraint.
**Solution:**

- Reduce input length to meet constraints
- Consider using shorter identifiers
- Review length requirements

**VE003: InvalidFormat**

```
Error: Invalid format: email does not match expected format
Code: VE003
```

**Cause:** Input value didn't match expected format pattern.
**Solution:**

- Verify input format matches requirements
- Check regular expression patterns
- Validate input on client side before submission

## Configuration Errors

### ConfigError

Configuration and setup errors.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required configuration: {key}")]
    MissingRequired { key: String },

    #[error("Invalid configuration value: {key} = {value}")]
    InvalidValue { key: String, value: String },

    #[error("Configuration file not found: {path}")]
    FileNotFound { path: String },

    #[error("Configuration parse error: {message}")]
    ParseError { message: String },

    #[error("Environment variable error: {message}")]
    EnvironmentError { message: String },

    #[error("Validation error: {message}")]
    ValidationError { message: String },
}
```

#### Error Codes and Solutions

**CF001: MissingRequired**

```
Error: Missing required configuration: DATABASE_URL
Code: CF001
```

**Cause:** Required configuration parameter not provided.
**Solution:**

- Set missing environment variable or configuration value
- Check configuration file completeness
- Verify environment setup

**CF002: InvalidValue**

```
Error: Invalid configuration value: MAX_CONNECTIONS = -5
Code: CF002
```

**Cause:** Configuration value is invalid for the parameter type.
**Solution:**

- Check value format and type requirements
- Verify numeric ranges and constraints
- Review configuration documentation

## Network Errors

### NetworkError

Network and connectivity related errors.

```rust
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Connection timeout: {endpoint}")]
    ConnectionTimeout { endpoint: String },

    #[error("DNS resolution failed: {hostname}")]
    DnsResolutionFailed { hostname: String },

    #[error("TLS error: {message}")]
    TlsError { message: String },

    #[error("HTTP error: {status} - {message}")]
    HttpError { status: u16, message: String },

    #[error("Network unreachable: {endpoint}")]
    NetworkUnreachable { endpoint: String },

    #[error("Connection refused: {endpoint}")]
    ConnectionRefused { endpoint: String },
}
```

## Serialization Errors

### SerializationError

Data serialization and deserialization errors.

```rust
#[derive(Debug, thiserror::Error)]
pub enum SerializationError {
    #[error("JSON serialization failed: {message}")]
    JsonSerializationFailed { message: String },

    #[error("JSON deserialization failed: {message}")]
    JsonDeserializationFailed { message: String },

    #[error("Invalid JSON format: {message}")]
    InvalidJsonFormat { message: String },

    #[error("Missing required field: {field}")]
    MissingField { field: String },

    #[error("Unknown field: {field}")]
    UnknownField { field: String },

    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: String, found: String },

    #[error("Schema version mismatch: expected {expected}, found {found}")]
    SchemaVersionMismatch { expected: String, found: String },
}
```

## Error Handling Patterns

### Retry Strategies

EventCore provides different retry strategies for different error types:

```rust
// Automatic retry for transient errors
match command_executor.execute(&command).await {
    Ok(result) => result,
    Err(CommandError::ConcurrencyConflict { .. }) => {
        // Retry with exponential backoff
        retry_with_backoff(|| command_executor.execute(&command)).await?
    },
    Err(CommandError::Timeout { .. }) => {
        // Retry with different timeout
        command_executor.execute_with_timeout(&command, increased_timeout).await?
    },
    Err(other) => return Err(other), // Don't retry business logic errors
}
```

### Error Conversion

Common error conversion patterns:

```rust
// Convert EventStore errors to Command errors
impl From<EventStoreError> for CommandError {
    fn from(err: EventStoreError) -> Self {
        match err {
            EventStoreError::VersionConflict { .. } => {
                CommandError::ConcurrencyConflict { streams: vec![] }
            },
            EventStoreError::StreamNotFound { stream_id } => {
                CommandError::StreamNotFound { stream_id }
            },
            other => CommandError::EventStoreError { source: other },
        }
    }
}
```

### Error Context

Adding context to errors for better debugging:

```rust
use anyhow::{Context, Result};

async fn execute_command<C: Command>(command: &C) -> Result<ExecutionResult> {
    command_executor
        .execute(command)
        .await
        .with_context(|| format!("Failed to execute command: {}", std::any::type_name::<C>()))
        .with_context(|| "Command execution failed in main handler")
}
```

## Troubleshooting Guide

### Quick Reference

**Performance Issues:**

1. Check `CE006: Timeout` errors → Review system performance
2. Check `ES003: ConnectionPoolExhausted` → Increase pool size or fix leaks
3. Check `PR003: RebuildFailed` → Optimize memory usage

**Data Issues:**

1. Check `CE003: ConcurrencyConflict` → Implement retry logic
2. Check `ES001: VersionConflict` → Review optimistic concurrency
3. Check `PR004: StateCorruption` → Rebuild projections

**Configuration Issues:**

1. Check `CF001: MissingRequired` → Set required configuration
2. Check `ES002: ConnectionFailed` → Verify database connectivity
3. Check `CF002: InvalidValue` → Review configuration values

**Security Issues:**

1. Check `CE005: Unauthorized` → Verify permissions
2. Check `CE007: StreamAccessDenied` → Fix stream access patterns

### Diagnostic Commands

```bash
# Check EventCore health
eventcore-cli health-check

# Validate configuration
eventcore-cli config validate

# Test database connectivity
eventcore-cli database ping

# Check projection status
eventcore-cli projections status

# Verify stream access
eventcore-cli commands validate <command-type>
```

### Log Analysis

Common log patterns to look for:

```bash
# High error rates
grep "ERROR" logs/eventcore.log | grep -c "CommandError"

# Concurrency conflicts
grep "ConcurrencyConflict" logs/eventcore.log | tail -10

# Performance issues
grep "Timeout\|slow query" logs/eventcore.log

# Connection issues
grep "ConnectionFailed\|ConnectionPoolExhausted" logs/eventcore.log
```

## Error Prevention

### Best Practices

1. **Input Validation:** Use type-safe domain types with validation
2. **Error Handling:** Implement comprehensive error handling strategies
3. **Monitoring:** Set up alerts for error rate thresholds
4. **Testing:** Include error scenarios in integration tests
5. **Documentation:** Document expected error conditions

### Type Safety

EventCore's type system prevents many error categories:

```rust
// Good: Type-safe stream access
#[derive(Command)]
struct TransferMoney {
    #[stream]
    source_account: StreamId,  // Guaranteed valid

    #[stream]
    target_account: StreamId,  // Guaranteed valid

    amount: Money,  // Guaranteed valid currency/amount
}

// Prevents: CE007 StreamAccessDenied, VE001-VE003 validation errors
```

This completes the error reference documentation. Use this guide to understand, diagnose, and resolve EventCore errors effectively.

Next, explore the [Glossary](./04-glossary.md) →
