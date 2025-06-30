//! Testing utilities for the `EventCore` event sourcing library.
//!
//! This module provides comprehensive testing support for event sourcing applications,
//! including property test generators, builders, assertions, and test harnesses.
//!
//! # Overview
//!
//! The testing utilities are organized into several submodules:
//!
//! - [`generators`]: Property test generators for all domain types
//! - [`builders`]: Fluent builders for creating test data
//! - [`assertions`]: Custom assertions for verifying domain invariants
//! - [`fixtures`]: Common test data and scenarios
//! - [`harness`]: Command test harness for end-to-end testing
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use eventcore::testing::prelude::*;
//! use proptest::prelude::*;
//!
//! // Use generators in property tests
//! proptest! {
//!     #[test]
//!     fn test_stream_id_property(stream_id in arb_stream_id()) {
//!         // Test properties of StreamId
//!         assert!(!stream_id.as_ref().is_empty());
//!         assert!(stream_id.as_ref().len() <= 255);
//!     }
//! }
//!
//! // Use builders for creating test data
//! #[test]
//! fn test_with_builder() {
//!     let event = EventBuilder::new()
//!         .stream_id("test-stream")
//!         .payload("test payload")
//!         .build();
//!     
//!     assert_eq!(event.stream_id.as_ref(), "test-stream");
//! }
//!
//! // Use assertions for domain invariants
//! #[test]
//! fn test_event_ordering() {
//!     let events = create_test_events(10);
//!     assert_events_ordered(&events);
//! }
//! ```

pub mod assertions;
pub mod builders;
pub mod chaos;
pub mod fixtures;
pub mod generators;
pub mod harness;

/// Prelude module for convenient imports.
///
/// Import everything needed for testing with:
/// ```rust,ignore
/// use eventcore::testing::prelude::*;
/// ```
pub mod prelude {
    pub use super::assertions::*;
    pub use super::builders::*;
    pub use super::chaos::*;
    pub use super::fixtures::*;
    pub use super::generators::*;
    pub use super::harness::*;
}
