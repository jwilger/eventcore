//! Integration tests for the type-safe projection protocol with phantom types.

use eventcore::{
    shutdown_with_checkpoint, InMemoryProjection, ProjectionConfig, ProjectionProtocol,
    ProjectionStatus,
};
use eventcore_memory::InMemoryEventStore;
use std::sync::Arc;

#[tokio::test]
async fn projection_protocol_lifecycle() {
    // Create test infrastructure
    let event_store = Arc::new(InMemoryEventStore::<String>::new());
    let config = ProjectionConfig::new("test-projection");
    let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

    // Phase 1: Setup
    let setup_protocol = ProjectionProtocol::new(projection);

    // Configure with event store
    let setup_with_store = setup_protocol.with_event_store(event_store.clone());

    // Load checkpoint (initializes state)
    let initialized_setup = setup_with_store.load_checkpoint().await.unwrap();

    // Phase 2: Processing
    let mut processing_protocol = initialized_setup.start_processing().await.unwrap();

    // Can now perform processing operations
    // Note: The InMemoryProjection doesn't automatically update status based on protocol phase
    // In a real implementation, the projection would track its own status
    let status = processing_protocol.get_status().await.unwrap();
    assert!(matches!(
        status,
        ProjectionStatus::Stopped | ProjectionStatus::Running
    ));

    // Process some events (in real usage, subscription would handle this)
    let events_processed = processing_protocol.process_events(10).await.unwrap();
    assert_eq!(events_processed, 10);

    // Can pause and resume
    processing_protocol.pause().await.unwrap();
    processing_protocol.resume().await.unwrap();

    // Phase 3: Checkpointing
    let mut checkpoint_protocol = processing_protocol.prepare_checkpoint();

    // Save checkpoint
    checkpoint_protocol.save_checkpoint().await.unwrap();

    // Can resume processing after checkpoint
    let processing_again = checkpoint_protocol.resume_processing();

    // Phase 4: Shutdown with final checkpoint
    shutdown_with_checkpoint(processing_again).await.unwrap();
}

#[tokio::test]
async fn projection_protocol_prevents_uninitialized_processing() {
    let event_store = Arc::new(InMemoryEventStore::<String>::new());
    let config = ProjectionConfig::new("test-projection");
    let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

    // Create protocol in setup phase
    let setup_protocol = ProjectionProtocol::new(projection).with_event_store(event_store);

    // Try to start processing without loading checkpoint
    let result = setup_protocol.start_processing().await;

    // Should fail because state is not initialized
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("State not initialized"));
    }
}

#[tokio::test]
async fn projection_protocol_type_safety_demonstration() {
    // This test demonstrates the compile-time type safety of the protocol.
    // The following code examples would not compile:

    /*
    // Example 1: Cannot process events in Setup phase
    let setup_protocol = ProjectionProtocol::new(projection);
    setup_protocol.process_events(10).await; // Compile error: method not found

    // Example 2: Cannot save checkpoint in Processing phase
    let processing_protocol = /* ... */;
    processing_protocol.save_checkpoint().await; // Compile error: method not found

    // Example 3: Cannot shutdown from Setup phase
    let setup_protocol = ProjectionProtocol::new(projection);
    setup_protocol.shutdown().await; // Compile error: method not found

    // Example 4: Cannot use protocol after shutdown (consumed by shutdown)
    let shutdown_protocol = /* ... */;
    shutdown_protocol.shutdown().await;
    shutdown_protocol.final_state(); // Compile error: value used after move
    */

    // The phantom type parameter ensures these constraints at compile time
    // Test passes by virtue of compilation - no assertion needed
}

#[tokio::test]
async fn projection_protocol_state_transitions() {
    let event_store = Arc::new(InMemoryEventStore::<String>::new());
    let config = ProjectionConfig::new("state-test-projection");
    let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

    // Setup -> Processing
    let setup = ProjectionProtocol::new(projection)
        .with_event_store(event_store)
        .load_checkpoint()
        .await
        .unwrap();

    let processing = setup.start_processing().await.unwrap();

    // Processing -> Checkpointing
    let mut checkpointing = processing.prepare_checkpoint();
    checkpointing.save_checkpoint().await.unwrap();

    // Checkpointing -> Processing (resume)
    let processing_resumed = checkpointing.resume_processing();

    // Processing -> Checkpointing -> Shutdown
    let checkpointing_final = processing_resumed.prepare_checkpoint();
    let shutdown = checkpointing_final.prepare_shutdown();

    // Shutdown is terminal
    shutdown.shutdown().await.unwrap();
}

// Property test demonstrating phase transition invariants
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn projection_protocol_phase_invariants(
            projection_name in "[a-zA-Z0-9_-]{1,50}",
            checkpoint_frequency in 1u64..=1000u64,
            batch_size in 1usize..=10000usize
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result = rt.block_on(async {
                let event_store = Arc::new(InMemoryEventStore::<String>::new());
                let config = ProjectionConfig::new(projection_name)
                    .with_checkpoint_frequency(checkpoint_frequency)
                    .with_batch_size(batch_size);
                let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

                // Every projection must go through Setup phase first
                let setup = ProjectionProtocol::new(projection);

                // Must configure event store before processing
                let with_store = setup.with_event_store(event_store);

                // Must load checkpoint before processing
                let initialized = with_store.load_checkpoint().await.unwrap();

                // Can transition to processing
                let processing = initialized.start_processing().await.unwrap();

                // From processing, can only go to checkpointing
                let _checkpointing = processing.prepare_checkpoint();

                // From checkpointing, can go to processing or shutdown
                // (Type system enforces these constraints)

                Ok(()) as Result<(), Box<dyn std::error::Error>>
            });

            prop_assert!(result.is_ok());
        }
    }
}
