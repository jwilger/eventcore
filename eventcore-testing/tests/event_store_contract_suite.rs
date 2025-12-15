#![allow(unused_doc_comments)]
#![allow(unused_imports)]

//! EventStore contract suite entry point for reusable backend verification.
//!
//! This integration demonstrates how to invoke the `event_store_contract_tests!`
//! macro so any EventStore implementation can plug into the shared behavioral specification.
//!
//! When new contract tests are added to the suite, all invocations of this macro
//! automatically include them - no manual updates required.

use eventcore_testing::contract::event_store_contract_tests;

/// Runs the complete EventStore contract suite against the in-memory store implementation.
/// This includes all EventStore and EventSubscription contract tests.
event_store_contract_tests! {
    suite = in_memory,
    make_store = eventcore::InMemoryEventStore::new,
}
