# Chapter 3.5: Error Handling

Error handling in EventCore is designed to be explicit, recoverable, and informative. This chapter covers error types, handling strategies, and best practices for building resilient event-sourced systems.

## Error Philosophy

EventCore follows these principles:

1. **Errors are values** - Use `Result<T, E>` everywhere
2. **Be specific** - Different error types for different failures
3. **Fail fast** - Validate early in the command pipeline
4. **Recover gracefully** - Automatic retries for transient errors
5. **Provide context** - Rich error messages for debugging

## Error Types

### Command Errors

The main error type for command execution:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Business rule violation: {0}")]
    BusinessRuleViolation(String),

    #[error("Stream not found: {0}")]
    StreamNotFound(StreamId),

    #[error("Concurrency conflict on streams: {0:?}")]
    ConcurrencyConflict(Vec<StreamId>),

    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Maximum retries exceeded: {0}")]
    MaxRetriesExceeded(String),
}
```

### Event Store Errors

Storage-specific errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("Version conflict in stream {stream_id}: expected {expected:?}, actual {actual}")]
    VersionConflict {
        stream_id: StreamId,
        expected: ExpectedVersion,
        actual: EventVersion,
    },

    #[error("Stream {0} not found")]
    StreamNotFound(StreamId),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("Transaction rolled back: {0}")]
    TransactionRollback(String),
}
```

## Validation Patterns

### Using the `require!` Macro

The `require!` macro makes validation concise:

```rust
use eventcore::require;

async fn handle(
    &self,
    stream_declarations: StreamDeclarations<Self::StreamSet>,
    state: Self::State,
    _stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Simple validation
    require!(self.amount > 0, "Amount must be positive");

    // Validation with formatting
    require!(
        state.balance >= self.amount,
        "Insufficient balance: have {}, need {}",
        state.balance,
        self.amount
    );

    // Complex validation
    require!(
        state.account.is_active && !state.account.is_frozen,
        "Account must be active and not frozen"
    );

    // Continue with business logic...
    Ok(vec![/* events */])
}
```

### Custom Validation Functions

For complex validations:

```rust
impl TransferMoney {
    fn validate_business_rules(&self, state: &AccountState) -> CommandResult<()> {
        // Daily limit check
        self.validate_daily_limit(state)?;

        // Fraud check
        self.validate_fraud_rules(state)?;

        // Compliance check
        self.validate_compliance(state)?;

        Ok(())
    }

    fn validate_daily_limit(&self, state: &AccountState) -> CommandResult<()> {
        const DAILY_LIMIT: Money = Money::from_cents(50_000_00);

        let today_total = state.transfers_today() + self.amount;
        require!(
            today_total <= DAILY_LIMIT,
            "Daily transfer limit exceeded: {} > {}",
            today_total,
            DAILY_LIMIT
        );

        Ok(())
    }
}

// In handle()
async fn handle(/* ... */) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Run all validations
    self.validate_business_rules(&state)?;

    // Generate events...
}
```

### Type-Safe Validation

Use types to make invalid states unrepresentable:

```rust
use nutype::nutype;

// Email validation at type level
#[nutype(
    sanitize(lowercase, trim),
    validate(regex = r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"),
    derive(Debug, Clone, Serialize, Deserialize)
)]
pub struct Email(String);

// Money that can't be negative
#[nutype(
    validate(greater_or_equal = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)
)]
pub struct Money(u64);

// Now these validations happen at construction
let email = Email::try_new("invalid-email")?; // Fails at parse time
let amount = Money::try_new(-100)?; // Compile error - u64 can't be negative
```

## Handling Transient Errors

### Automatic Retries

EventCore automatically retries on version conflicts:

```rust
// This happens inside EventCore:
pub async fn execute_with_retry<C: Command>(
    command: &C,
    max_retries: usize,
) -> CommandResult<ExecutionResult> {
    let mut attempts = 0;

    loop {
        attempts += 1;

        match execute_once(command).await {
            Ok(result) => return Ok(result),

            Err(CommandError::ConcurrencyConflict(_)) if attempts < max_retries => {
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2_u64.pow(attempts as u32));
                tokio::time::sleep(delay).await;
                continue;
            }

            Err(e) => return Err(e),
        }
    }
}
```

### Circuit Breaker Pattern

Protect against cascading failures:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure_time: AtomicU64,
    threshold: u32,
    timeout: Duration,
}

impl CircuitBreaker {
    pub fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        // Check if circuit is open
        if self.is_open() {
            return Err(CircuitBreakerError::Open);
        }

        // Try the operation
        match f() {
            Ok(result) => {
                self.on_success();
                Ok(result)
            }
            Err(e) => {
                self.on_failure();
                Err(CircuitBreakerError::Failed(e))
            }
        }
    }

    fn is_open(&self) -> bool {
        let failures = self.failure_count.load(Ordering::Relaxed);
        if failures >= self.threshold {
            let last_failure = self.last_failure_time.load(Ordering::Relaxed);
            let elapsed = Duration::from_millis(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64 - last_failure
            );
            elapsed < self.timeout
        } else {
            false
        }
    }
}

// Usage in event store
impl PostgresEventStore {
    pub async fn read_stream_with_circuit_breaker(
        &self,
        stream_id: &StreamId,
    ) -> Result<StreamEvents, EventStoreError> {
        self.circuit_breaker.call(|| {
            self.read_stream_internal(stream_id).await
        })
    }
}
```

## Error Recovery Strategies

### Compensating Commands

When things go wrong, emit compensating events:

```rust
#[derive(Command, Clone)]
struct RefundPayment {
    #[stream]
    payment: StreamId,

    #[stream]
    account: StreamId,

    reason: RefundReason,
}

async fn handle(/* ... */) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Validate refund is possible
    require!(
        state.payment.status == PaymentStatus::Completed,
        "Can only refund completed payments"
    );

    require!(
        !state.payment.is_refunded,
        "Payment already refunded"
    );

    // Compensating events
    Ok(vec![
        StreamWrite::new(&stream_declarations, self.payment.clone(),
            PaymentEvent::Refunded {
                amount: state.payment.amount,
                reason: self.reason.clone(),
            })?,

        StreamWrite::new(&stream_declarations, self.account.clone(),
            AccountEvent::Credited {
                amount: state.payment.amount,
                reference: format!("Refund for payment {}", state.payment.id),
            })?,
    ])
}
```

### Dead Letter Queues

Handle permanently failed commands:

```rust
pub struct DeadLetterQueue<C: Command> {
    failed_commands: Vec<FailedCommand<C>>,
}

#[derive(Debug)]
pub struct FailedCommand<C> {
    pub command: C,
    pub error: CommandError,
    pub attempts: usize,
    pub first_attempted: DateTime<Utc>,
    pub last_attempted: DateTime<Utc>,
}

impl<C: Command> CommandExecutor<C> {
    pub async fn execute_with_dlq(
        &self,
        command: C,
        dlq: &mut DeadLetterQueue<C>,
    ) -> CommandResult<ExecutionResult> {
        match self.execute_with_retry(&command, 5).await {
            Ok(result) => Ok(result),
            Err(e) if e.is_permanent() => {
                // Add to DLQ for manual intervention
                dlq.add(FailedCommand {
                    command,
                    error: e.clone(),
                    attempts: 5,
                    first_attempted: Utc::now(),
                    last_attempted: Utc::now(),
                });
                Err(e)
            }
            Err(e) => Err(e),
        }
    }
}
```

## Error Context and Debugging

### Rich Error Context

Add context to errors:

```rust
use std::fmt;

#[derive(Debug)]
pub struct ErrorContext {
    pub command_type: &'static str,
    pub stream_ids: Vec<StreamId>,
    pub correlation_id: CorrelationId,
    pub user_id: Option<UserId>,
    pub additional_context: HashMap<String, String>,
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Command: {}, Streams: {:?}, Correlation: {}",
            self.command_type,
            self.stream_ids,
            self.correlation_id
        )?;

        if let Some(user) = &self.user_id {
            write!(f, ", User: {}", user)?;
        }

        for (key, value) in &self.additional_context {
            write!(f, ", {}: {}", key, value)?;
        }

        Ok(())
    }
}

// Wrap errors with context
pub type ContextualResult<T> = Result<T, ContextualError>;

#[derive(Debug, thiserror::Error)]
#[error("{context}\nError: {source}")]
pub struct ContextualError {
    #[source]
    source: CommandError,
    context: ErrorContext,
}
```

### Structured Logging

Log errors with full context:

```rust
use tracing::{error, warn, info, instrument};

#[instrument(skip(self, stream_declarations, state, stream_resolver))]
async fn handle(
    &self,
    stream_declarations: StreamDeclarations<Self::StreamSet>,
    state: Self::State,
    stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    info!(
        amount = %self.amount,
        from = %self.from_account,
        to = %self.to_account,
        "Processing transfer"
    );

    if let Err(e) = self.validate_business_rules(&state) {
        error!(
            error = %e,
            balance = %state.balance,
            daily_total = %state.transfers_today(),
            "Transfer validation failed"
        );
        return Err(e);
    }

    // Continue...
}
```

## Testing Error Scenarios

### Unit Tests for Validation

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insufficient_balance_error() {
        let command = TransferMoney {
            from_account: StreamId::from_static("account-1"),
            to_account: StreamId::from_static("account-2"),
            amount: Money::from_cents(1000),
        };

        let state = AccountState {
            balance: Money::from_cents(500),
            ..Default::default()
        };

        let result = command.validate_business_rules(&state);

        assert!(matches!(
            result,
            Err(CommandError::ValidationFailed(msg)) if msg.contains("Insufficient balance")
        ));
    }

    #[tokio::test]
    async fn test_daily_limit_exceeded() {
        let command = TransferMoney {
            from_account: StreamId::from_static("account-1"),
            to_account: StreamId::from_static("account-2"),
            amount: Money::from_cents(10_000),
        };

        let mut state = AccountState::default();
        state.add_todays_transfer(Money::from_cents(45_000));

        let result = command.validate_business_rules(&state);

        assert!(matches!(
            result,
            Err(CommandError::BusinessRuleViolation(msg)) if msg.contains("Daily transfer limit")
        ));
    }
}
```

### Integration Tests for Concurrency

```rust
#[tokio::test]
async fn test_concurrent_modification_handling() {
    let store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(store);

    // Setup
    create_account(&executor, "account-1", 1000).await;

    // Create two conflicting commands
    let withdraw1 = WithdrawMoney {
        account: StreamId::from_static("account-1"),
        amount: Money::from_cents(600),
    };

    let withdraw2 = WithdrawMoney {
        account: StreamId::from_static("account-1"),
        amount: Money::from_cents(700),
    };

    // Execute concurrently
    let (result1, result2) = tokio::join!(
        executor.execute(&withdraw1),
        executor.execute(&withdraw2)
    );

    // One should succeed, one should fail due to insufficient funds after retry
    let successes = [&result1, &result2]
        .iter()
        .filter(|r| r.is_ok())
        .count();

    assert_eq!(successes, 1, "Exactly one withdrawal should succeed");

    // Check final balance
    let balance = get_account_balance(&store, "account-1").await;
    assert!(balance == 400 || balance == 300); // 1000 - 600 or 1000 - 700
}
```

### Chaos Testing

```rust
use eventcore::testing::chaos::ChaosConfig;

#[tokio::test]
async fn test_resilience_under_chaos() {
    let base_store = InMemoryEventStore::new();
    let chaos_store = base_store.with_chaos(ChaosConfig {
        failure_probability: 0.1,  // 10% chance of failure
        latency_ms: Some(50..200), // Random latency
        version_conflict_probability: 0.2, // 20% chance of conflicts
    });

    let executor = CommandExecutor::new(chaos_store)
        .with_max_retries(10);

    // Run many operations
    let mut handles = vec![];
    for i in 0..100 {
        let executor = executor.clone();
        let handle = tokio::spawn(async move {
            let command = CreateTask {
                title: format!("Task {}", i),
                // ...
            };
            executor.execute(&command).await
        });
        handles.push(handle);
    }

    // Collect results
    let results: Vec<_> = futures::future::join_all(handles).await;

    // Despite chaos, most should succeed due to retries
    let success_rate = results.iter()
        .filter(|r| r.as_ref().unwrap().is_ok())
        .count() as f64 / results.len() as f64;

    assert!(success_rate > 0.95, "Success rate too low: {}", success_rate);
}
```

## Production Error Handling

### Monitoring and Alerting

```rust
use prometheus::{Counter, Histogram, register_counter, register_histogram};

lazy_static! {
    static ref COMMAND_ERRORS: Counter = register_counter!(
        "eventcore_command_errors_total",
        "Total number of command errors"
    ).unwrap();

    static ref RETRY_COUNT: Histogram = register_histogram!(
        "eventcore_command_retries",
        "Number of retries per command"
    ).unwrap();
}

impl CommandExecutor {
    async fn execute_with_metrics(&self, command: &impl Command) -> CommandResult<ExecutionResult> {
        let start = Instant::now();
        let mut retries = 0;

        loop {
            match self.execute_once(command).await {
                Ok(result) => {
                    RETRY_COUNT.observe(retries as f64);
                    return Ok(result);
                }
                Err(e) => {
                    COMMAND_ERRORS.inc();

                    if e.is_retriable() && retries < self.max_retries {
                        retries += 1;
                        continue;
                    }

                    return Err(e);
                }
            }
        }
    }
}
```

### Error Recovery Procedures

Document recovery procedures:

```rust
/// Recovery procedure for payment processing failures
///
/// 1. Check payment provider status
/// 2. Verify account balances match event history
/// 3. Look for orphaned payments in provider but not in events
/// 4. Run reconciliation command if discrepancies found
/// 5. Contact support if automated recovery fails
#[derive(Command, Clone)]
struct ReconcilePayments {
    #[stream]
    payment_provider: StreamId,

    #[stream]
    reconciliation_log: StreamId,

    provider_transactions: Vec<ProviderTransaction>,
}
```

## Best Practices

### 1. Fail Fast

Validate as early as possible:

```rust
// ✅ Good - validate at construction
impl TransferMoney {
    pub fn new(
        from: StreamId,
        to: StreamId,
        amount: Money,
    ) -> Result<Self, ValidationError> {
        if from == to {
            return Err(ValidationError::SameAccount);
        }

        Ok(Self {
            from_account: from,
            to_account: to,
            amount,
        })
    }
}

// ❌ Bad - validate late in handle()
```

### 2. Be Specific

Use specific error types:

```rust
// ✅ Good - specific errors
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("Insufficient balance: available {available}, requested {requested}")]
    InsufficientBalance { available: Money, requested: Money },

    #[error("Daily limit exceeded: limit {limit}, attempted {attempted}")]
    DailyLimitExceeded { limit: Money, attempted: Money },

    #[error("Account {0} is frozen")]
    AccountFrozen(AccountId),
}

// ❌ Bad - generic errors
Err("Transfer failed".into())
```

### 3. Make Errors Actionable

Provide enough context to fix issues:

```rust
// ✅ Good - actionable error
require!(
    state.account.kyc_verified,
    "Account KYC verification required. Please complete verification at: https://example.com/kyc/{}",
    state.account.id
);

// ❌ Bad - vague error
require!(state.account.kyc_verified, "KYC required");
```

## Summary

Error handling in EventCore:

- ✅ **Type-safe** - Errors encoded in function signatures
- ✅ **Recoverable** - Automatic retries for transient failures
- ✅ **Informative** - Rich context for debugging
- ✅ **Testable** - Easy to test error scenarios
- ✅ **Production-ready** - Monitoring and recovery built-in

Best practices:

1. Use `require!` macro for concise validation
2. Create specific error types for your domain
3. Add context to errors for debugging
4. Test error scenarios thoroughly
5. Monitor errors in production

You've completed Part 3! Continue to [Part 4: Building Web APIs](../04-building-web-apis/README.md) →
