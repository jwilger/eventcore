//! Integration test demonstrating the unified contract suite macro behavior.
//!
//! This test demonstrates how a SINGLE `event_store_suite!` macro invocation
//! automatically runs ALL applicable contract tests (both EventStore and EventReader)
//! without requiring implementers to call separate macros.
//!
//! **Current State**: The unified `event_store_suite!` macro is implemented and this
//! file serves as a working demonstration that a single invocation generates all
//! relevant EventStore and EventReader contract tests.
//!
//! **Behavior**: This macro:
//! 1. Generates all 5 EventStore contract tests
//! 2. Automatically detects if EventReader is implemented
//! 3. Generates all 5 EventReader contract tests when applicable
//! 4. Allows implementers to opt-out of EventReader tests if needed

use eventcore_testing::event_store_suite;

// Desired API: Single macro invocation generates ALL tests
event_store_suite! {
    suite = in_memory_store_unified_suite,
    make_store = || { eventcore::InMemoryEventStore::new() },
}

// The macro should generate 10 tests total:
// 1. basic_read_write_contract (EventStore)
// 2. concurrent_version_conflicts_contract (EventStore)
// 3. stream_isolation_contract (EventStore)
// 4. missing_stream_reads_contract (EventStore)
// 5. conflict_preserves_atomicity_contract (EventStore)
// 6. event_ordering_across_streams_contract (EventReader)
// 7. position_based_resumption_contract (EventReader)
// 8. stream_prefix_filtering_contract (EventReader)
// 9. stream_prefix_requires_prefix_match_contract (EventReader)
// 10. batch_limiting_contract (EventReader)
