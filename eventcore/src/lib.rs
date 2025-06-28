//! `EventCore` - Multi-stream aggregateless event sourcing library
//!
//! This library implements the aggregate-per-command pattern, eliminating
//! traditional aggregate boundaries in favor of self-contained commands
//! that can read from and write to multiple streams atomically.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Command system for aggregate-per-command pattern event sourcing
pub mod command;
pub mod errors;
/// Event types for the event sourcing system
pub mod event;
/// Event store abstraction for backend-independent storage
pub mod event_store;
/// Event store adapter infrastructure for backend configuration
pub mod event_store_adapter;
/// Command execution engine with retry logic and concurrency control
pub mod executor;
pub mod metadata;
/// Projection system for building read models from event streams
pub mod projection;
/// Projection management system for handling projection lifecycles
pub mod projection_manager;
/// State reconstruction from event streams
pub mod state_reconstruction;
/// Event subscription system for processing events from event streams
pub mod subscription;
pub mod types;

/// Testing utilities for event sourcing applications
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
