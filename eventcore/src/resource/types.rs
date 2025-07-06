//! Type definitions for resource lifecycle management
//!
//! This module contains phantom types and marker traits that provide
//! compile-time guarantees for resource state transitions.

/// Phantom type markers for resource states
pub mod states {
    /// Resource has been acquired and is ready for use
    pub struct Acquired;

    /// Resource has been released and cannot be used
    pub struct Released;

    /// Resource is in an intermediate state (e.g., during initialization)
    pub struct Initializing;

    /// Resource has failed and requires recovery
    pub struct Failed;
}

/// Marker trait for resources in acquired state
pub trait IsAcquired: private::Sealed {}

impl IsAcquired for states::Acquired {}

/// Marker trait for resources that can be released
pub trait IsReleasable: private::Sealed {}

impl IsReleasable for states::Acquired {}
impl IsReleasable for states::Failed {}

/// Marker trait for resources that can be recovered
pub trait IsRecoverable: private::Sealed {}

impl IsRecoverable for states::Failed {}

/// Private module to seal traits
mod private {
    pub trait Sealed {}
    
    impl Sealed for super::states::Acquired {}
    impl Sealed for super::states::Released {}
    impl Sealed for super::states::Initializing {}
    impl Sealed for super::states::Failed {}
}