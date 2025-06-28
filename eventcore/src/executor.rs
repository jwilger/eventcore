use crate::command::{Command, CommandResult};
use crate::errors::CommandError;
use crate::types::{EventVersion, StreamId};
use async_trait::async_trait;
use std::time::Duration;

/// Configuration for command execution retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Base delay between retry attempts.
    pub base_delay: Duration,
    /// Maximum delay between retry attempts (for exponential backoff).
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

/// Policy defining which errors should trigger a retry.
#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// Only retry on concurrency conflicts.
    ConcurrencyConflictsOnly,
    /// Retry on concurrency conflicts and transient errors.
    ConcurrencyAndTransient,
    /// Custom policy with user-defined predicate.
    Custom(fn(&CommandError) -> bool),
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::ConcurrencyConflictsOnly
    }
}

impl RetryPolicy {
    /// Determines if an error should trigger a retry.
    pub fn should_retry(&self, error: &CommandError) -> bool {
        match self {
            Self::ConcurrencyConflictsOnly => {
                matches!(error, CommandError::ConcurrencyConflict { .. })
            }
            Self::ConcurrencyAndTransient => {
                matches!(
                    error,
                    CommandError::ConcurrencyConflict { .. } | CommandError::StreamNotFound(_)
                )
            }
            Self::Custom(predicate) => predicate(error),
        }
    }
}

/// Context information for command execution.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Correlation ID for request tracing.
    pub correlation_id: String,
    /// User ID for auditing.
    pub user_id: Option<String>,
    /// Additional metadata for the execution.
    pub metadata: std::collections::HashMap<String, String>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            correlation_id: uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string(),
            user_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// Stream state information for concurrency control.
#[derive(Debug, Clone)]
pub struct StreamState {
    /// The stream identifier.
    pub stream_id: StreamId,
    /// Expected version for optimistic concurrency control.
    pub expected_version: Option<EventVersion>,
    /// Current version of the stream.
    pub current_version: EventVersion,
}

/// Trait representing an event store for command execution.
///
/// This trait will be fully defined in Phase 4, but we need a placeholder
/// for the CommandExecutor to be generic over different event store implementations.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Placeholder for event store operations.
    /// This will be expanded in Phase 4: Event Store Abstraction.
    async fn placeholder(&self) -> CommandResult<()> {
        todo!("EventStore trait will be fully implemented in Phase 4")
    }
}

/// Command executor responsible for orchestrating command execution.
///
/// The `CommandExecutor` handles the complete lifecycle of command execution:
/// 1. Reading required streams from the event store
/// 2. Reconstructing state by folding events
/// 3. Executing command business logic
/// 4. Writing resulting events atomically
/// 5. Handling optimistic concurrency control
/// 6. Managing retries for transient failures
///
/// # Type Parameters
///
/// * `ES` - The event store implementation
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::executor::{CommandExecutor, RetryConfig};
///
/// let executor = CommandExecutor::new(event_store)
///     .with_retry_config(RetryConfig::default());
///
/// let result = executor
///     .execute(&transfer_command, transfer_input, context)
///     .await?;
/// ```
#[derive(Debug)]
pub struct CommandExecutor<ES> {
    /// The event store implementation.
    #[allow(dead_code)] // Will be used when execute methods are implemented
    event_store: ES,
    /// Configuration for retry behavior.
    retry_config: RetryConfig,
    /// Policy for determining retry eligibility.
    retry_policy: RetryPolicy,
}

impl<ES> CommandExecutor<ES>
where
    ES: EventStore,
{
    /// Creates a new command executor with the given event store.
    ///
    /// # Arguments
    ///
    /// * `event_store` - The event store implementation to use
    ///
    /// # Returns
    ///
    /// A new `CommandExecutor` instance with default retry configuration.
    pub fn new(event_store: ES) -> Self {
        Self {
            event_store,
            retry_config: RetryConfig::default(),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Sets the retry configuration for this executor.
    ///
    /// # Arguments
    ///
    /// * `config` - The retry configuration to use
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Cannot be const due to potential future complexity
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Sets the retry policy for this executor.
    ///
    /// # Arguments
    ///
    /// * `policy` - The retry policy to use
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Cannot be const due to potential future complexity
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Executes a command without retry logic.
    ///
    /// This method orchestrates the complete command execution flow:
    /// 1. Determines which streams to read using `command.read_streams()`
    /// 2. Reads events from those streams
    /// 3. Reconstructs state by folding events using `command.apply()`
    /// 4. Executes business logic using `command.handle()`
    /// 5. Writes resulting events atomically with optimistic concurrency control
    ///
    /// # Type Parameters
    ///
    /// * `C` - The command type to execute
    ///
    /// # Arguments
    ///
    /// * `command` - The command instance to execute
    /// * `input` - The validated command input
    /// * `context` - Execution context for tracing and auditing
    ///
    /// # Returns
    ///
    /// A result containing the success outcome or a `CommandError`.
    ///
    /// # Errors
    ///
    /// Returns `CommandError` for various failure scenarios:
    /// - Validation failures
    /// - Business rule violations
    /// - Concurrency conflicts
    /// - Event store errors
    #[allow(clippy::unused_async)]
    pub async fn execute<C>(
        &self,
        _command: &C,
        _input: C::Input,
        _context: ExecutionContext,
    ) -> CommandResult<()>
    where
        C: Command,
    {
        todo!("Implement command execution flow")
    }

    /// Executes a command with automatic retry logic.
    ///
    /// This method wraps the execute method with retry logic based on the
    /// configured `RetryConfig` and `RetryPolicy`. It will retry the operation
    /// if the error matches the retry policy, up to the maximum number of
    /// attempts specified in the retry configuration.
    ///
    /// Retry delays follow exponential backoff with jitter to prevent
    /// thundering herd problems in concurrent scenarios.
    ///
    /// # Type Parameters
    ///
    /// * `C` - The command type to execute
    ///
    /// # Arguments
    ///
    /// * `command` - The command instance to execute
    /// * `input` - The validated command input  
    /// * `context` - Execution context for tracing and auditing
    ///
    /// # Returns
    ///
    /// A result containing the success outcome or the final `CommandError`
    /// after all retry attempts have been exhausted.
    ///
    /// # Errors
    ///
    /// Returns `CommandError` if:
    /// - The error is not retryable according to the retry policy
    /// - All retry attempts have been exhausted
    /// - A non-retryable error occurs during any attempt
    #[allow(clippy::unused_async)]
    pub async fn execute_with_retry<C>(
        &self,
        _command: &C,
        _input: C::Input,
        _context: ExecutionContext,
    ) -> CommandResult<()>
    where
        C: Command,
    {
        todo!("Implement command execution with retry logic")
    }

    /// Calculates the delay for the next retry attempt.
    ///
    /// Uses exponential backoff with jitter to prevent thundering herd problems.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The current attempt number (0-based)
    ///
    /// # Returns
    ///
    /// The duration to wait before the next retry attempt.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::items_after_statements,
        dead_code // Will be used when execute_with_retry is implemented
    )]
    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        use rand::Rng;

        let base_delay_ms = self.retry_config.base_delay.as_millis() as f64;
        let max_delay_ms = self.retry_config.max_delay.as_millis() as f64;

        let delay = base_delay_ms * self.retry_config.backoff_multiplier.powi(attempt as i32);
        let delay = delay.min(max_delay_ms);

        // Add jitter (±25% of the delay)
        let mut rng = rand::thread_rng();
        let jitter = delay * 0.25 * (rng.gen::<f64>() - 0.5) * 2.0;
        let final_delay = (delay + jitter).max(0.0).min(max_delay_ms) as u64;

        Duration::from_millis(final_delay)
    }
}

/// Builder utilities for common command execution patterns.
impl<ES> CommandExecutor<ES>
where
    ES: EventStore,
{
    /// Creates an execution context with a correlation ID and optional user ID.
    ///
    /// # Arguments
    ///
    /// * `correlation_id` - The correlation ID for request tracing
    /// * `user_id` - Optional user ID for auditing
    ///
    /// # Returns
    ///
    /// A new `ExecutionContext` with the specified values.
    pub fn context(correlation_id: String, user_id: Option<String>) -> ExecutionContext {
        ExecutionContext {
            correlation_id,
            user_id,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Creates an execution context with additional metadata.
    ///
    /// # Arguments
    ///
    /// * `correlation_id` - The correlation ID for request tracing
    /// * `user_id` - Optional user ID for auditing
    /// * `metadata` - Additional metadata for the execution
    ///
    /// # Returns
    ///
    /// A new `ExecutionContext` with the specified values.
    pub const fn context_with_metadata(
        correlation_id: String,
        user_id: Option<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> ExecutionContext {
        ExecutionContext {
            correlation_id,
            user_id,
            metadata,
        }
    }

    /// Creates a retry configuration for high-throughput scenarios.
    ///
    /// This configuration reduces retry delays and attempts for scenarios
    /// where fast failure is preferred over persistence.
    ///
    /// # Returns
    ///
    /// A `RetryConfig` optimized for high-throughput scenarios.
    pub const fn fast_retry_config() -> RetryConfig {
        RetryConfig {
            max_attempts: 2,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 1.5,
        }
    }

    /// Creates a retry configuration for fault-tolerant scenarios.
    ///
    /// This configuration increases retry attempts and delays for scenarios
    /// where eventual success is preferred over fast failure.
    ///
    /// # Returns
    ///
    /// A `RetryConfig` optimized for fault-tolerant scenarios.
    pub const fn fault_tolerant_retry_config() -> RetryConfig {
        RetryConfig {
            max_attempts: 10,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(120),
            backoff_multiplier: 2.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Mock event store for testing.
    struct MockEventStore;

    #[async_trait]
    impl EventStore for MockEventStore {
        async fn placeholder(&self) -> CommandResult<()> {
            Ok(())
        }
    }

    #[test]
    fn retry_config_default_values_are_reasonable() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.base_delay, Duration::from_millis(100));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert!((config.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn retry_policy_concurrency_conflicts_only() {
        let policy = RetryPolicy::ConcurrencyConflictsOnly;

        assert!(policy.should_retry(&CommandError::ConcurrencyConflict { streams: vec![] }));
        assert!(!policy.should_retry(&CommandError::ValidationFailed("test".to_string())));
        assert!(!policy.should_retry(&CommandError::BusinessRuleViolation("test".to_string())));
    }

    #[test]
    fn retry_policy_concurrency_and_transient() {
        let policy = RetryPolicy::ConcurrencyAndTransient;

        assert!(policy.should_retry(&CommandError::ConcurrencyConflict { streams: vec![] }));
        assert!(policy.should_retry(&CommandError::StreamNotFound(
            StreamId::try_new("test").unwrap()
        )));
        assert!(!policy.should_retry(&CommandError::ValidationFailed("test".to_string())));
    }

    #[test]
    fn retry_policy_custom() {
        let policy =
            RetryPolicy::Custom(|error| matches!(error, CommandError::ValidationFailed(_)));

        assert!(policy.should_retry(&CommandError::ValidationFailed("test".to_string())));
        assert!(!policy.should_retry(&CommandError::ConcurrencyConflict { streams: vec![] }));
    }

    #[test]
    fn command_executor_builder_pattern() {
        let event_store = MockEventStore;
        let config = RetryConfig {
            max_attempts: 5,
            ..Default::default()
        };
        let policy = RetryPolicy::ConcurrencyAndTransient;

        let executor = CommandExecutor::new(event_store)
            .with_retry_config(config)
            .with_retry_policy(policy);

        assert_eq!(executor.retry_config.max_attempts, 5);
    }

    #[test]
    fn execution_context_default_creates_correlation_id() {
        let context = ExecutionContext::default();
        assert!(!context.correlation_id.is_empty());
        assert!(context.user_id.is_none());
        assert!(context.metadata.is_empty());
    }

    #[test]
    fn command_executor_context_builder() {
        let correlation_id = "test-correlation".to_string();
        let user_id = Some("user123".to_string());

        let context =
            CommandExecutor::<MockEventStore>::context(correlation_id.clone(), user_id.clone());

        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_id, user_id);
        assert!(context.metadata.is_empty());
    }

    #[test]
    fn command_executor_context_with_metadata_builder() {
        let correlation_id = "test-correlation".to_string();
        let user_id = Some("user123".to_string());
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());
        metadata.insert("key2".to_string(), "value2".to_string());

        let context = CommandExecutor::<MockEventStore>::context_with_metadata(
            correlation_id.clone(),
            user_id.clone(),
            metadata.clone(),
        );

        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_id, user_id);
        assert_eq!(context.metadata, metadata);
    }

    #[test]
    fn fast_retry_config_has_reduced_values() {
        let config = CommandExecutor::<MockEventStore>::fast_retry_config();

        assert_eq!(config.max_attempts, 2);
        assert_eq!(config.base_delay, Duration::from_millis(50));
        assert_eq!(config.max_delay, Duration::from_secs(5));
        assert!((config.backoff_multiplier - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn fault_tolerant_retry_config_has_increased_values() {
        let config = CommandExecutor::<MockEventStore>::fault_tolerant_retry_config();

        assert_eq!(config.max_attempts, 10);
        assert_eq!(config.base_delay, Duration::from_millis(200));
        assert_eq!(config.max_delay, Duration::from_secs(120));
        assert!((config.backoff_multiplier - 2.5).abs() < f64::EPSILON);
    }

    proptest! {
        #[test]
        fn retry_delay_calculation_respects_bounds(attempt in 0u32..10) {
            let executor = CommandExecutor::new(MockEventStore)
                .with_retry_config(RetryConfig {
                    base_delay: Duration::from_millis(100),
                    max_delay: Duration::from_secs(5),
                    backoff_multiplier: 2.0,
                    ..Default::default()
                });

            let delay = executor.calculate_retry_delay(attempt);

            // Delay should never exceed max_delay (plus some tolerance for jitter)
            prop_assert!(delay <= Duration::from_secs(6));
            // Delay should always be non-negative
            prop_assert!(!delay.is_zero() || attempt == 0);
        }

        #[test]
        fn retry_delay_increases_with_attempts(
            attempt1 in 0u32..5,
            attempt2 in 0u32..5,
        ) {
            prop_assume!(attempt1 < attempt2);

            let executor = CommandExecutor::new(MockEventStore);

            // Run multiple times to account for jitter
            let mut delay1_less_than_delay2 = 0;
            let trials = 10;

            for _ in 0..trials {
                let delay1 = executor.calculate_retry_delay(attempt1);
                let delay2 = executor.calculate_retry_delay(attempt2);

                if delay1 < delay2 {
                    delay1_less_than_delay2 += 1;
                }
            }

            // With exponential backoff, delay should generally increase
            // Allow some tolerance for jitter
            prop_assert!(delay1_less_than_delay2 >= trials / 2);
        }
    }
}
