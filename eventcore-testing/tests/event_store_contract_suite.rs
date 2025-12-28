//! Backend contract suite for InMemoryEventStore.
//!
//! Uses the unified `backend_contract_tests!` macro to run ALL contract tests.
//! When new tests are added to eventcore-testing, they automatically run here.

use eventcore_testing::contract::backend_contract_tests;

backend_contract_tests! {
    suite = in_memory,
    make_store = eventcore_memory::InMemoryEventStore::new,
    make_checkpoint_store = eventcore_memory::InMemoryCheckpointStore::new,
}
