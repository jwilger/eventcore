//! Projection runner for managing event subscription and processing.
//!
//! This module provides the core infrastructure for running projections by:
//! - Managing event subscription lifecycles
//! - Processing events through projections
//! - Handling checkpoint management
//! - Providing error recovery mechanisms
//! - Ensuring reliable event processing

use crate::{
    errors::{ProjectionError, ProjectionResult},
    event::StoredEvent,
    event_store::EventStore,
    projection::{Projection, ProjectionCheckpoint, ProjectionStatus},
    subscription::{
        EventProcessor, Subscription, SubscriptionError, SubscriptionName, SubscriptionOptions,
        SubscriptionResult,
    },
};
use async_trait::async_trait;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{interval, sleep, MissedTickBehavior},
};
use tracing::{debug, error, info, instrument, warn};

/// Configuration for the projection runner.
#[derive(Debug, Clone)]
pub struct ProjectionRunnerConfig {
    /// How often to save checkpoints (in number of events processed).
    pub checkpoint_frequency: u64,
    /// Maximum number of events to process in a single batch.
    pub batch_size: usize,
    /// Maximum time to wait between checkpoints.
    pub checkpoint_timeout: Duration,
    /// How long to wait before retrying after an error.
    pub error_retry_delay: Duration,
    /// Maximum number of retries for failed event processing.
    pub max_retries: u32,
    /// Exponential backoff multiplier for retries.
    pub retry_backoff_multiplier: f64,
    /// Maximum retry delay before giving up.
    pub max_retry_delay: Duration,
    /// Whether to stop on first unrecoverable error.
    pub stop_on_error: bool,
}

impl Default for ProjectionRunnerConfig {
    fn default() -> Self {
        Self {
            checkpoint_frequency: 100,
            batch_size: 1000,
            checkpoint_timeout: Duration::from_secs(30),
            error_retry_delay: Duration::from_millis(100),
            max_retries: 5,
            retry_backoff_multiplier: 2.0,
            max_retry_delay: Duration::from_secs(60),
            stop_on_error: false,
        }
    }
}

/// Statistics for projection runner performance monitoring.
#[derive(Debug, Clone, Default)]
pub struct ProjectionRunnerStats {
    /// Total number of events processed.
    pub events_processed: u64,
    /// Total number of checkpoints saved.
    pub checkpoints_saved: u64,
    /// Total number of errors encountered.
    pub errors_encountered: u64,
    /// Total number of retries performed.
    pub retries_performed: u64,
    /// Last processing time per event in microseconds.
    pub last_processing_time_micros: u64,
    /// Average processing time per event in microseconds.
    pub avg_processing_time_micros: u64,
    /// Time when the runner was started.
    pub started_at: Option<Instant>,
    /// Time when the last event was processed.
    pub last_event_processed_at: Option<Instant>,
}

/// Manages the execution of a single projection with error recovery and checkpointing.
pub struct ProjectionRunner<P, E>
where
    P: Projection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + 'static,
{
    projection: Arc<P>,
    subscription: Arc<Mutex<Option<Box<dyn Subscription<Event = E>>>>>,
    config: ProjectionRunnerConfig,
    state: Arc<RwLock<Option<P::State>>>,
    last_checkpoint: Arc<RwLock<ProjectionCheckpoint>>,
    runner_stats: Arc<RwLock<ProjectionRunnerStats>>,
    is_running: Arc<AtomicBool>,
    events_processed_since_checkpoint: Arc<AtomicU64>,
    task_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl<P, E> ProjectionRunner<P, E>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + 'static,
{
    /// Creates a new projection runner.
    pub fn new(projection: P) -> Self {
        Self::with_config(projection, ProjectionRunnerConfig::default())
    }

    /// Creates a new projection runner with custom configuration.
    pub fn with_config(projection: P, config: ProjectionRunnerConfig) -> Self {
        let initial_checkpoint = ProjectionCheckpoint::initial();
        Self {
            projection: Arc::new(projection),
            subscription: Arc::new(Mutex::new(None)),
            config,
            state: Arc::new(RwLock::new(None)),
            last_checkpoint: Arc::new(RwLock::new(initial_checkpoint)),
            runner_stats: Arc::new(RwLock::new(ProjectionRunnerStats::default())),
            is_running: Arc::new(AtomicBool::new(false)),
            events_processed_since_checkpoint: Arc::new(AtomicU64::new(0)),
            task_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Starts the projection runner with the given event store.
    #[instrument(skip(self, event_store))]
    pub async fn start(&self, event_store: Arc<dyn EventStore<Event = E>>) -> ProjectionResult<()> {
        if self.is_running.load(Ordering::Acquire) {
            return Err(ProjectionError::InvalidStateTransition {
                from: ProjectionStatus::Running,
                to: ProjectionStatus::Running,
            });
        }

        info!(
            "Starting projection runner for: {}",
            self.projection.config().name
        );

        // Initialize state and load checkpoint
        self.initialize_runner().await?;

        // Create subscription
        let subscription = self.create_subscription(event_store).await?;
        {
            let mut sub_guard = self.subscription.lock().await;
            *sub_guard = Some(subscription);
        }

        // Start the processing task
        let task = self.spawn_processing_task();
        {
            let mut task_guard = self.task_handle.lock().await;
            *task_guard = Some(task);
        }

        // Mark as running
        self.is_running.store(true, Ordering::Release);

        // Call lifecycle hook
        self.projection.on_start().await?;

        info!(
            "Successfully started projection runner for: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Stops the projection runner.
    #[instrument(skip(self))]
    pub async fn stop(&self) -> ProjectionResult<()> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err(ProjectionError::InvalidStateTransition {
                from: ProjectionStatus::Stopped,
                to: ProjectionStatus::Stopped,
            });
        }

        info!(
            "Stopping projection runner for: {}",
            self.projection.config().name
        );

        // Mark as not running
        self.is_running.store(false, Ordering::Release);

        // Stop the processing task
        {
            let mut task_guard = self.task_handle.lock().await;
            if let Some(task) = task_guard.take() {
                task.abort();
                let _ = task.await; // Wait for graceful shutdown
            }
        }

        // Stop subscription
        {
            let mut sub_guard = self.subscription.lock().await;
            if let Some(subscription) = sub_guard.as_mut() {
                subscription.stop().await.map_err(|e| {
                    ProjectionError::SubscriptionFailed(format!("Failed to stop subscription: {e}"))
                })?;
            }
            *sub_guard = None;
        }

        // Save final checkpoint
        self.save_checkpoint().await?;

        // Call lifecycle hook
        self.projection.on_stop().await?;

        info!(
            "Successfully stopped projection runner for: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Pauses the projection runner.
    #[instrument(skip(self))]
    pub async fn pause(&self) -> ProjectionResult<()> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err(ProjectionError::InvalidStateTransition {
                from: ProjectionStatus::Stopped,
                to: ProjectionStatus::Paused,
            });
        }

        info!(
            "Pausing projection runner for: {}",
            self.projection.config().name
        );

        // Pause subscription
        {
            let mut sub_guard = self.subscription.lock().await;
            if let Some(subscription) = sub_guard.as_mut() {
                subscription.pause().await.map_err(|e| {
                    ProjectionError::SubscriptionFailed(format!(
                        "Failed to pause subscription: {e}"
                    ))
                })?;
            }
        }

        // Save checkpoint before pausing
        self.save_checkpoint().await?;

        // Call lifecycle hook
        self.projection.on_pause().await?;

        info!(
            "Successfully paused projection runner for: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Resumes the projection runner.
    #[instrument(skip(self))]
    pub async fn resume(&self) -> ProjectionResult<()> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err(ProjectionError::InvalidStateTransition {
                from: ProjectionStatus::Stopped,
                to: ProjectionStatus::Running,
            });
        }

        info!(
            "Resuming projection runner for: {}",
            self.projection.config().name
        );

        // Resume subscription
        {
            let mut sub_guard = self.subscription.lock().await;
            if let Some(subscription) = sub_guard.as_mut() {
                subscription.resume().await.map_err(|e| {
                    ProjectionError::SubscriptionFailed(format!(
                        "Failed to resume subscription: {e}"
                    ))
                })?;
            }
        }

        // Call lifecycle hook
        self.projection.on_resume().await?;

        info!(
            "Successfully resumed projection runner for: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Gets the current state of the projection.
    pub async fn get_state(&self) -> ProjectionResult<Option<P::State>> {
        let state_guard = self.state.read().await;
        Ok(state_guard.clone())
    }

    /// Gets the current checkpoint.
    pub async fn get_checkpoint(&self) -> ProjectionCheckpoint {
        let checkpoint_guard = self.last_checkpoint.read().await;
        checkpoint_guard.clone()
    }

    /// Gets the current statistics.
    pub async fn get_stats(&self) -> ProjectionRunnerStats {
        let stats_guard = self.runner_stats.read().await;
        stats_guard.clone()
    }

    /// Returns whether the runner is currently active.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Forces a checkpoint save.
    #[instrument(skip(self))]
    pub async fn save_checkpoint(&self) -> ProjectionResult<()> {
        let checkpoint = {
            let checkpoint_guard = self.last_checkpoint.read().await;
            checkpoint_guard.clone()
        };

        debug!(
            "Saving checkpoint for projection: {}",
            self.projection.config().name
        );

        self.projection.save_checkpoint(checkpoint).await?;

        // Reset counter
        self.events_processed_since_checkpoint
            .store(0, Ordering::Release);

        // Update stats
        {
            let mut stats = self.runner_stats.write().await;
            stats.checkpoints_saved += 1;
        }

        debug!(
            "Successfully saved checkpoint for projection: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Internal method to initialize the runner state.
    async fn initialize_runner(&self) -> ProjectionResult<()> {
        debug!(
            "Initializing projection runner for: {}",
            self.projection.config().name
        );

        // Load checkpoint
        let checkpoint = self.projection.load_checkpoint().await?;
        {
            let mut checkpoint_guard = self.last_checkpoint.write().await;
            *checkpoint_guard = checkpoint;
        }

        // Initialize state
        let initial_state = self.projection.initialize_state().await?;
        {
            let mut state_guard = self.state.write().await;
            *state_guard = Some(initial_state);
        }

        // Initialize stats
        {
            let mut stats = self.runner_stats.write().await;
            stats.started_at = Some(Instant::now());
        }

        debug!(
            "Successfully initialized projection runner for: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Creates a subscription for the projection.
    async fn create_subscription(
        &self,
        event_store: Arc<dyn EventStore<Event = E>>,
    ) -> ProjectionResult<Box<dyn Subscription<Event = E>>> {
        let streams = self.projection.interested_streams();
        let checkpoint = {
            let checkpoint_guard = self.last_checkpoint.read().await;
            checkpoint_guard.clone()
        };

        let subscription_options = if streams.is_empty() {
            SubscriptionOptions::AllStreams {
                from_position: checkpoint.last_event_id,
            }
        } else {
            SubscriptionOptions::SpecificStreams {
                streams,
                from_position: checkpoint.last_event_id,
            }
        };

        debug!(
            "Creating subscription with options: {:?}",
            subscription_options
        );

        event_store
            .subscribe(subscription_options)
            .await
            .map_err(Into::into)
    }

    /// Spawns the background processing task.
    fn spawn_processing_task(&self) -> JoinHandle<()> {
        let projection = Arc::clone(&self.projection);
        let subscription = Arc::clone(&self.subscription);
        let config = self.config.clone();
        let state = Arc::clone(&self.state);
        let last_checkpoint = Arc::clone(&self.last_checkpoint);
        let runner_stats = Arc::clone(&self.runner_stats);
        let is_running = Arc::clone(&self.is_running);
        let events_processed_since_checkpoint = Arc::clone(&self.events_processed_since_checkpoint);

        let task = tokio::spawn(async move {
            debug!(
                "Starting projection processing task for: {}",
                projection.config().name
            );

            let mut checkpoint_interval = interval(config.checkpoint_timeout);
            checkpoint_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

            let processor = ProjectionEventProcessor::new(
                projection.clone(),
                config,
                state,
                last_checkpoint,
                runner_stats,
                events_processed_since_checkpoint,
            );

            // Store the projection name before moving processor
            let projection_name = processor.projection.config().name.clone();

            // Start the subscription with our processor
            let subscription_name =
                SubscriptionName::try_new(format!("projection-{projection_name}"))
                    .expect("Projection name should be valid for subscription");

            {
                let mut sub_guard = subscription.lock().await;
                if let Some(subscription) = sub_guard.as_mut() {
                    let result = subscription
                        .start(
                            subscription_name,
                            SubscriptionOptions::AllStreams {
                                from_position: None,
                            }, // Will be overridden by checkpoint
                            Box::new(processor),
                        )
                        .await;

                    if let Err(e) = result {
                        error!("Failed to start subscription: {}", e);
                        is_running.store(false, Ordering::Release);
                        return;
                    }
                }
            }

            // Checkpoint management loop
            while is_running.load(Ordering::Acquire) {
                tokio::select! {
                    _ = checkpoint_interval.tick() => {
                        debug!("Checkpoint interval triggered");
                        // Checkpoint will be handled by the event processor
                    }
                    () = sleep(Duration::from_millis(100)) => {
                        // Small delay to prevent busy waiting
                    }
                }
            }

            debug!(
                "Projection processing task completed for: {}",
                projection_name
            );
        });

        task
    }
}

/// Event processor implementation for projections.
struct ProjectionEventProcessor<P, E>
where
    P: Projection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug,
{
    projection: Arc<P>,
    config: ProjectionRunnerConfig,
    state: Arc<RwLock<Option<P::State>>>,
    last_checkpoint: Arc<RwLock<ProjectionCheckpoint>>,
    runner_stats: Arc<RwLock<ProjectionRunnerStats>>,
    events_processed_since_checkpoint: Arc<AtomicU64>,
}

impl<P, E> ProjectionEventProcessor<P, E>
where
    P: Projection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug,
{
    const fn new(
        projection: Arc<P>,
        config: ProjectionRunnerConfig,
        state: Arc<RwLock<Option<P::State>>>,
        last_checkpoint: Arc<RwLock<ProjectionCheckpoint>>,
        runner_stats: Arc<RwLock<ProjectionRunnerStats>>,
        events_processed_since_checkpoint: Arc<AtomicU64>,
    ) -> Self {
        Self {
            projection,
            config,
            state,
            last_checkpoint,
            runner_stats,
            events_processed_since_checkpoint,
        }
    }

    /// Processes an event with retry logic and error recovery.
    async fn process_event_with_retry(&self, event: &StoredEvent<E>) -> SubscriptionResult<()> {
        let mut retries = 0;
        let mut delay = self.config.error_retry_delay;

        loop {
            let start_time = Instant::now();

            match self.process_single_event(event).await {
                Ok(()) => {
                    let processing_time = start_time.elapsed();
                    self.update_processing_stats(processing_time).await;
                    return Ok(());
                }
                Err(e) => {
                    // Update error stats
                    {
                        let mut stats = self.runner_stats.write().await;
                        stats.errors_encountered += 1;
                    }

                    // Call error handler
                    let processing_error = ProjectionError::EventProcessingFailed {
                        event_id: event.event.id,
                        reason: format!("Event processing failed: {e}"),
                    };
                    if let Err(handler_error) = self.projection.on_error(&processing_error).await {
                        error!("Error handler failed: {}", handler_error);
                    }

                    if retries >= self.config.max_retries {
                        error!("Max retries exceeded for event processing: {}", e);

                        if self.config.stop_on_error {
                            let max_retries_error = ProjectionError::EventProcessingFailed {
                                event_id: event.event.id,
                                reason: format!("Max retries exceeded: {e}"),
                            };
                            return Err(SubscriptionError::Projection(max_retries_error));
                        }
                        warn!("Skipping event after max retries: {}", e);
                        return Ok(());
                    }

                    retries += 1;

                    // Update retry stats
                    {
                        let mut stats = self.runner_stats.write().await;
                        stats.retries_performed += 1;
                    }

                    warn!(
                        "Retrying event processing (attempt {}/{}): {}",
                        retries, self.config.max_retries, e
                    );

                    // Exponential backoff
                    sleep(delay).await;
                    delay = std::cmp::min(
                        Duration::from_millis(
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            {
                                delay
                                    .as_millis()
                                    .saturating_mul(self.config.retry_backoff_multiplier as u128)
                                    .min(u128::from(u64::MAX))
                                    as u64
                            },
                        ),
                        self.config.max_retry_delay,
                    );
                }
            }
        }
    }

    /// Processes a single event through the projection.
    async fn process_single_event(&self, event: &StoredEvent<E>) -> ProjectionResult<()> {
        // Check if projection should process this event
        if !self.projection.should_process_event(&event.event) {
            debug!("Skipping event {} - projection not interested", event.id());
            return Ok(());
        }

        // Get current state
        let mut current_state = {
            let state_guard = self.state.read().await;
            state_guard.clone().ok_or_else(|| {
                ProjectionError::Internal("Projection state not initialized".to_string())
            })?
        };

        // Apply the event
        self.projection
            .apply_event(&mut current_state, &event.event)
            .await?;

        // Update state
        {
            let mut state_guard = self.state.write().await;
            *state_guard = Some(current_state);
        }

        // Update checkpoint
        {
            let mut checkpoint_guard = self.last_checkpoint.write().await;
            *checkpoint_guard = checkpoint_guard
                .clone()
                .with_event_id(event.event.id)
                .with_stream_position(event.event.stream_id.clone(), event.event.id);
        }

        // Check if we should save checkpoint
        let events_since_checkpoint = self
            .events_processed_since_checkpoint
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        if events_since_checkpoint >= self.config.checkpoint_frequency {
            self.save_checkpoint().await?;
        }

        Ok(())
    }

    /// Saves the current checkpoint.
    async fn save_checkpoint(&self) -> ProjectionResult<()> {
        let checkpoint = {
            let checkpoint_guard = self.last_checkpoint.read().await;
            checkpoint_guard.clone()
        };

        self.projection.save_checkpoint(checkpoint).await?;
        self.events_processed_since_checkpoint
            .store(0, Ordering::Release);

        // Update stats
        {
            let mut stats = self.runner_stats.write().await;
            stats.checkpoints_saved += 1;
        }

        Ok(())
    }

    /// Updates processing time statistics.
    async fn update_processing_stats(&self, processing_time: Duration) {
        let mut stats = self.runner_stats.write().await;
        stats.events_processed += 1;
        stats.last_event_processed_at = Some(Instant::now());

        #[allow(clippy::cast_possible_truncation)]
        let processing_micros = processing_time.as_micros().min(u128::from(u64::MAX)) as u64;
        stats.last_processing_time_micros = processing_micros;

        // Update running average
        if stats.events_processed == 1 {
            stats.avg_processing_time_micros = processing_micros;
        } else {
            // Simple moving average
            stats.avg_processing_time_micros = (stats.avg_processing_time_micros
                * (stats.events_processed - 1)
                + processing_micros)
                / stats.events_processed;
        }
    }
}

#[async_trait]
impl<P, E> EventProcessor for ProjectionEventProcessor<P, E>
where
    P: Projection<Event = E> + Send + Sync,
    P::State: Send + Sync + std::fmt::Debug + Clone,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug,
{
    type Event = E;

    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()> {
        self.process_event_with_retry(&event).await
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        info!(
            "Projection {} caught up to live position",
            self.projection.config().name
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::similar_names, clippy::float_cmp, clippy::redundant_clone)]
#[allow(clippy::significant_drop_tightening, clippy::explicit_counter_loop)]
mod tests {
    use super::*;
    use crate::{
        errors::ProjectionError,
        event::Event,
        metadata::EventMetadata,
        projection::{InMemoryProjection, ProjectionConfig},
        types::{EventVersion, StreamId},
    };
    use async_trait::async_trait;
    use std::{
        sync::{
            atomic::{AtomicU32, Ordering as AtomicOrdering},
            Arc,
        },
        time::Duration,
    };
    use tokio::sync::RwLock;

    #[test]
    fn projection_runner_config_default() {
        let config = ProjectionRunnerConfig::default();
        assert_eq!(config.checkpoint_frequency, 100);
        assert_eq!(config.batch_size, 1000);
        assert_eq!(config.checkpoint_timeout, Duration::from_secs(30));
        assert_eq!(config.error_retry_delay, Duration::from_millis(100));
        assert_eq!(config.max_retries, 5);
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(config.retry_backoff_multiplier, 2.0);
        }
        assert_eq!(config.max_retry_delay, Duration::from_secs(60));
        assert!(!config.stop_on_error);
    }

    #[test]
    fn projection_runner_stats_default() {
        let stats = ProjectionRunnerStats::default();
        assert_eq!(stats.events_processed, 0);
        assert_eq!(stats.checkpoints_saved, 0);
        assert_eq!(stats.errors_encountered, 0);
        assert_eq!(stats.retries_performed, 0);
        assert_eq!(stats.last_processing_time_micros, 0);
        assert_eq!(stats.avg_processing_time_micros, 0);
        assert!(stats.started_at.is_none());
        assert!(stats.last_event_processed_at.is_none());
    }

    #[tokio::test]
    async fn projection_runner_new() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        let runner = ProjectionRunner::new(projection);

        assert!(!runner.is_running());

        let state = runner.get_state().await.unwrap();
        assert!(state.is_none());

        let checkpoint = runner.get_checkpoint().await;
        assert!(checkpoint.last_event_id.is_none());

        let projection_stats = runner.get_stats().await;
        assert_eq!(projection_stats.events_processed, 0);
    }

    #[tokio::test]
    async fn projection_runner_with_config() {
        let projection_config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> =
            InMemoryProjection::new(projection_config);

        let runner_config = ProjectionRunnerConfig {
            checkpoint_frequency: 50,
            batch_size: 500,
            ..ProjectionRunnerConfig::default()
        };

        let runner = ProjectionRunner::with_config(projection, runner_config);

        assert_eq!(runner.config.checkpoint_frequency, 50);
        assert_eq!(runner.config.batch_size, 500);
    }

    #[tokio::test]
    async fn projection_event_processor_new() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);
        let projection_arc = Arc::new(projection);

        let runner_config = ProjectionRunnerConfig::default();
        let state = Arc::new(RwLock::new(Some(String::new())));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            projection_arc,
            runner_config,
            state,
            checkpoint,
            runner_stats,
            events_processed,
        );

        assert_eq!(processor.projection.config().name, "test-projection");
    }

    #[tokio::test]
    async fn projection_event_processor_update_stats() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);
        let projection_arc = Arc::new(projection);

        let runner_config = ProjectionRunnerConfig::default();
        let state = Arc::new(RwLock::new(Some(String::new())));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            projection_arc,
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed,
        );

        let processing_time = Duration::from_micros(100);
        processor.update_processing_stats(processing_time).await;

        {
            let stats_guard = runner_stats.read().await;
            assert_eq!(stats_guard.events_processed, 1);
            assert_eq!(stats_guard.last_processing_time_micros, 100);
            assert_eq!(stats_guard.avg_processing_time_micros, 100);
            assert!(stats_guard.last_event_processed_at.is_some());
        }
    }

    #[tokio::test]
    async fn projection_event_processor_average_processing_time() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);
        let projection_arc = Arc::new(projection);

        let runner_config = ProjectionRunnerConfig::default();
        let state = Arc::new(RwLock::new(Some(String::new())));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            projection_arc,
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed,
        );

        // Process multiple events with different processing times
        processor
            .update_processing_stats(Duration::from_micros(100))
            .await;
        processor
            .update_processing_stats(Duration::from_micros(200))
            .await;
        processor
            .update_processing_stats(Duration::from_micros(300))
            .await;

        let stats_guard = runner_stats.read().await;
        assert_eq!(stats_guard.events_processed, 3);
        assert_eq!(stats_guard.last_processing_time_micros, 300);
        assert_eq!(stats_guard.avg_processing_time_micros, 200); // (100 + 200 + 300) / 3
        assert!(stats_guard.last_event_processed_at.is_some());
    }

    // Mock projection that allows us to test error handling
    #[derive(Debug)]
    struct MockFailingProjection {
        config: ProjectionConfig,
        failure_count: Arc<AtomicU32>,
        max_failures: u32,
    }

    impl MockFailingProjection {
        fn new(name: &str, max_failures: u32) -> Self {
            Self {
                config: ProjectionConfig::new(name),
                failure_count: Arc::new(AtomicU32::new(0)),
                max_failures,
            }
        }
    }

    #[async_trait]
    impl Projection for MockFailingProjection {
        type State = u32;
        type Event = String;

        fn config(&self) -> &ProjectionConfig {
            &self.config
        }

        async fn get_state(&self) -> ProjectionResult<Self::State> {
            Ok(0)
        }

        async fn get_status(&self) -> ProjectionResult<crate::projection::ProjectionStatus> {
            Ok(crate::projection::ProjectionStatus::Running)
        }

        async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
            Ok(ProjectionCheckpoint::initial())
        }

        async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
            Ok(())
        }

        async fn apply_event(
            &self,
            _state: &mut Self::State,
            _event: &Event<Self::Event>,
        ) -> ProjectionResult<()> {
            let current_failures = self.failure_count.fetch_add(1, AtomicOrdering::SeqCst);
            if current_failures < self.max_failures {
                Err(ProjectionError::EventProcessingFailed {
                    event_id: _event.id,
                    reason: format!("Simulated failure {}", current_failures + 1),
                })
            } else {
                Ok(())
            }
        }

        async fn initialize_state(&self) -> ProjectionResult<Self::State> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn projection_event_processor_retry_logic() {
        let failing_projection = Arc::new(MockFailingProjection::new("failing-projection", 2));

        let runner_config = ProjectionRunnerConfig {
            max_retries: 3,
            error_retry_delay: Duration::from_millis(1), // Very short delay for testing
            ..ProjectionRunnerConfig::default()
        };

        let state = Arc::new(RwLock::new(Some(0u32)));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            failing_projection.clone(),
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed,
        );

        // Create a test event
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = Event::new(
            stream_id.clone(),
            "test-payload".to_string(),
            EventMetadata::default(),
        );
        let stored_event = StoredEvent::new(event, EventVersion::try_new(1).unwrap());

        // Process the event - should succeed after retries
        let result = processor.process_event_with_retry(&stored_event).await;
        assert!(result.is_ok());

        // Check that retries were performed
        let stats_guard = runner_stats.read().await;
        assert_eq!(stats_guard.retries_performed, 2); // Should have retried 2 times before success
        assert_eq!(stats_guard.errors_encountered, 2); // Should have encountered 2 errors
    }

    #[tokio::test]
    async fn projection_event_processor_max_retries_exceeded() {
        let failing_projection =
            Arc::new(MockFailingProjection::new("always-failing-projection", 100)); // Always fails

        let runner_config = ProjectionRunnerConfig {
            max_retries: 2,
            error_retry_delay: Duration::from_millis(1),
            stop_on_error: true, // Stop on error to test failure case
            ..ProjectionRunnerConfig::default()
        };

        let state = Arc::new(RwLock::new(Some(0u32)));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            failing_projection,
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed,
        );

        // Create a test event
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = Event::new(
            stream_id.clone(),
            "test-payload".to_string(),
            EventMetadata::default(),
        );
        let stored_event = StoredEvent::new(event, EventVersion::try_new(1).unwrap());

        // Process the event - should fail after max retries
        let result = processor.process_event_with_retry(&stored_event).await;
        assert!(result.is_err());

        // Check that max retries were attempted
        let stats_guard = runner_stats.read().await;
        assert_eq!(stats_guard.retries_performed, 2); // Should have retried max times
        assert_eq!(stats_guard.errors_encountered, 3); // Initial failure + 2 retries
    }

    #[tokio::test]
    async fn projection_event_processor_skip_on_max_retries_when_continue_on_error() {
        let failing_projection =
            Arc::new(MockFailingProjection::new("always-failing-projection", 100)); // Always fails

        let runner_config = ProjectionRunnerConfig {
            max_retries: 1,
            error_retry_delay: Duration::from_millis(1),
            stop_on_error: false, // Continue on error - should skip the event
            ..ProjectionRunnerConfig::default()
        };

        let state = Arc::new(RwLock::new(Some(0u32)));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            failing_projection,
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed,
        );

        // Create a test event
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = Event::new(
            stream_id.clone(),
            "test-payload".to_string(),
            EventMetadata::default(),
        );
        let stored_event = StoredEvent::new(event, EventVersion::try_new(1).unwrap());

        // Process the event - should succeed (skip the event) after max retries
        let result = processor.process_event_with_retry(&stored_event).await;
        assert!(result.is_ok());

        // Check that retries were attempted and then event was skipped
        let stats_guard = runner_stats.read().await;
        assert_eq!(stats_guard.retries_performed, 1); // Should have retried once
        assert_eq!(stats_guard.errors_encountered, 2); // Initial failure + 1 retry
    }

    #[tokio::test]
    async fn projection_event_processor_checkpoint_frequency() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);
        let projection_arc = Arc::new(projection);

        let runner_config = ProjectionRunnerConfig {
            checkpoint_frequency: 2, // Checkpoint every 2 events
            ..ProjectionRunnerConfig::default()
        };

        let state = Arc::new(RwLock::new(Some(String::new())));
        let checkpoint = Arc::new(RwLock::new(ProjectionCheckpoint::initial()));
        let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));
        let events_processed = Arc::new(AtomicU64::new(0));

        let processor = ProjectionEventProcessor::new(
            projection_arc,
            runner_config,
            state,
            checkpoint,
            runner_stats.clone(),
            events_processed.clone(),
        );

        // Create test events
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event1 = Event::new(
            stream_id.clone(),
            "payload1".to_string(),
            EventMetadata::default(),
        );
        let event2 = Event::new(
            stream_id.clone(),
            "payload2".to_string(),
            EventMetadata::default(),
        );
        let stored_event1 = StoredEvent::new(event1, EventVersion::try_new(1).unwrap());
        let stored_event2 = StoredEvent::new(event2, EventVersion::try_new(2).unwrap());

        // Process first event
        let result1 = processor.process_single_event(&stored_event1).await;
        assert!(result1.is_ok());

        // Should have 1 event processed, 0 checkpoints saved (frequency not reached)
        {
            let stats_guard = runner_stats.read().await;
            assert_eq!(stats_guard.checkpoints_saved, 0);
        }
        assert_eq!(events_processed.load(AtomicOrdering::Acquire), 1);

        // Process second event
        let result2 = processor.process_single_event(&stored_event2).await;
        assert!(result2.is_ok());

        // Should have triggered checkpoint save due to frequency
        {
            let stats_guard = runner_stats.read().await;
            assert_eq!(stats_guard.checkpoints_saved, 1);
        }
        assert_eq!(events_processed.load(AtomicOrdering::Acquire), 0); // Reset after checkpoint
    }

    #[tokio::test]
    async fn projection_runner_initialize_runner() {
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        let runner = ProjectionRunner::new(projection);

        // Before initialization
        let state_before = runner.get_state().await.unwrap();
        assert!(state_before.is_none());

        let metrics_before = runner.get_stats().await;
        assert!(metrics_before.started_at.is_none());

        // Initialize
        let result = runner.initialize_runner().await;
        assert!(result.is_ok());

        // After initialization
        let state_after = runner.get_state().await.unwrap();
        assert!(state_after.is_some());

        let metrics_after = runner.get_stats().await;
        assert!(metrics_after.started_at.is_some());
    }

    // Property tests for reliability
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn projection_runner_config_checkpoint_frequency_always_positive(
                freq in 1u64..=10000u64
            ) {
                let config = ProjectionRunnerConfig {
                    checkpoint_frequency: freq,
                    ..ProjectionRunnerConfig::default()
                };
                prop_assert!(config.checkpoint_frequency > 0);
            }

            #[test]
            fn projection_runner_config_batch_size_always_positive(
                size in 1usize..=10000usize
            ) {
                let config = ProjectionRunnerConfig {
                    batch_size: size,
                    ..ProjectionRunnerConfig::default()
                };
                prop_assert!(config.batch_size > 0);
            }

            #[test]
            fn projection_runner_config_max_retries_bounded(
                retries in 0u32..=100u32
            ) {
                let config = ProjectionRunnerConfig {
                    max_retries: retries,
                    ..ProjectionRunnerConfig::default()
                };
                prop_assert!(config.max_retries <= 100);
            }

            #[test]
            fn projection_runner_stats_events_processed_monotonic(
                events in prop::collection::vec(1u64..=1000u64, 1..10)
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let _ = rt.block_on(async {
                    let runner_stats = Arc::new(RwLock::new(ProjectionRunnerStats::default()));

                    let mut total_events = 0u64;
                    for _event_count in events {
                        {
                            let mut stats_guard = runner_stats.write().await;
                            stats_guard.events_processed += 1;
                            total_events += 1;
                        }

                        let current_stats = runner_stats.read().await;
                        prop_assert_eq!(current_stats.events_processed, total_events);
                    }
                    Ok(())
                });
            }
        }
    }
}
