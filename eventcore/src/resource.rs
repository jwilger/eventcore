//! Resource acquisition and release with phantom type safety
//!
//! This module provides compile-time guarantees for resource lifecycle management.
//! Resources must be acquired before use and cannot be used after release.
//! The type system prevents use-after-release and double-release errors.

pub mod implementations;
pub mod lifecycle;
pub mod monitor;
#[cfg(test)]
mod tests;
pub mod types;

pub use implementations::*;
pub use lifecycle::{ManagedResource, ResourceExt, ResourceScope, TimedResourceGuard};
pub use monitor::{global_leak_detector, ResourceLeakDetector, ResourceLeakStats};
pub use types::{states, IsAcquired, IsReleasable, IsRecoverable};

use std::marker::PhantomData;

use async_trait::async_trait;
use thiserror::Error;

/// Errors that can occur during resource operations
#[derive(Debug, Error)]
pub enum ResourceError {
    /// Resource acquisition failed
    #[error("Resource acquisition failed: {0}")]
    AcquisitionFailed(String),

    /// Resource release failed
    #[error("Resource release failed: {0}")]
    ReleaseFailed(String),

    /// Resource is in invalid state for operation
    #[error("Resource is in invalid state: {0}")]
    InvalidState(String),

    /// Resource operation timed out
    #[error("Resource operation timed out after {duration:?}")]
    Timeout {
        /// The duration after which the operation timed out
        duration: Duration,
    },

    /// Resource has been poisoned due to panic
    #[error("Resource poisoned: {0}")]
    Poisoned(String),
}

/// Type alias for resource operation results
pub type ResourceResult<T> = Result<T, ResourceError>;

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
/// // Example with database pool resource (requires postgres feature)
/// // This would typically be used with eventcore-postgres crate:
/// // ```rust,ignore
/// // use eventcore::resource::database::{DatabaseResourceManager, DatabasePool};
/// // use sqlx::PgPool;
/// //
/// // let pool = Arc::new(PgPool::connect("postgres://localhost/mydb").await?);
/// // let manager = DatabaseResourceManager::new(pool);
/// // let db_resource = manager.acquire_pool().await?;
/// // let result = db_resource.execute_query("SELECT 1").await?;
/// // let _released = db_resource.release()?;
/// // ```
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
    const fn new(inner: T) -> Self {
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

/// Integration with existing EventCore types
pub mod integration {
    use super::{states, Resource};

    /// Resource wrapper for event stores
    pub type EventStoreResource<ES> = Resource<ES, states::Acquired>;

    /// Resource wrapper for subscriptions
    pub type SubscriptionResource<S> = Resource<S, states::Acquired>;

    /// Extension trait for event store resources
    pub trait EventStoreResourceExt<ES> {
        /// Create an event store resource
        fn into_resource(self) -> EventStoreResource<ES>;
    }

    impl<ES> EventStoreResourceExt<ES> for ES {
        fn into_resource(self) -> EventStoreResource<ES> {
            Resource::new(self)
        }
    }

    /// Extension trait for subscription resources
    pub trait SubscriptionResourceExt<S> {
        /// Create a subscription resource
        fn into_resource(self) -> SubscriptionResource<S>;
    }

    impl<S> SubscriptionResourceExt<S> for S {
        fn into_resource(self) -> SubscriptionResource<S> {
            Resource::new(self)
        }
    }
}
