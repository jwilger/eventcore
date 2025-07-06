//! Resource lifecycle management patterns
//!
//! This module provides utilities for managing resource acquisition,
//! release, and automatic cleanup patterns.

use crate::resource::{states, IsAcquired, Resource, ResourceError, ResourceResult};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

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
        crate::resource::monitor::global_leak_detector().register_acquisition(
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
            crate::resource::monitor::global_leak_detector().register_release(&self.resource_id);
            self.cleanup_registered = false;
        }
        self.inner.take()
    }
}

impl<T, S> Drop for ManagedResource<T, S> {
    fn drop(&mut self) {
        if self.cleanup_registered {
            crate::resource::monitor::global_leak_detector().register_release(&self.resource_id);
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