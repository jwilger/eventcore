//! `EventCore` - Multi-stream aggregateless event sourcing library
//!
//! This library implements the aggregate-per-command pattern, eliminating
//! traditional aggregate boundaries in favor of self-contained commands
//! that can read from and write to multiple streams atomically.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Core module documentation placeholder
pub mod core {
    // TODO: Implementation will be added in Phase 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
