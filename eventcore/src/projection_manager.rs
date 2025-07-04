//! Projection management system for the `EventCore` event sourcing library.
//!
//! This module provides the `ProjectionManager` which handles the lifecycle of
//! projections including starting, stopping, pausing, resuming, and rebuilding
//! projections while monitoring their health and performance.

#![allow(clippy::significant_drop_tightening)]

use crate::errors::{ProjectionError, ProjectionResult};
use crate::event_store::EventStore;
use crate::projection::{Projection, ProjectionCheckpoint, ProjectionStatus};
use crate::subscription::{Subscription, SubscriptionOptions};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument};

/// Type alias for the projection wrapper storage.
type ProjectionMap<E> = Arc<RwLock<HashMap<String, Box<dyn ProjectionWrapper<E>>>>>;

/// Type alias for the subscription storage.
type SubscriptionMap<E> = Arc<Mutex<HashMap<String, Box<dyn Subscription<Event = E>>>>>;

/// A wrapper trait to make Projection trait object-safe by erasing the State type.
#[async_trait::async_trait]
pub trait ProjectionWrapper<E>: Send + Sync + Debug {
    /// Gets the current status of the projection.
    async fn get_status(&self) -> ProjectionResult<ProjectionStatus>;

    /// Loads the projection's checkpoint from storage.
    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint>;

    /// Saves the projection's checkpoint to storage.
    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()>;

    /// Initializes the projection's state.
    async fn initialize_state(&self) -> ProjectionResult<()>;

    /// Called when the projection is started.
    async fn on_start(&self) -> ProjectionResult<()>;

    /// Called when the projection is stopped.
    async fn on_stop(&self) -> ProjectionResult<()>;

    /// Called when the projection is paused.
    async fn on_pause(&self) -> ProjectionResult<()>;

    /// Called when the projection is resumed.
    async fn on_resume(&self) -> ProjectionResult<()>;

    /// Called when the projection encounters an error.
    async fn on_error(&self, error: &ProjectionError) -> ProjectionResult<()>;

    /// Gets the name of the projection configuration.
    fn config_name(&self) -> &str;

    /// Gets the list of streams this projection is interested in.
    fn interested_streams(&self) -> Vec<crate::types::StreamId>;
}

/// Implementation of `ProjectionWrapper` for any Projection.
#[derive(Debug)]
pub struct ProjectionWrapperImpl<P> {
    projection: P,
}

impl<P> ProjectionWrapperImpl<P> {
    /// Creates a new projection wrapper.
    pub const fn new(projection: P) -> Self {
        Self { projection }
    }
}

#[async_trait::async_trait]
impl<P, E> ProjectionWrapper<E> for ProjectionWrapperImpl<P>
where
    P: Projection<Event = E> + Send + Sync + Debug + 'static,
    P::State: Send + Sync + Debug + Clone + 'static,
    E: Send + Sync + Debug + PartialEq + Eq + 'static,
{
    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        self.projection.get_status().await
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        self.projection.load_checkpoint().await
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        self.projection.save_checkpoint(checkpoint).await
    }

    async fn initialize_state(&self) -> ProjectionResult<()> {
        self.projection.initialize_state().await?;
        Ok(())
    }

    async fn on_start(&self) -> ProjectionResult<()> {
        self.projection.on_start().await
    }

    async fn on_stop(&self) -> ProjectionResult<()> {
        self.projection.on_stop().await
    }

    async fn on_pause(&self) -> ProjectionResult<()> {
        self.projection.on_pause().await
    }

    async fn on_resume(&self) -> ProjectionResult<()> {
        self.projection.on_resume().await
    }

    async fn on_error(&self, error: &ProjectionError) -> ProjectionResult<()> {
        self.projection.on_error(error).await
    }

    fn config_name(&self) -> &str {
        &self.projection.config().name
    }

    fn interested_streams(&self) -> Vec<crate::types::StreamId> {
        self.projection.interested_streams()
    }
}

/// Health information for a projection.
#[derive(Debug, Clone)]
pub struct ProjectionHealth {
    /// The current status of the projection.
    pub status: ProjectionStatus,
    /// The last time the projection processed an event.
    pub last_activity: Option<Instant>,
    /// The number of events processed since start.
    pub events_processed: u64,
    /// The number of errors encountered.
    pub error_count: u64,
    /// The last error message, if any.
    pub last_error: Option<String>,
    /// The current checkpoint position.
    pub current_checkpoint: ProjectionCheckpoint,
    /// Whether the projection is healthy (no recent errors, processing events).
    pub is_healthy: bool,
}

impl ProjectionHealth {
    /// Creates initial health state for a projection.
    pub const fn initial(checkpoint: ProjectionCheckpoint) -> Self {
        Self {
            status: ProjectionStatus::Stopped,
            last_activity: None,
            events_processed: 0,
            error_count: 0,
            last_error: None,
            current_checkpoint: checkpoint,
            is_healthy: true,
        }
    }

    /// Updates the health after processing an event.
    pub fn record_event_processed(&mut self) {
        self.last_activity = Some(Instant::now());
        self.events_processed += 1;
        self.is_healthy = true;
    }

    /// Records an error in projection processing.
    pub fn record_error(&mut self, error: &str) {
        self.error_count += 1;
        self.last_error = Some(error.to_string());
        self.is_healthy = false;
    }

    /// Updates the status.
    pub fn set_status(&mut self, status: ProjectionStatus) {
        self.status = status;
    }

    /// Updates the checkpoint.
    pub fn set_checkpoint(&mut self, checkpoint: ProjectionCheckpoint) {
        self.current_checkpoint = checkpoint;
    }
}

/// Configuration for the projection manager.
#[derive(Debug, Clone)]
pub struct ProjectionManagerConfig {
    /// How often to check projection health.
    pub health_check_interval: Duration,
    /// Maximum number of consecutive errors before marking unhealthy.
    pub max_consecutive_errors: u64,
    /// Time after which a projection is considered stale if no activity.
    pub stale_threshold: Duration,
}

impl Default for ProjectionManagerConfig {
    fn default() -> Self {
        Self {
            health_check_interval: Duration::from_secs(30),
            max_consecutive_errors: 5,
            stale_threshold: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Manages the lifecycle and health of projections.
///
/// The `ProjectionManager` is responsible for:
/// - Starting and stopping projections
/// - Pausing and resuming projections
/// - Rebuilding projections from scratch
/// - Monitoring projection health
/// - Managing subscription lifecycles
pub struct ProjectionManager<E> {
    event_store: Arc<dyn EventStore<Event = E>>,
    projections: ProjectionMap<E>,
    health_status: Arc<RwLock<HashMap<String, ProjectionHealth>>>,
    running_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    subscriptions: SubscriptionMap<E>,
    config: ProjectionManagerConfig,
}

impl<E> ProjectionManager<E>
where
    E: Send + Sync + Debug + PartialEq + Eq + 'static,
{
    /// Creates a new projection manager.
    pub fn new(event_store: Arc<dyn EventStore<Event = E>>) -> Self {
        Self {
            event_store,
            projections: Arc::new(RwLock::new(HashMap::new())),
            health_status: Arc::new(RwLock::new(HashMap::new())),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            config: ProjectionManagerConfig::default(),
        }
    }

    /// Creates a new projection manager with custom configuration.
    pub fn with_config(
        event_store: Arc<dyn EventStore<Event = E>>,
        config: ProjectionManagerConfig,
    ) -> Self {
        Self {
            event_store,
            projections: Arc::new(RwLock::new(HashMap::new())),
            health_status: Arc::new(RwLock::new(HashMap::new())),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Registers a projection with the manager.
    #[instrument(skip(self, projection))]
    pub async fn register_projection<P>(&self, projection: P) -> ProjectionResult<()>
    where
        P: Projection<Event = E> + Send + Sync + Debug + 'static,
        P::State: Send + Sync + Debug + Clone + 'static,
    {
        let name = projection.config().name.clone();

        info!("Registering projection: {}", name);

        // Load initial checkpoint
        let checkpoint = projection.load_checkpoint().await?;
        let health = ProjectionHealth::initial(checkpoint);

        let wrapped = Box::new(ProjectionWrapperImpl::new(projection));

        {
            let mut projections = self.projections.write().await;
            let mut health_status = self.health_status.write().await;

            projections.insert(name.clone(), wrapped);
            health_status.insert(name.clone(), health);
        }

        debug!("Successfully registered projection: {}", name);
        Ok(())
    }

    /// Unregisters a projection from the manager.
    #[instrument(skip(self))]
    pub async fn unregister_projection(&self, name: &str) -> ProjectionResult<()> {
        info!("Unregistering projection: {}", name);

        // Stop the projection if running
        if self.is_running(name).await? {
            self.stop(name).await?;
        }

        // Remove from collections
        {
            let mut projections = self.projections.write().await;
            let mut health_status = self.health_status.write().await;

            projections.remove(name);
            health_status.remove(name);
        }

        debug!("Successfully unregistered projection: {}", name);
        Ok(())
    }

    /// Starts a projection.
    #[instrument(skip(self))]
    pub async fn start(&self, name: &str) -> ProjectionResult<()> {
        info!("Starting projection: {}", name);

        // Check if projection exists and get status
        let current_status = {
            let projections = self.projections.read().await;
            let projection = projections
                .get(name)
                .ok_or_else(|| ProjectionError::NotFound(name.to_string()))?;
            projection.get_status().await?
        };

        if !current_status.can_start() {
            return Err(ProjectionError::InvalidStateTransition {
                from: current_status,
                to: ProjectionStatus::Running,
            });
        }

        // Initialize state if needed
        if current_status == ProjectionStatus::Stopped {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            projection.initialize_state().await?;
        }

        // Call lifecycle hook
        {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            projection.on_start().await?;
        }

        // Update health status
        {
            let mut health_status = self.health_status.write().await;
            if let Some(health) = health_status.get_mut(name) {
                health.set_status(ProjectionStatus::Running);
            }
        }

        // Start the projection task
        let task = {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            self.start_projection_task(name.to_string(), projection.as_ref())
                .await?
        };

        // Store the task handle
        {
            let mut running_tasks = self.running_tasks.lock().await;
            running_tasks.insert(name.to_string(), task);
        }

        info!("Successfully started projection: {}", name);
        Ok(())
    }

    /// Stops a projection.
    #[instrument(skip(self))]
    pub async fn stop(&self, name: &str) -> ProjectionResult<()> {
        info!("Stopping projection: {}", name);

        // Check if projection exists and get status
        let current_status = {
            let projections = self.projections.read().await;
            let projection = projections
                .get(name)
                .ok_or_else(|| ProjectionError::NotFound(name.to_string()))?;
            projection.get_status().await?
        };

        if !current_status.can_stop() {
            return Err(ProjectionError::InvalidStateTransition {
                from: current_status,
                to: ProjectionStatus::Stopped,
            });
        }

        // Stop the running task
        {
            let mut running_tasks = self.running_tasks.lock().await;
            if let Some(task) = running_tasks.remove(name) {
                task.abort();
                // Wait for task to complete
                let _ = task.await;
            }
        }

        // Stop subscription
        {
            let mut subscriptions = self.subscriptions.lock().await;
            if let Some(mut subscription) = subscriptions.remove(name) {
                subscription
                    .stop()
                    .await
                    .map_err(|e| ProjectionError::SubscriptionFailed(e.to_string()))?;
            }
        }

        // Call lifecycle hook
        {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            projection.on_stop().await?;
        }

        // Update health status
        {
            let mut health_status = self.health_status.write().await;
            if let Some(health) = health_status.get_mut(name) {
                health.set_status(ProjectionStatus::Stopped);
            }
        }

        info!("Successfully stopped projection: {}", name);
        Ok(())
    }

    /// Pauses a projection.
    #[instrument(skip(self))]
    pub async fn pause(&self, name: &str) -> ProjectionResult<()> {
        info!("Pausing projection: {}", name);

        // Check if projection exists and can be paused
        let current_status = {
            let projections = self.projections.read().await;
            let projection = projections
                .get(name)
                .ok_or_else(|| ProjectionError::NotFound(name.to_string()))?;
            projection.get_status().await?
        };

        if !current_status.can_pause() {
            return Err(ProjectionError::InvalidStateTransition {
                from: current_status,
                to: ProjectionStatus::Paused,
            });
        }

        // Pause subscription
        {
            let mut subscriptions = self.subscriptions.lock().await;
            if let Some(subscription) = subscriptions.get_mut(name) {
                subscription
                    .pause()
                    .await
                    .map_err(|e| ProjectionError::SubscriptionFailed(e.to_string()))?;
            }
        }

        // Call lifecycle hook
        {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            projection.on_pause().await?;
        }

        // Update health status
        {
            let mut health_status = self.health_status.write().await;
            if let Some(health) = health_status.get_mut(name) {
                health.set_status(ProjectionStatus::Paused);
            }
        }

        info!("Successfully paused projection: {}", name);
        Ok(())
    }

    /// Resumes a paused projection.
    #[instrument(skip(self))]
    pub async fn resume(&self, name: &str) -> ProjectionResult<()> {
        info!("Resuming projection: {}", name);

        // Check if projection exists and can be resumed
        let current_status = {
            let projections = self.projections.read().await;
            let projection = projections
                .get(name)
                .ok_or_else(|| ProjectionError::NotFound(name.to_string()))?;
            projection.get_status().await?
        };

        if current_status != ProjectionStatus::Paused {
            return Err(ProjectionError::InvalidStateTransition {
                from: current_status,
                to: ProjectionStatus::Running,
            });
        }

        // Resume subscription
        {
            let mut subscriptions = self.subscriptions.lock().await;
            if let Some(subscription) = subscriptions.get_mut(name) {
                subscription
                    .resume()
                    .await
                    .map_err(|e| ProjectionError::SubscriptionFailed(e.to_string()))?;
            }
        }

        // Call lifecycle hook
        {
            let projections = self.projections.read().await;
            let projection = projections.get(name).unwrap();
            projection.on_resume().await?;
        }

        // Update health status
        {
            let mut health_status = self.health_status.write().await;
            if let Some(health) = health_status.get_mut(name) {
                health.set_status(ProjectionStatus::Running);
            }
        }

        info!("Successfully resumed projection: {}", name);
        Ok(())
    }

    /// Rebuilds a projection from the beginning.
    #[instrument(skip(self))]
    pub async fn rebuild(&self, name: &str) -> ProjectionResult<()> {
        info!("Rebuilding projection: {}", name);

        // Stop the projection if running
        self.stop(name).await.ok(); // Ignore errors if already stopped

        // Update health status
        {
            let mut health_status = self.health_status.write().await;
            if let Some(health) = health_status.get_mut(name) {
                let initial_checkpoint = ProjectionCheckpoint::initial();
                health.set_status(ProjectionStatus::Rebuilding);
                health.set_checkpoint(initial_checkpoint);
                health.events_processed = 0;
                health.error_count = 0;
                health.last_error = None;
                health.is_healthy = true;
            } else {
                return Err(ProjectionError::NotFound(name.to_string()));
            }
        }

        info!("Successfully started rebuilding projection: {}", name);
        Ok(())
    }

    /// Gets the health status of a projection.
    pub async fn get_health(&self, name: &str) -> ProjectionResult<ProjectionHealth> {
        let health_status = self.health_status.read().await;
        health_status
            .get(name)
            .cloned()
            .ok_or_else(|| ProjectionError::NotFound(name.to_string()))
    }

    /// Gets the health status of all projections.
    pub async fn get_all_health(&self) -> HashMap<String, ProjectionHealth> {
        let health_status = self.health_status.read().await;
        health_status.clone()
    }

    /// Checks if a projection is currently running.
    pub async fn is_running(&self, name: &str) -> ProjectionResult<bool> {
        let running_tasks = self.running_tasks.lock().await;
        Ok(running_tasks.contains_key(name))
    }

    /// Gets the names of all registered projections.
    pub async fn list_projections(&self) -> Vec<String> {
        let projections = self.projections.read().await;
        projections.keys().cloned().collect()
    }

    /// Starts health monitoring for all projections.
    pub fn start_health_monitoring(&self) -> JoinHandle<()> {
        let health_check_interval = self.config.health_check_interval;

        // Create shared references
        let health_status_ref = Arc::clone(&self.health_status);
        let projections_ref = Arc::clone(&self.projections);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(health_check_interval);
            loop {
                interval.tick().await;

                let projection_names: Vec<String> = {
                    let projections_guard = projections_ref.read().await;
                    projections_guard.keys().cloned().collect()
                };

                for name in projection_names {
                    if let Err(e) = Self::check_projection_health(&name, &health_status_ref).await {
                        error!("Failed to check health for projection {}: {}", name, e);
                    }
                }
            }
        })
    }

    /// Internal method to start a projection processing task.
    async fn start_projection_task(
        &self,
        name: String,
        projection: &dyn ProjectionWrapper<E>,
    ) -> ProjectionResult<JoinHandle<()>> {
        // Create subscription
        let streams = projection.interested_streams();
        let checkpoint = projection.load_checkpoint().await?;
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

        let subscription = self.event_store.subscribe(subscription_options).await?;

        // Store subscription
        {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions.insert(name.clone(), subscription);
        }

        // Start processing task
        let task_name = name.clone();

        let task = tokio::spawn(async move {
            debug!("Projection task started for: {}", task_name);
            // TODO: Implement actual event processing loop here
            // This would involve:
            // 1. Reading events from the subscription
            // 2. Processing them through the projection
            // 3. Updating checkpoints
            // 4. Updating health status
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            debug!("Projection task completed for: {}", task_name);
        });

        Ok(task)
    }

    /// Internal method to check the health of a specific projection.
    async fn check_projection_health(
        name: &str,
        health_status: &Arc<RwLock<HashMap<String, ProjectionHealth>>>,
    ) -> ProjectionResult<()> {
        debug!("Checking health for projection: {}", name);

        // Simple health check implementation
        let mut health_guard = health_status.write().await;
        if let Some(health) = health_guard.get_mut(name) {
            // Check if projection has been inactive for too long
            if let Some(last_activity) = health.last_activity {
                let now = Instant::now();
                let duration_since_activity = now.duration_since(last_activity);

                // Mark as unhealthy if no activity for more than 5 minutes
                if duration_since_activity > Duration::from_secs(300) {
                    health.is_healthy = false;
                    health.record_error("Projection has been inactive for too long");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{InMemoryProjection, ProjectionConfig};
    use std::collections::HashMap;

    /// Mock event store for testing
    struct MockEventStore;

    #[async_trait::async_trait]
    impl EventStore for MockEventStore {
        type Event = String;

        async fn read_streams(
            &self,
            _stream_ids: &[crate::types::StreamId],
            _options: &crate::event_store::ReadOptions,
        ) -> crate::errors::EventStoreResult<crate::event_store::StreamData<Self::Event>> {
            Ok(crate::event_store::StreamData::new(vec![], HashMap::new()))
        }

        async fn write_events_multi(
            &self,
            _stream_events: Vec<crate::event_store::StreamEvents<Self::Event>>,
        ) -> crate::errors::EventStoreResult<
            HashMap<crate::types::StreamId, crate::types::EventVersion>,
        > {
            Ok(HashMap::new())
        }

        async fn stream_exists(
            &self,
            _stream_id: &crate::types::StreamId,
        ) -> crate::errors::EventStoreResult<bool> {
            Ok(false)
        }

        async fn get_stream_version(
            &self,
            _stream_id: &crate::types::StreamId,
        ) -> crate::errors::EventStoreResult<Option<crate::types::EventVersion>> {
            Ok(None)
        }

        async fn subscribe(
            &self,
            _options: crate::subscription::SubscriptionOptions,
        ) -> crate::errors::EventStoreResult<
            Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
        > {
            let subscription =
                crate::subscription::SubscriptionImpl::new(std::sync::Arc::new(Self));
            Ok(Box::new(subscription))
        }
    }

    #[tokio::test]
    async fn projection_manager_register_and_unregister() {
        let event_store = Arc::new(MockEventStore);
        let manager: ProjectionManager<String> = ProjectionManager::new(event_store);

        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        // Register projection
        manager.register_projection(projection).await.unwrap();

        // Check it's registered
        let projections = manager.list_projections().await;
        assert!(projections.contains(&"test-projection".to_string()));

        // Unregister projection
        manager
            .unregister_projection("test-projection")
            .await
            .unwrap();

        // Check it's no longer registered
        let projections = manager.list_projections().await;
        assert!(!projections.contains(&"test-projection".to_string()));
    }

    #[tokio::test]
    async fn projection_manager_get_health() {
        let event_store = Arc::new(MockEventStore);
        let manager: ProjectionManager<String> = ProjectionManager::new(event_store);

        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        manager.register_projection(projection).await.unwrap();

        let health = manager.get_health("test-projection").await.unwrap();
        assert_eq!(health.status, ProjectionStatus::Stopped);
        assert_eq!(health.events_processed, 0);
        assert!(health.is_healthy);
    }

    #[tokio::test]
    async fn projection_manager_not_found_error() {
        let event_store = Arc::new(MockEventStore);
        let manager: ProjectionManager<String> = ProjectionManager::new(event_store);

        let result = manager.get_health("nonexistent").await;
        assert!(matches!(result, Err(ProjectionError::NotFound(_))));
    }

    #[tokio::test]
    async fn projection_health_record_event() {
        let checkpoint = ProjectionCheckpoint::initial();
        let mut health = ProjectionHealth::initial(checkpoint);

        assert_eq!(health.events_processed, 0);
        assert!(health.last_activity.is_none());

        health.record_event_processed();

        assert_eq!(health.events_processed, 1);
        assert!(health.last_activity.is_some());
        assert!(health.is_healthy);
    }

    #[tokio::test]
    async fn projection_health_record_error() {
        let checkpoint = ProjectionCheckpoint::initial();
        let mut health = ProjectionHealth::initial(checkpoint);

        assert_eq!(health.error_count, 0);
        assert!(health.last_error.is_none());
        assert!(health.is_healthy);

        health.record_error("Test error");

        assert_eq!(health.error_count, 1);
        assert_eq!(health.last_error, Some("Test error".to_string()));
        assert!(!health.is_healthy);
    }
}
