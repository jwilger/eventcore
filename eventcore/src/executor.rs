use crate::command::{Command, CommandResult, ReadStreams, StreamWrite};
use crate::errors::CommandError;
use crate::event_store::{EventToWrite, ExpectedVersion, ReadOptions, StreamEvents};
use crate::types::{EventId, EventVersion, StreamId};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, instrument, warn};

#[cfg(test)]
use async_trait::async_trait;

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

/// Execution options for command execution with sensible defaults.
///
/// By default, commands are executed with retry logic enabled for concurrency conflicts.
/// This provides automatic handling of transient failures without requiring explicit
/// configuration in the common case.
///
/// # Example
///
/// ```rust,ignore
/// // Execute with default retry behavior
/// executor.execute(&command, input, ExecutionOptions::default()).await?;
///
/// // Execute without retry
/// executor.execute(&command, input, ExecutionOptions::new().without_retry()).await?;
///
/// // Execute with custom retry configuration
/// executor.execute(
///     &command,
///     input,
///     ExecutionOptions::new()
///         .with_retry_config(RetryConfig { max_attempts: 10, ..Default::default() })
/// ).await?;
/// ```
#[derive(Debug, Clone)]
pub struct ExecutionOptions {
    /// Execution context for tracing and auditing.
    pub context: ExecutionContext,
    /// Retry configuration. None disables retries entirely.
    pub retry_config: Option<RetryConfig>,
    /// Policy for determining which errors should trigger a retry.
    pub retry_policy: RetryPolicy,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            context: ExecutionContext::default(),
            retry_config: Some(RetryConfig::default()), // Retry enabled by default
            retry_policy: RetryPolicy::default(),
        }
    }
}

impl ExecutionOptions {
    /// Creates new execution options with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the execution context.
    #[must_use]
    pub fn with_context(mut self, context: ExecutionContext) -> Self {
        self.context = context;
        self
    }

    /// Disables retry logic entirely.
    #[must_use]
    pub const fn without_retry(mut self) -> Self {
        self.retry_config = None;
        self
    }

    /// Sets a custom retry configuration.
    #[must_use]
    pub const fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Sets the retry policy.
    #[must_use]
    pub const fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Sets the correlation ID in the execution context.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.context.correlation_id = correlation_id;
        self
    }

    /// Sets the user ID in the execution context.
    #[must_use]
    pub fn with_user_id(mut self, user_id: Option<String>) -> Self {
        self.context.user_id = user_id;
        self
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

/// Type alias for the event store trait used by command executor.
///
/// This re-exports the `EventStore` trait from the `event_store` module
/// to maintain a clean interface for the executor.
pub use crate::event_store::EventStore;

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
#[derive(Debug, Clone)]
pub struct CommandExecutor<ES> {
    /// The event store implementation.
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

    /// Executes a command with automatic retry logic based on the provided options.
    ///
    /// By default, this method will retry on concurrency conflicts using exponential
    /// backoff. The retry behavior can be customized or disabled through the
    /// `ExecutionOptions` parameter.
    ///
    /// # Type Parameters
    ///
    /// * `C` - The command type to execute
    ///
    /// # Arguments
    ///
    /// * `command` - The command instance to execute
    /// * `input` - The validated command input
    /// * `options` - Execution options including retry configuration and context
    ///
    /// # Returns
    ///
    /// A result containing the success outcome or a `CommandError`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Execute with default retry behavior
    /// let result = executor.execute(
    ///     &command,
    ///     input,
    ///     ExecutionOptions::default()
    /// ).await?;
    ///
    /// // Execute without retry
    /// let result = executor.execute(
    ///     &command,
    ///     input,
    ///     ExecutionOptions::new().without_retry()
    /// ).await?;
    /// ```
    #[instrument(skip(self, command, input), fields(
        command_type = std::any::type_name::<C>(),
        correlation_id = %options.context.correlation_id,
        user_id = options.context.user_id.as_deref().unwrap_or("anonymous"),
        retry_enabled = options.retry_config.is_some()
    ))]
    pub async fn execute<C>(
        &self,
        command: &C,
        input: C::Input,
        options: ExecutionOptions,
    ) -> CommandResult<HashMap<StreamId, EventVersion>>
    where
        C: Command,
        C::Input: Clone,
        C::Event: Clone + for<'a> TryFrom<&'a ES::Event>,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
        ES::Event: From<C::Event>,
    {
        match options.retry_config {
            Some(retry_config) => {
                // Execute with retry logic
                self.execute_with_retry_internal(
                    command,
                    input,
                    options.context,
                    retry_config,
                    options.retry_policy,
                )
                .await
            }
            None => {
                // Execute without retry
                self.execute_once(command, input, options.context).await
            }
        }
    }

    /// Executes a command once without retry logic.
    ///
    /// This internal method orchestrates the complete command execution flow:
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
    #[instrument(
        skip(self, command, input),
        fields(
            command_type = std::any::type_name::<C>(),
            correlation_id = %context.correlation_id,
            user_id = context.user_id.as_deref().unwrap_or("anonymous")
        )
    )]
    async fn execute_once<C>(
        &self,
        command: &C,
        input: C::Input,
        context: ExecutionContext,
    ) -> CommandResult<HashMap<StreamId, EventVersion>>
    where
        C: Command,
        C::Event: Clone + for<'a> TryFrom<&'a ES::Event>,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
        ES::Event: From<C::Event>,
    {
        // Step 1: Determine which streams to read
        let stream_ids = command.read_streams(&input);
        info!(
            streams_count = stream_ids.len(),
            "Reading streams for command execution"
        );

        // Step 2: Read events from those streams
        let stream_data = self
            .event_store
            .read_streams(&stream_ids, &ReadOptions::new())
            .await
            .map_err(|err| {
                warn!(error = %err, "Failed to read streams from event store");
                CommandError::from(err)
            })?;

        let total_events = stream_data.len();
        info!(
            events_count = total_events,
            "Retrieved events for state reconstruction"
        );

        // Step 3: Reconstruct state by folding events (optimized)
        let mut state = C::State::default();
        let mut applied_events = 0;

        // Pre-allocate vector to avoid reallocations during event conversion
        let events: Vec<_> = stream_data.events().collect();
        let mut converted_events = Vec::with_capacity(events.len());

        // Convert all events upfront to reduce allocations in the loop
        for event in events {
            if let Ok(command_event) = Self::try_convert_event::<C>(&event.payload) {
                let stored_event = crate::event_store::StoredEvent::new(
                    event.event_id,
                    event.stream_id.clone(),
                    event.event_version,
                    event.timestamp,
                    command_event,
                    event.metadata.clone(),
                );
                converted_events.push(stored_event);
            }
        }

        // Apply all converted events to state
        for stored_event in converted_events {
            command.apply(&mut state, &stored_event);
            applied_events += 1;
        }
        info!(applied_events, "Applied events to reconstruct state");

        // Step 4: Execute command business logic with type-safe stream access
        let read_streams = ReadStreams::new(stream_ids.clone());
        let stream_writes = command
            .handle(read_streams, state, input)
            .await
            .map_err(|err| {
                warn!(error = %err, "Command business logic failed");
                err
            })?;

        let stream_writes_count = stream_writes.len();
        info!(
            events_to_write = stream_writes_count,
            "Command produced events for writing"
        );

        // Convert StreamWrite instances to (StreamId, Event) pairs
        let events_to_write: Vec<(StreamId, C::Event)> = stream_writes
            .into_iter()
            .map(super::command::StreamWrite::into_parts)
            .collect();

        // Step 5: Write resulting events atomically with optimistic concurrency control
        // CRITICAL: Check expected versions of ALL streams that were read, not just written streams
        let stream_events = Self::prepare_stream_events_with_complete_concurrency_control::<C>(
            events_to_write,
            &stream_data,
            &stream_ids, // Pass ALL read streams for version checking
            &context,
        );

        let result_versions = self
            .event_store
            .write_events_multi(stream_events)
            .await
            .map_err(|err| {
                warn!(error = %err, "Failed to write events to event store");
                CommandError::from(err)
            })?;

        info!(
            written_streams = result_versions.len(),
            "Successfully executed command"
        );
        Ok(result_versions)
    }

    /// Attempts to convert an event store event to a command event.
    ///
    /// This is a helper method that tries to convert between event types.
    /// In practice, this conversion logic will depend on the specific event
    /// serialization strategy used by the application.
    fn try_convert_event<C>(event: &ES::Event) -> Result<C::Event, CommandError>
    where
        C: Command,
        C::Event: Clone + for<'a> TryFrom<&'a ES::Event>,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
    {
        C::Event::try_from(event)
            .map_err(|e| CommandError::ValidationFailed(format!("Event conversion failed: {e}")))
    }

    /// Prepares stream events for writing with proper version control.
    fn prepare_stream_events<C>(
        events_to_write: Vec<(StreamId, C::Event)>,
        stream_data: &crate::event_store::StreamData<ES::Event>,
        context: &ExecutionContext,
    ) -> Vec<StreamEvents<ES::Event>>
    where
        C: Command,
        ES::Event: From<C::Event>,
    {
        use crate::metadata::{CorrelationId, UserId};

        // Group events by stream - optimized to avoid reallocations
        let mut streams: HashMap<StreamId, Vec<C::Event>> = HashMap::with_capacity(4); // Most commands use 1-4 streams
        for (stream_id, event) in events_to_write {
            streams
                .entry(stream_id)
                .or_insert_with(|| Vec::with_capacity(1))
                .push(event);
        }

        let mut stream_events = Vec::with_capacity(streams.len());

        for (stream_id, events) in streams {
            // Get current version for optimistic concurrency control
            let current_version = stream_data
                .stream_version(&stream_id)
                .unwrap_or_else(EventVersion::initial);
            let expected_version = if current_version == EventVersion::initial() {
                ExpectedVersion::New
            } else {
                ExpectedVersion::Exact(current_version)
            };

            // Convert events to EventToWrite instances
            let events_to_write: Vec<EventToWrite<ES::Event>> = events
                .into_iter()
                .map(|event| {
                    let event_id = EventId::new();

                    // Parse correlation ID - use a new one if parsing fails
                    let correlation_id = uuid::Uuid::parse_str(&context.correlation_id)
                        .ok()
                        .and_then(|uuid| CorrelationId::try_new(uuid).ok())
                        .unwrap_or_default();

                    let user_id = context
                        .user_id
                        .as_ref()
                        .and_then(|uid| UserId::try_new(uid.clone()).ok());

                    let metadata = crate::metadata::EventMetadata::new()
                        .with_correlation_id(correlation_id)
                        .with_user_id(user_id);

                    EventToWrite::with_metadata(event_id, ES::Event::from(event), metadata)
                })
                .collect();

            stream_events.push(StreamEvents::new(
                stream_id,
                expected_version,
                events_to_write,
            ));
        }

        stream_events
    }

    /// Prepares stream events with COMPLETE concurrency control.
    ///
    /// This method ensures that ALL streams that were read are checked for version conflicts,
    /// not just the streams being written to. This prevents commands from making decisions
    /// based on stale data from ANY of the streams they read.
    fn prepare_stream_events_with_complete_concurrency_control<C>(
        events_to_write: Vec<(StreamId, C::Event)>,
        stream_data: &crate::event_store::StreamData<ES::Event>,
        all_read_streams: &[StreamId], // ALL streams that were read
        context: &ExecutionContext,
    ) -> Vec<StreamEvents<ES::Event>>
    where
        C: Command,
        ES::Event: From<C::Event>,
    {
        use crate::metadata::{CorrelationId, UserId};

        // Group events by stream for writing
        let mut streams_with_writes: HashMap<StreamId, Vec<C::Event>> = HashMap::with_capacity(4);
        for (stream_id, event) in events_to_write {
            streams_with_writes
                .entry(stream_id)
                .or_insert_with(|| Vec::with_capacity(1))
                .push(event);
        }

        let mut stream_events =
            Vec::with_capacity(all_read_streams.len().max(streams_with_writes.len()));

        // Process streams that have writes (same as before)
        for (stream_id, events) in streams_with_writes {
            let current_version = stream_data
                .stream_version(&stream_id)
                .unwrap_or_else(EventVersion::initial);
            let expected_version = if current_version == EventVersion::initial() {
                ExpectedVersion::New
            } else {
                ExpectedVersion::Exact(current_version)
            };

            // Convert events to EventToWrite instances
            let events_to_write: Vec<EventToWrite<ES::Event>> = events
                .into_iter()
                .map(|event| {
                    let event_id = EventId::new();

                    let correlation_id = uuid::Uuid::parse_str(&context.correlation_id)
                        .ok()
                        .and_then(|uuid| CorrelationId::try_new(uuid).ok())
                        .unwrap_or_default();

                    let user_id = context
                        .user_id
                        .as_ref()
                        .and_then(|uid| UserId::try_new(uid.clone()).ok());

                    let metadata = crate::metadata::EventMetadata::new()
                        .with_correlation_id(correlation_id)
                        .with_user_id(user_id);

                    EventToWrite::with_metadata(event_id, ES::Event::from(event), metadata)
                })
                .collect();

            stream_events.push(StreamEvents::new(
                stream_id,
                expected_version,
                events_to_write,
            ));
        }

        // CRITICAL: Also add version checks for streams that were READ but NOT written to
        // This ensures complete concurrency control - any change to ANY read stream will
        // cause the command to be retried with fresh data
        for read_stream_id in all_read_streams {
            // Skip streams we're already writing to (handled above)
            if stream_events
                .iter()
                .any(|se| &se.stream_id == read_stream_id)
            {
                continue;
            }

            // Add a version check for this read-only stream
            let current_version = stream_data
                .stream_version(read_stream_id)
                .unwrap_or_else(EventVersion::initial);
            let expected_version = if current_version == EventVersion::initial() {
                ExpectedVersion::New
            } else {
                ExpectedVersion::Exact(current_version)
            };

            // Create a StreamEvents with no writes, just the version check
            stream_events.push(StreamEvents::new(
                read_stream_id.clone(),
                expected_version,
                Vec::new(), // No events to write, just checking version
            ));
        }

        stream_events
    }

    /// Internal method that executes a command with automatic retry logic.
    ///
    /// This method wraps the execute_once method with retry logic based on the
    /// provided `RetryConfig` and `RetryPolicy`. It will retry the operation
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
    /// * `retry_config` - Configuration for retry behavior
    /// * `retry_policy` - Policy for determining which errors should trigger a retry
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
    #[instrument(
        skip(self, command, input),
        fields(
            command_type = std::any::type_name::<C>(),
            correlation_id = %context.correlation_id,
            user_id = context.user_id.as_deref().unwrap_or("anonymous"),
            max_attempts = retry_config.max_attempts
        )
    )]
    async fn execute_with_retry_internal<C>(
        &self,
        command: &C,
        input: C::Input,
        context: ExecutionContext,
        retry_config: RetryConfig,
        retry_policy: RetryPolicy,
    ) -> CommandResult<HashMap<StreamId, EventVersion>>
    where
        C: Command,
        C::Input: Clone,
        C::Event: Clone + for<'a> TryFrom<&'a ES::Event>,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
        ES::Event: From<C::Event>,
    {
        let mut last_error = None;

        for attempt in 0..retry_config.max_attempts {
            info!(attempt = attempt + 1, "Attempting command execution");

            match self
                .execute_once(command, input.clone(), context.clone())
                .await
            {
                Ok(result) => {
                    if attempt > 0 {
                        info!(attempt = attempt + 1, "Command succeeded after retry");
                    }
                    return Ok(result);
                }
                Err(error) => {
                    warn!(
                        attempt = attempt + 1,
                        error = %error,
                        "Command execution failed"
                    );

                    // Check if this error should trigger a retry
                    if !retry_policy.should_retry(&error) {
                        warn!("Error is not retryable, failing immediately");
                        return Err(error);
                    }

                    last_error = Some(error);

                    // If this is not the last attempt, wait before retrying
                    if attempt < retry_config.max_attempts - 1 {
                        let delay = Self::calculate_retry_delay(attempt, &retry_config);
                        info!(
                            retry_delay_ms = delay.as_millis(),
                            next_attempt = attempt + 2,
                            "Retrying command execution after delay"
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        // All retries exhausted, return the last error
        warn!("All retry attempts exhausted");
        Err(last_error.unwrap_or_else(|| {
            CommandError::ValidationFailed("Retry exhausted with no error".to_string())
        }))
    }

    /// Calculates the delay for the next retry attempt.
    ///
    /// Uses exponential backoff with jitter to prevent thundering herd problems.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The current attempt number (0-based)
    /// * `retry_config` - The retry configuration to use
    ///
    /// # Returns
    ///
    /// The duration to wait before the next retry attempt.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::items_after_statements
    )]
    fn calculate_retry_delay(attempt: u32, retry_config: &RetryConfig) -> Duration {
        use rand::Rng;

        let base_delay_ms = retry_config.base_delay.as_millis() as f64;
        let max_delay_ms = retry_config.max_delay.as_millis() as f64;

        let delay = base_delay_ms * retry_config.backoff_multiplier.powi(attempt as i32);
        let delay = delay.min(max_delay_ms);

        // Add jitter (±25% of the delay)
        let mut rng = rand::thread_rng();
        let jitter = delay * 0.25 * (rng.gen::<f64>() - 0.5) * 2.0;
        let final_delay = (delay + jitter).max(0.0).min(max_delay_ms) as u64;

        Duration::from_millis(final_delay)
    }

    /// Returns a reference to the event store.
    ///
    /// This accessor is useful for direct access to the event store
    /// when needed for advanced operations or testing.
    ///
    /// # Returns
    ///
    /// A reference to the underlying event store implementation.
    pub const fn event_store(&self) -> &ES {
        &self.event_store
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
    use crate::event_store::StoredEvent;
    use crate::types::Timestamp;
    use proptest::prelude::*;
    use std::sync::{Arc, Mutex};

    /// Mock event store for testing with configurable behavior.
    #[derive(Clone)]
    #[allow(dead_code)]
    struct MockEventStore {
        streams: Arc<Mutex<HashMap<StreamId, Vec<StoredEvent<String>>>>>,
        versions: Arc<Mutex<HashMap<StreamId, EventVersion>>>,
        fail_reads: Arc<Mutex<bool>>,
        fail_writes: Arc<Mutex<bool>>,
    }

    #[allow(dead_code, clippy::significant_drop_tightening)]
    impl MockEventStore {
        fn new() -> Self {
            Self {
                streams: Arc::new(Mutex::new(HashMap::new())),
                versions: Arc::new(Mutex::new(HashMap::new())),
                fail_reads: Arc::new(Mutex::new(false)),
                fail_writes: Arc::new(Mutex::new(false)),
            }
        }

        fn add_event(&self, stream_id: StreamId, event: String) {
            let mut streams = self.streams.lock().unwrap();
            let mut versions = self.versions.lock().unwrap();

            let current_version = versions
                .get(&stream_id)
                .copied()
                .unwrap_or_else(EventVersion::initial);
            let new_version = current_version.next();

            let stored_event = StoredEvent::new(
                EventId::new(),
                stream_id.clone(),
                new_version,
                Timestamp::now(),
                event,
                None,
            );

            streams
                .entry(stream_id.clone())
                .or_default()
                .push(stored_event);
            versions.insert(stream_id, new_version);
        }

        fn set_fail_reads(&self, fail: bool) {
            *self.fail_reads.lock().unwrap() = fail;
        }

        fn set_fail_writes(&self, fail: bool) {
            *self.fail_writes.lock().unwrap() = fail;
        }
    }

    #[async_trait]
    #[allow(clippy::significant_drop_tightening)]
    impl EventStore for MockEventStore {
        type Event = String;

        async fn read_streams(
            &self,
            stream_ids: &[StreamId],
            _options: &ReadOptions,
        ) -> crate::errors::EventStoreResult<crate::event_store::StreamData<Self::Event>> {
            if *self.fail_reads.lock().unwrap() {
                return Err(crate::errors::EventStoreError::ConnectionFailed(
                    "Mock read failure".to_string(),
                ));
            }

            let streams = self.streams.lock().unwrap();
            let versions = self.versions.lock().unwrap();

            let mut all_events = Vec::new();
            let mut stream_versions = HashMap::new();

            for stream_id in stream_ids {
                let version = versions
                    .get(stream_id)
                    .copied()
                    .unwrap_or_else(EventVersion::initial);
                stream_versions.insert(stream_id.clone(), version);

                if let Some(stream_events) = streams.get(stream_id) {
                    all_events.extend(stream_events.clone());
                }
            }

            all_events.sort_by_key(|e| e.event_id);
            Ok(crate::event_store::StreamData::new(
                all_events,
                stream_versions,
            ))
        }

        async fn write_events_multi(
            &self,
            stream_events: Vec<StreamEvents<Self::Event>>,
        ) -> crate::errors::EventStoreResult<HashMap<StreamId, EventVersion>> {
            if *self.fail_writes.lock().unwrap() {
                return Err(crate::errors::EventStoreError::ConnectionFailed(
                    "Mock write failure".to_string(),
                ));
            }

            let mut streams = self.streams.lock().unwrap();
            let mut versions = self.versions.lock().unwrap();
            let mut result_versions = HashMap::new();

            for stream_event in stream_events {
                let current_version = versions
                    .get(&stream_event.stream_id)
                    .copied()
                    .unwrap_or_else(EventVersion::initial);

                // Check expected version
                match stream_event.expected_version {
                    ExpectedVersion::New => {
                        if versions.contains_key(&stream_event.stream_id) {
                            return Err(crate::errors::EventStoreError::VersionConflict {
                                stream: stream_event.stream_id,
                                expected: EventVersion::initial(),
                                current: current_version,
                            });
                        }
                    }
                    ExpectedVersion::Exact(expected) => {
                        if current_version != expected {
                            return Err(crate::errors::EventStoreError::VersionConflict {
                                stream: stream_event.stream_id,
                                expected,
                                current: current_version,
                            });
                        }
                    }
                    ExpectedVersion::Any => {}
                }

                let mut new_version = current_version;
                for event_to_write in stream_event.events {
                    new_version = new_version.next();
                    let stored_event = StoredEvent::new(
                        event_to_write.event_id,
                        stream_event.stream_id.clone(),
                        new_version,
                        Timestamp::now(),
                        event_to_write.payload,
                        event_to_write.metadata,
                    );

                    streams
                        .entry(stream_event.stream_id.clone())
                        .or_default()
                        .push(stored_event);
                }

                versions.insert(stream_event.stream_id.clone(), new_version);
                result_versions.insert(stream_event.stream_id, new_version);
            }

            Ok(result_versions)
        }

        async fn stream_exists(
            &self,
            stream_id: &StreamId,
        ) -> crate::errors::EventStoreResult<bool> {
            let streams = self.streams.lock().unwrap();
            Ok(streams.contains_key(stream_id))
        }

        async fn get_stream_version(
            &self,
            stream_id: &StreamId,
        ) -> crate::errors::EventStoreResult<Option<EventVersion>> {
            let versions = self.versions.lock().unwrap();
            Ok(versions.get(stream_id).copied())
        }

        async fn subscribe(
            &self,
            _options: crate::subscription::SubscriptionOptions,
        ) -> crate::errors::EventStoreResult<
            Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
        > {
            let subscription = crate::subscription::SubscriptionImpl::new();
            Ok(Box::new(subscription))
        }
    }

    /// Mock command for testing.
    struct MockCommand {
        streams_to_read: Vec<StreamId>,
        events_to_write: Vec<(StreamId, String)>,
        should_fail: bool,
    }

    impl MockCommand {
        fn new(streams_to_read: Vec<StreamId>, events_to_write: Vec<(StreamId, String)>) -> Self {
            Self {
                streams_to_read,
                events_to_write,
                should_fail: false,
            }
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }

    #[derive(Default, Clone)]
    struct MockState {
        applied_events: Vec<String>,
    }

    #[derive(Clone)]
    #[allow(dead_code)]
    struct MockInput {
        value: String,
    }

    // Create a simple mock event type for testing
    #[derive(Debug, Clone, PartialEq)]
    struct MockEvent(String);

    // Custom error type for testing that implements Display
    #[derive(Debug)]
    struct MockConversionError;

    impl std::fmt::Display for MockConversionError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Mock conversion error")
        }
    }

    impl std::error::Error for MockConversionError {}

    // Implement conversion for testing - our mock event to String
    impl TryFrom<&String> for MockEvent {
        type Error = MockConversionError;

        fn try_from(value: &String) -> Result<Self, Self::Error> {
            Ok(Self(value.clone()))
        }
    }

    // Allow conversion from MockEvent to String for event store
    impl From<MockEvent> for String {
        fn from(event: MockEvent) -> Self {
            event.0
        }
    }

    #[async_trait]
    impl Command for MockCommand {
        type Input = MockInput;
        type State = MockState;
        type Event = MockEvent;
        type StreamSet = ();

        fn read_streams(&self, _input: &Self::Input) -> Vec<StreamId> {
            self.streams_to_read.clone()
        }

        fn apply(
            &self,
            state: &mut Self::State,
            stored_event: &crate::event_store::StoredEvent<Self::Event>,
        ) {
            state.applied_events.push(stored_event.payload.0.clone());
        }

        async fn handle(
            &self,
            _read_streams: ReadStreams<Self::StreamSet>,
            _state: Self::State,
            _input: Self::Input,
        ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
            if self.should_fail {
                Err(CommandError::BusinessRuleViolation(
                    "Mock failure".to_string(),
                ))
            } else {
                Ok(self
                    .events_to_write
                    .clone()
                    .into_iter()
                    .map(|(stream_id, event_str)| {
                        StreamWrite::new(&_read_streams, stream_id, MockEvent(event_str)).unwrap()
                    })
                    .collect())
            }
        }
    }

    #[tokio::test]
    async fn execute_command_handles_business_rule_violation() {
        let event_store = MockEventStore::new();
        let executor = CommandExecutor::new(event_store);

        let stream_id = StreamId::try_new("test-stream").unwrap();
        let command = MockCommand::new(
            vec![stream_id.clone()],
            vec![(stream_id.clone(), "test-event".to_string())],
        )
        .with_failure();
        let input = MockInput {
            value: "test".to_string(),
        };

        let result = executor
            .execute(&command, input, ExecutionOptions::default())
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CommandError::BusinessRuleViolation(_)
        ));
    }

    #[tokio::test]
    async fn execute_command_handles_event_store_read_failure() {
        let event_store = MockEventStore::new();
        event_store.set_fail_reads(true);
        let executor = CommandExecutor::new(event_store);

        let stream_id = StreamId::try_new("test-stream").unwrap();
        let command = MockCommand::new(
            vec![stream_id.clone()],
            vec![(stream_id.clone(), "test-event".to_string())],
        );
        let input = MockInput {
            value: "test".to_string(),
        };

        let result = executor
            .execute(&command, input, ExecutionOptions::default())
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CommandError::EventStore(_)));
    }

    #[tokio::test]
    async fn retry_policy_respects_non_retryable_errors() {
        let event_store = MockEventStore::new();
        let executor = CommandExecutor::new(event_store)
            .with_retry_policy(RetryPolicy::ConcurrencyConflictsOnly);

        let stream_id = StreamId::try_new("test-stream").unwrap();
        let command = MockCommand::new(
            vec![stream_id.clone()],
            vec![(stream_id.clone(), "test-event".to_string())],
        )
        .with_failure(); // This creates a BusinessRuleViolation which shouldn't retry
        let input = MockInput {
            value: "test".to_string(),
        };
        let context = ExecutionContext::default();

        let result = executor
            .execute(
                &command,
                input,
                ExecutionOptions::new().with_context(context),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CommandError::BusinessRuleViolation(_)
        ));
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
        let event_store = MockEventStore::new();
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
            let executor = CommandExecutor::new(MockEventStore::new())
                .with_retry_config(RetryConfig {
                    base_delay: Duration::from_millis(100),
                    max_delay: Duration::from_secs(5),
                    backoff_multiplier: 2.0,
                    ..Default::default()
                });

            let delay = calculate_retry_delay(attempt, &executor.retry_config);

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

            let executor = CommandExecutor::new(MockEventStore::new());

            // Run multiple times to account for jitter
            let mut delay1_less_than_delay2 = 0;
            let trials = 10;

            for _ in 0..trials {
                let delay1 = calculate_retry_delay(attempt1, &executor.retry_config);
                let delay2 = calculate_retry_delay(attempt2, &executor.retry_config);

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
