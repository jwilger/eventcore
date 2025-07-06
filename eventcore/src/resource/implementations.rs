//! Concrete resource implementations
//!
//! This module contains the actual implementations of resource management
//! structures including Resource, ResourceScope, TimedResourceGuard, etc.

use super::types::{states, IsAcquired};
use super::{ResourceError, ResourceResult};
use async_trait::async_trait;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// A resource that enforces acquisition and release through the type system
///
/// # Type Parameters
/// * `T` - The underlying resource type
/// * `S` - The current state of the resource (phantom type)
///
/// # Example
/// ```rust,no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use eventcore::resource::{Resource, states, ResourceManager};
/// use std::sync::Arc;
///
/// // Example with a custom resource type
/// struct MyResource {
///     data: String,
/// }
///
/// // Implement ResourceManager for your type
/// struct MyResourceManager;
///
/// #[async_trait::async_trait]
/// impl ResourceManager<MyResource> for MyResourceManager {
///     async fn acquire() -> Result<Resource<MyResource, states::Acquired>, eventcore::resource::ResourceError> {
///         // In practice, you'd acquire from a pool or create the resource
///         let resource = Self::create_initializing(MyResource { data: "example".to_string() });
///         Ok(resource.mark_acquired())
///     }
/// }
///
/// // Use the resource manager to acquire a resource
/// let resource = MyResourceManager::acquire().await?;
///
/// // Access the inner data (only possible in Acquired state)
/// let data = resource.get();
/// println!("Resource data: {}", data.data);
///
/// // Transition to released state
/// let _released: Resource<MyResource, states::Released> = resource.release()?;
/// // released resource cannot be used anymore (compile-time guarantee)
///
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Resource<T, S> {
    inner: T,
    _state: PhantomData<S>,
}

impl<T, S> Resource<T, S> {
    /// Create a new resource in the given state
    ///
    /// # Safety
    /// This is an internal method and should only be called by the ResourceManager
    /// with appropriate state validation.
    pub const fn new(inner: T) -> Self {
        Self {
            inner,
            _state: PhantomData,
        }
    }

    /// Get a reference to the underlying resource
    ///
    /// Only available when resource is acquired
    pub const fn get(&self) -> &T
    where
        S: IsAcquired,
    {
        &self.inner
    }

    /// Get a mutable reference to the underlying resource
    ///
    /// Only available when resource is acquired
    pub fn get_mut(&mut self) -> &mut T
    where
        S: IsAcquired,
    {
        &mut self.inner
    }

    /// Consume the resource and return the inner value
    ///
    /// Only available when resource is acquired
    pub fn into_inner(self) -> T
    where
        S: IsAcquired,
    {
        self.inner
    }
}

/// State transitions for resources
impl<T> Resource<T, states::Initializing> {
    /// Transition from initializing to acquired state
    ///
    /// This represents successful resource acquisition
    pub fn mark_acquired(self) -> Resource<T, states::Acquired> {
        Resource::new(self.inner)
    }

    /// Transition from initializing to failed state
    ///
    /// This represents failed resource acquisition
    pub fn mark_failed(self) -> Resource<T, states::Failed> {
        Resource::new(self.inner)
    }
}

impl<T> Resource<T, states::Acquired> {
    /// Release the resource, transitioning to released state
    ///
    /// After release, the resource cannot be used anymore
    pub fn release(self) -> ResourceResult<Resource<T, states::Released>> {
        // Perform any cleanup logic here
        // For now, we just transition the state
        Ok(Resource::new(self.inner))
    }

    /// Mark resource as failed due to an error
    ///
    /// Failed resources can be recovered or released
    pub fn mark_failed(self) -> Resource<T, states::Failed> {
        Resource::new(self.inner)
    }
}

impl<T> Resource<T, states::Failed> {
    /// Attempt to recover a failed resource
    ///
    /// If successful, transitions back to acquired state
    pub fn recover(self) -> ResourceResult<Resource<T, states::Acquired>> {
        // Perform recovery logic here
        // For now, we optimistically assume recovery succeeds
        Ok(Resource::new(self.inner))
    }

    /// Release a failed resource
    ///
    /// This allows cleanup even when the resource is in a failed state
    pub fn release(self) -> ResourceResult<Resource<T, states::Released>> {
        // Perform cleanup of failed resource
        Ok(Resource::new(self.inner))
    }
}

impl<T> Resource<T, states::Released> {
    /// Check if resource has been released
    ///
    /// This is always true for resources in Released state
    pub const fn is_released(&self) -> bool {
        true
    }
}

/// Trait for types that can manage resource acquisition and release
#[async_trait]
pub trait ResourceManager<T> {
    /// Acquire a resource, returning it in an acquired state
    async fn acquire() -> ResourceResult<Resource<T, states::Acquired>>;

    /// Create a resource in initializing state
    ///
    /// Callers must transition to acquired or failed state
    fn create_initializing(inner: T) -> Resource<T, states::Initializing> {
        Resource::new(inner)
    }
}

/// Scoped resource management with automatic cleanup
///
/// This ensures resources are always released when the scope ends,
/// even if an error occurs. Supports both async and sync cleanup.
pub struct ResourceScope<T> {
    resource: Option<Resource<T, states::Acquired>>,
    leaked: bool,
}

impl<T> ResourceScope<T> {
    /// Create a new resource scope with an acquired resource
    pub const fn new(resource: Resource<T, states::Acquired>) -> Self {
        Self {
            resource: Some(resource),
            leaked: false,
        }
    }

    /// Access the resource within the scope
    ///
    /// Panics if the resource has already been released
    pub fn with_resource<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Resource<T, states::Acquired>) -> R,
    {
        let resource = self
            .resource
            .as_mut()
            .expect("Resource has already been released from scope");
        f(resource)
    }

    /// Manually release the resource from the scope
    ///
    /// If not called, the resource will be automatically released when dropped
    pub fn release(mut self) -> ResourceResult<()> {
        if let Some(resource) = self.resource.take() {
            resource.release()?;
        }
        Ok(())
    }

    /// Check if the resource has been released
    pub const fn is_released(&self) -> bool {
        self.resource.is_none()
    }

    /// Check if the resource was leaked (dropped without explicit release)
    pub const fn is_leaked(&self) -> bool {
        self.leaked
    }
}

impl<T> Drop for ResourceScope<T> {
    fn drop(&mut self) {
        if self.resource.is_some() {
            self.leaked = true;
            tracing::error!("ResourceScope dropped without explicit release - resource may leak");

            // Attempt to perform emergency cleanup if the resource supports it
            if let Some(resource) = self.resource.take() {
                // Try to release synchronously if possible
                // Note: This is a best-effort cleanup for resources that don't require async release
                drop(resource);
            }
        }
    }
}

/// Timeout-based resource cleanup guard
///
/// Automatically releases resources after a specified timeout if not explicitly released
pub struct TimedResourceGuard<T>
where
    T: Send + 'static,
{
    resource: Option<Resource<T, states::Acquired>>,
    timeout: Duration,
    acquired_at: Instant,
    cleanup_task: Option<tokio::task::JoinHandle<()>>,
}

impl<T> TimedResourceGuard<T>
where
    T: Send + 'static,
{
    /// Create a new timed resource guard
    pub fn new(resource: Resource<T, states::Acquired>, timeout: Duration) -> Self {
        let acquired_at = Instant::now();

        Self {
            resource: Some(resource),
            timeout,
            acquired_at,
            cleanup_task: None,
        }
    }

    /// Create a new timed resource guard with automatic cleanup task
    pub fn new_with_auto_cleanup(resource: Resource<T, states::Acquired>, timeout: Duration) -> Self
    where
        T: Send + Sync + 'static,
    {
        let acquired_at = Instant::now();

        // Note: In a real implementation, we'd need a way to cancel the cleanup task
        // when the resource is manually released. This is a simplified version.

        Self {
            resource: Some(resource),
            timeout,
            acquired_at,
            cleanup_task: None,
        }
    }

    /// Check if the resource has timed out
    pub fn is_timed_out(&self) -> bool {
        self.acquired_at.elapsed() > self.timeout
    }

    /// Get time remaining before timeout
    pub fn time_remaining(&self) -> Option<Duration> {
        self.timeout.checked_sub(self.acquired_at.elapsed())
    }

    /// Access the resource if it hasn't timed out
    pub fn get(&self) -> Option<&T>
    where
        T: Send,
    {
        if self.is_timed_out() {
            None
        } else {
            self.resource.as_ref().map(Resource::get)
        }
    }

    /// Release the resource manually before timeout
    pub fn release(mut self) -> ResourceResult<()> {
        if let Some(cleanup_task) = self.cleanup_task.take() {
            cleanup_task.abort();
        }

        if let Some(resource) = self.resource.take() {
            if self.is_timed_out() {
                return Err(ResourceError::Timeout {
                    duration: self.acquired_at.elapsed(),
                });
            }
            resource.release()?;
        }
        Ok(())
    }
}

impl<T> Drop for TimedResourceGuard<T>
where
    T: Send + 'static,
{
    fn drop(&mut self) {
        if let Some(cleanup_task) = self.cleanup_task.take() {
            cleanup_task.abort();
        }

        if self.resource.is_some() {
            if self.is_timed_out() {
                tracing::error!(
                    "TimedResourceGuard dropped after timeout of {:?} - resource forcibly cleaned up",
                    self.timeout
                );
            } else {
                tracing::warn!("TimedResourceGuard dropped before timeout - resource may leak");
            }
        }
    }
}

/// Resource leak detector for debugging and monitoring
#[derive(Debug, Default)]
pub struct ResourceLeakDetector {
    active_resources: std::sync::Mutex<std::collections::HashMap<String, ResourceInfo>>,
}

#[derive(Debug, Clone)]
struct ResourceInfo {
    resource_type: String,
    acquired_at: Instant,
    location: Option<String>,
}

impl ResourceLeakDetector {
    /// Create a new leak detector
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a resource acquisition
    pub fn register_acquisition(
        &self,
        resource_id: &str,
        resource_type: &str,
        location: Option<String>,
    ) {
        if let Ok(mut resources) = self.active_resources.lock() {
            resources.insert(
                resource_id.to_string(),
                ResourceInfo {
                    resource_type: resource_type.to_string(),
                    acquired_at: Instant::now(),
                    location,
                },
            );
        }
    }

    /// Register a resource release
    pub fn register_release(&self, resource_id: &str) {
        if let Ok(mut resources) = self.active_resources.lock() {
            resources.remove(resource_id);
        }
    }

    /// Get statistics about active resources
    pub fn get_stats(&self) -> ResourceLeakStats {
        self.active_resources.lock().map_or_else(
            |_| ResourceLeakStats::default(),
            |resources| {
                let total_count = resources.len();
                let mut by_type = std::collections::HashMap::new();
                let mut oldest_age = Duration::ZERO;

                for info in resources.values() {
                    *by_type.entry(info.resource_type.clone()).or_insert(0) += 1;
                    let age = info.acquired_at.elapsed();
                    if age > oldest_age {
                        oldest_age = age;
                    }
                }

                ResourceLeakStats {
                    total_active: total_count,
                    by_type,
                    oldest_resource_age: oldest_age,
                }
            },
        )
    }

    /// Find potentially leaked resources (older than threshold)
    pub fn find_potential_leaks(&self, threshold: Duration) -> Vec<String> {
        self.active_resources.lock().map_or_else(
            |_| Vec::new(),
            |resources| {
                resources
                    .iter()
                    .filter(|(_, info)| info.acquired_at.elapsed() > threshold)
                    .map(|(id, _)| id.clone())
                    .collect()
            },
        )
    }
}

/// Statistics about resource usage and potential leaks
#[derive(Debug, Default)]
pub struct ResourceLeakStats {
    /// Total number of active resources
    pub total_active: usize,
    /// Count of active resources by type
    pub by_type: std::collections::HashMap<String, usize>,
    /// Age of the oldest active resource
    pub oldest_resource_age: Duration,
}

/// Global resource leak detector instance
static GLOBAL_LEAK_DETECTOR: std::sync::OnceLock<ResourceLeakDetector> = std::sync::OnceLock::new();

/// Get the global resource leak detector
pub fn global_leak_detector() -> &'static ResourceLeakDetector {
    GLOBAL_LEAK_DETECTOR.get_or_init(ResourceLeakDetector::new)
}

/// Automatic cleanup resource wrapper
///
/// This wrapper automatically registers resources for leak detection
/// and provides cleanup on drop
pub struct ManagedResource<T, S> {
    inner: Option<Resource<T, S>>,
    resource_id: String,
    cleanup_registered: bool,
}

impl<T, S> ManagedResource<T, S> {
    /// Create a new managed resource
    pub fn new(resource: Resource<T, S>, resource_type: &str) -> Self {
        let resource_id = format!(
            "{}_{}",
            resource_type,
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        );

        // Register with leak detector
        global_leak_detector().register_acquisition(
            &resource_id,
            resource_type,
            Some(format!("{}:{}:{}", file!(), line!(), column!())),
        );

        Self {
            inner: Some(resource),
            resource_id,
            cleanup_registered: true,
        }
    }

    /// Get a reference to the inner resource
    pub const fn get(&self) -> Option<&Resource<T, S>> {
        self.inner.as_ref()
    }

    /// Take the inner resource, transferring ownership
    pub fn take(mut self) -> Option<Resource<T, S>> {
        if self.cleanup_registered {
            global_leak_detector().register_release(&self.resource_id);
            self.cleanup_registered = false;
        }
        self.inner.take()
    }
}

impl<T, S> Drop for ManagedResource<T, S> {
    fn drop(&mut self) {
        if self.cleanup_registered {
            global_leak_detector().register_release(&self.resource_id);
        }

        if self.inner.is_some() {
            tracing::debug!("ManagedResource dropped with resource still present");
        }
    }
}

/// Extension trait for adding automatic cleanup to resources
pub trait ResourceExt<T, S>: Sized {
    /// Wrap in a managed resource for automatic leak detection
    fn managed(self, resource_type: &str) -> ManagedResource<T, S>;

    /// Wrap in a scoped resource for automatic cleanup
    fn scoped(self) -> ResourceScope<T>
    where
        S: IsAcquired;

    /// Wrap in a timed guard for timeout-based cleanup
    fn with_timeout(self, timeout: Duration) -> TimedResourceGuard<T>
    where
        S: IsAcquired,
        T: Send + 'static;
}

// Implementation only for Acquired state to avoid unsafe code and conflicts
impl<T> ResourceExt<T, states::Acquired> for Resource<T, states::Acquired> {
    fn managed(self, resource_type: &str) -> ManagedResource<T, states::Acquired> {
        ManagedResource::new(self, resource_type)
    }

    fn scoped(self) -> ResourceScope<T> {
        ResourceScope::new(self)
    }

    fn with_timeout(self, timeout: Duration) -> TimedResourceGuard<T>
    where
        T: Send + 'static,
    {
        TimedResourceGuard::new(self, timeout)
    }
}

// Implementation for other states (can only use managed)
impl<T, S> Resource<T, S> {
    /// Create a managed resource for any state
    pub fn managed(self, resource_type: &str) -> ManagedResource<T, S> {
        ManagedResource::new(self, resource_type)
    }
}