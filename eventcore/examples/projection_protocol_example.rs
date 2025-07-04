//! Example demonstrating the type-safe projection protocol with phantom types.
//!
//! This example shows how the new `ProjectionProtocol` provides compile-time
//! guarantees for correct phase transitions in projection lifecycle management.

use async_trait::async_trait;
use eventcore::{
    shutdown_with_checkpoint, Event, EventId, Projection, ProjectionCheckpoint, ProjectionConfig,
    ProjectionProtocol, ProjectionResult, ProjectionStatus,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

/// Example domain events for a simple counter
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::enum_variant_names)]
enum CounterEvent {
    CounterCreated { name: String },
    CounterIncremented { amount: u32 },
    CounterDecremented { amount: u32 },
    CounterReset,
}

impl TryFrom<&Self> for CounterEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// State maintained by the counter projection
#[derive(Debug, Clone, Default)]
struct CounterSummaryState {
    counters: HashMap<String, CounterInfo>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CounterInfo {
    name: String,
    current_value: i64,
    total_increments: u32,
    total_decrements: u32,
    reset_count: u32,
}

/// Example projection that maintains counter summaries
#[derive(Debug)]
struct CounterSummaryProjection {
    config: ProjectionConfig,
    state: Arc<RwLock<CounterSummaryState>>,
    checkpoint: Arc<RwLock<ProjectionCheckpoint>>,
}

impl CounterSummaryProjection {
    fn new(name: &str) -> Self {
        Self {
            config: ProjectionConfig::new(name)
                .with_checkpoint_frequency(10)
                .with_batch_size(100),
            state: Arc::new(RwLock::new(CounterSummaryState::default())),
            checkpoint: Arc::new(RwLock::new(ProjectionCheckpoint::initial())),
        }
    }

    /// Query method to get counter info
    #[allow(dead_code)]
    async fn get_counter(&self, stream_id: &str) -> Option<CounterInfo> {
        let state = self.state.read().await;
        state.counters.get(stream_id).cloned()
    }
}

#[async_trait]
impl Projection for CounterSummaryProjection {
    type State = CounterSummaryState;
    type Event = CounterEvent;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        let state = self.state.read().await;
        Ok(state.clone())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        // In a real implementation, this would track actual status
        Ok(ProjectionStatus::Running)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        let checkpoint = self.checkpoint.read().await;
        Ok(checkpoint.clone())
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        *self.checkpoint.write().await = checkpoint;
        Ok(())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        let stream_id = event.stream_id.to_string();

        match &event.payload {
            CounterEvent::CounterCreated { name } => {
                state.counters.insert(
                    stream_id,
                    CounterInfo {
                        name: name.clone(),
                        current_value: 0,
                        total_increments: 0,
                        total_decrements: 0,
                        reset_count: 0,
                    },
                );
            }
            CounterEvent::CounterIncremented { amount } => {
                if let Some(counter) = state.counters.get_mut(&stream_id) {
                    counter.current_value += i64::from(*amount);
                    counter.total_increments += 1;
                }
            }
            CounterEvent::CounterDecremented { amount } => {
                if let Some(counter) = state.counters.get_mut(&stream_id) {
                    counter.current_value -= i64::from(*amount);
                    counter.total_decrements += 1;
                }
            }
            CounterEvent::CounterReset => {
                if let Some(counter) = state.counters.get_mut(&stream_id) {
                    counter.current_value = 0;
                    counter.reset_count += 1;
                }
            }
        }

        // Update internal state
        *self.state.write().await = state.clone();

        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(CounterSummaryState::default())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Projection Protocol Example ===\n");

    // Set up event store
    let event_store = Arc::new(InMemoryEventStore::<CounterEvent>::new());

    // In a real application, events would be written by commands
    // For this example, we'll just demonstrate the projection protocol phases

    // Create projection
    let projection = CounterSummaryProjection::new("counter-summary");

    // PHASE 1: Setup
    println!("📋 Phase 1: Setup");
    let setup_protocol = ProjectionProtocol::new(projection);
    println!("   ✓ Created protocol in Setup phase");

    // Configure with event store
    let configured_setup = setup_protocol.with_event_store(event_store.clone());
    println!("   ✓ Configured event store");

    // Load checkpoint and initialize state
    let initialized_setup = configured_setup.load_checkpoint().await?;
    println!("   ✓ Loaded checkpoint and initialized state\n");

    // PHASE 2: Processing
    println!("🚀 Phase 2: Processing");
    let mut processing_protocol = initialized_setup.start_processing().await?;
    println!("   ✓ Started processing");

    // Check status
    let status = processing_protocol.get_status().await?;
    println!("   ✓ Current status: {status:?}");

    // Process some events
    let events_processed = processing_protocol.process_events(10).await?;
    println!("   ✓ Processed {events_processed} events");

    // Demonstrate pause/resume
    println!("\n⏸️  Pausing projection...");
    processing_protocol.pause().await?;
    println!("   ✓ Projection paused");

    println!("\n▶️  Resuming projection...");
    processing_protocol.resume().await?;
    println!("   ✓ Projection resumed\n");

    // PHASE 3: Checkpointing
    println!("💾 Phase 3: Checkpointing");
    let mut checkpoint_protocol = processing_protocol.prepare_checkpoint();
    println!("   ✓ Prepared for checkpointing");

    // Update checkpoint with latest event
    checkpoint_protocol.update_checkpoint(EventId::new());

    // Save checkpoint
    checkpoint_protocol.save_checkpoint().await?;
    println!("   ✓ Checkpoint saved");

    // Resume processing after checkpoint
    let processing_resumed = checkpoint_protocol.resume_processing();
    println!("   ✓ Resumed processing after checkpoint\n");

    // PHASE 4: Shutdown
    println!("🛑 Phase 4: Shutdown");

    // Use convenience function for shutdown with checkpoint
    shutdown_with_checkpoint(processing_resumed).await?;
    println!("   ✓ Shutdown completed with final checkpoint\n");

    // Demonstrate type safety
    println!("🔒 Type Safety Demonstration:");
    println!("   The phantom type system ensures:");
    println!("   • Cannot process events before initialization");
    println!("   • Cannot save checkpoint during processing");
    println!("   • Cannot shutdown from setup phase");
    println!("   • Cannot use protocol after shutdown");
    println!("   All these constraints are enforced at compile time!");

    Ok(())
}

// The following code demonstrates what WOULD NOT compile due to phantom types:
/*
// ❌ Cannot process events in Setup phase
fn invalid_setup_processing(setup: ProjectionProtocol<impl Projection, impl Send + Sync, Setup>) {
    setup.process_events(10).await; // Compile error: method not found
}

// ❌ Cannot save checkpoint in Processing phase
fn invalid_processing_checkpoint(mut processing: ProjectionProtocol<impl Projection, impl Send + Sync, Processing>) {
    processing.save_checkpoint().await; // Compile error: method not found
}

// ❌ Cannot shutdown from Setup phase
fn invalid_setup_shutdown(setup: ProjectionProtocol<impl Projection, impl Send + Sync, Setup>) {
    setup.shutdown().await; // Compile error: method not found
}

// ❌ Cannot use protocol after shutdown (consumed)
async fn invalid_after_shutdown(shutdown: ProjectionProtocol<impl Projection, impl Send + Sync, Shutdown>) {
    shutdown.shutdown().await;
    let state = shutdown.final_state(); // Compile error: value used after move
}
*/
