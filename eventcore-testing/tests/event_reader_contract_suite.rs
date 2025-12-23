#![allow(unused_doc_comments)]

//! EventReader contract suite entry point for reusable backend verification.
//!
//! This integration demonstrates how to run EventReader contract tests against
//! an EventStore implementation that also implements EventReader.

use eventcore_testing::contract::{
    test_batch_limiting, test_event_ordering_across_streams, test_position_based_resumption,
    test_stream_prefix_filtering,
};

#[tokio::test(flavor = "multi_thread")]
async fn event_ordering_across_streams_contract() {
    test_event_ordering_across_streams(eventcore::InMemoryEventStore::new)
        .await
        .expect("event reader contract failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn position_based_resumption_contract() {
    test_position_based_resumption(eventcore::InMemoryEventStore::new)
        .await
        .expect("event reader contract failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn stream_prefix_filtering_contract() {
    test_stream_prefix_filtering(eventcore::InMemoryEventStore::new)
        .await
        .expect("event reader contract failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_limiting_contract() {
    test_batch_limiting(eventcore::InMemoryEventStore::new)
        .await
        .expect("event reader contract failed");
}
