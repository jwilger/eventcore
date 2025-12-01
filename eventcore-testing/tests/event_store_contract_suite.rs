#![allow(unused_doc_comments)]
#![allow(unused_imports)]

//! EventStore contract suite entry point for reusable backend verification.
//!
//! This integration demonstrates how to invoke the `event_store_contract_tests!` macro so
//! any EventStore implementation can plug into the shared behavioral specification.

use eventcore_testing::contract::event_store_contract_tests;

/// Runs the standard EventStore contract suite against the in-memory store implementation.
event_store_contract_tests! {
    suite = in_memory,
    make_store = eventcore::InMemoryEventStore::new,
}
