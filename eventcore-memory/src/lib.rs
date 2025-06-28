//! In-memory adapter for `EventCore` event sourcing library
//!
//! This crate provides an in-memory implementation of the `EventStore` trait
//! from the eventcore crate, useful for testing and development scenarios
//! where persistence is not required.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// In-memory event store implementation placeholder
pub struct MemoryEventStore {
    // TODO: Implementation will be added in Phase 5
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
