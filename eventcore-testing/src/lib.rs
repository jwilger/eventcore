//! Testing utilities for EventCore backends and commands.
//!
//! This crate provides helpers for verifying that `EventStore` implementations
//! satisfy their behavioral contracts, injecting failures for chaos and retry
//! testing, and writing readable Given-When-Then command tests.
//!
//! # Modules
//!
//! - [`contract`] — Behavioral contract tests via the `backend_contract_tests!`
//!   macro. Run these against any `EventStore` implementation to verify it
//!   upholds all required guarantees (optimistic concurrency, atomicity,
//!   ordering, etc.).
//!
//! - [`chaos`] — [`ChaosEventStore`]: wraps an `EventStore` and injects
//!   probabilistic failures. Use for chaos testing to verify that command
//!   retry logic handles transient errors correctly.
//!
//! - [`deterministic`] — [`DeterministicConflictStore`]: injects predictable
//!   stream-version conflicts on the first write attempt, then succeeds on
//!   retry. Use for deterministic testing of the automatic retry path in
//!   `eventcore::execute()`.
//!
//! - [`event_collector`] — [`EventCollector`]: a `Projector` implementation
//!   that accumulates every event it sees into a `Vec`. Use in assertions to
//!   confirm which events were written to the store.
//!
//! - [`scenario`] — [`TestScenario`]: a Given-When-Then builder for command
//!   tests. Seed the store with prior events (Given), execute a command
//!   (When), and assert on the resulting events or error (Then).

pub mod chaos;
pub mod contract;
pub mod deterministic;
pub mod event_collector;
pub mod projection_contract;
pub mod scenario;

pub use chaos::*;
pub use contract::*;
pub use deterministic::*;
pub use event_collector::*;
pub use projection_contract::*;
pub use scenario::*;

#[cfg(test)]
#[path = "projection_contract.test.rs"]
mod projection_contract_tests;
