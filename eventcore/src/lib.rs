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
/// Event store abstraction for backend-independent storage
pub mod event_store;
/// Command execution engine with retry logic and concurrency control
pub mod executor;
pub mod metadata;
pub mod types;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
