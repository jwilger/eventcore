# Phantom Type Projection Protocol

## Overview

The EventCore projection system now includes a type-safe protocol implementation that uses phantom types to enforce correct phase transitions at compile time. This eliminates entire classes of runtime errors by making invalid state transitions impossible to express in code.

## Problem Statement

Traditional projection lifecycle management relies on runtime checks to ensure operations are performed in the correct order:

```rust
// Traditional approach - prone to runtime errors
let mut projection_runner = ProjectionRunner::new(projection);

// What if we forget to initialize?
projection_runner.process_events().await; // Runtime error!

// What if we try to save checkpoint while processing?
projection_runner.save_checkpoint().await; // Runtime error!

// What if we use the runner after shutdown?
projection_runner.shutdown().await;
projection_runner.process_events().await; // Runtime error!
```

## Solution: Phantom Type Protocol

The new `ProjectionProtocol<P, E, Phase>` uses phantom types to encode the current phase in the type system:

```rust
use eventcore::projection_protocol::{ProjectionProtocol, Setup, Processing, Checkpointing, Shutdown};

// Each phase has different available methods
let setup: ProjectionProtocol<MyProjection, MyEvent, Setup> = ProjectionProtocol::new(projection);
let processing: ProjectionProtocol<MyProjection, MyEvent, Processing> = /* ... */;
let checkpointing: ProjectionProtocol<MyProjection, MyEvent, Checkpointing> = /* ... */;
let shutdown: ProjectionProtocol<MyProjection, MyEvent, Shutdown> = /* ... */;
```

## Phase Transitions

### 1. Setup Phase

The initial phase where configuration and initialization occur:

```rust
// Create protocol in Setup phase
let setup = ProjectionProtocol::new(projection);

// Configure event store (builder pattern preserves phase)
let configured = setup.with_event_store(event_store);

// Load checkpoint and initialize state
let initialized = configured.load_checkpoint().await?;

// Transition to Processing phase
let processing = initialized.start_processing().await?;
```

**Available operations in Setup:**
- `with_event_store()` - Configure the event store
- `load_checkpoint()` - Load existing checkpoint or initialize
- `start_processing()` - Transition to Processing phase

### 2. Processing Phase

The active phase where events are processed:

```rust
// Process events
let events_processed = processing.process_events(batch_size).await?;

// Pause and resume
processing.pause().await?;
processing.resume().await?;

// Check status
let status = processing.get_status().await?;

// Get current state
let state = processing.get_state();

// Transition to Checkpointing
let checkpointing = processing.prepare_checkpoint();
```

**Available operations in Processing:**
- `process_events()` - Process a batch of events
- `pause()` - Pause event processing
- `resume()` - Resume event processing
- `get_status()` - Get current projection status
- `get_state()` - Get current projection state
- `prepare_checkpoint()` - Transition to Checkpointing phase

### 3. Checkpointing Phase

The phase for saving progress:

```rust
// Update checkpoint data
checkpointing.update_checkpoint(last_event_id);

// Save checkpoint
checkpointing.save_checkpoint().await?;

// Either resume processing...
let processing = checkpointing.resume_processing();

// ...or prepare for shutdown
let shutdown = checkpointing.prepare_shutdown();
```

**Available operations in Checkpointing:**
- `update_checkpoint()` - Update checkpoint data
- `save_checkpoint()` - Persist checkpoint
- `resume_processing()` - Return to Processing phase
- `prepare_shutdown()` - Transition to Shutdown phase

### 4. Shutdown Phase

The terminal phase for cleanup:

```rust
// Perform shutdown (consumes the protocol)
shutdown.shutdown().await?;

// Can retrieve final state before shutdown
let final_state = shutdown.final_state();
let final_checkpoint = shutdown.final_checkpoint();
```

**Available operations in Shutdown:**
- `shutdown()` - Perform cleanup and release resources
- `final_state()` - Get the final projection state
- `final_checkpoint()` - Get the final checkpoint

## Convenience Functions

For common patterns, helper functions are provided:

```rust
use eventcore::projection_protocol::shutdown_with_checkpoint;

// Shutdown with automatic checkpoint save
shutdown_with_checkpoint(processing_protocol).await?;
```

## Benefits

### 1. Compile-Time Safety

Invalid operations are caught at compile time:

```rust
// ❌ These won't compile:
setup.process_events(10).await;        // Error: method not found
processing.save_checkpoint().await;     // Error: method not found
setup.shutdown().await;                 // Error: method not found
```

### 2. Self-Documenting Code

The type signature tells you exactly what phase the projection is in:

```rust
fn handle_processing(protocol: ProjectionProtocol<P, E, Processing>) {
    // This function can only be called with a protocol in Processing phase
}
```

### 3. Prevents Use-After-Shutdown

The shutdown method consumes the protocol, preventing accidental reuse:

```rust
protocol.shutdown().await?;
protocol.get_state(); // Compile error: value used after move
```

### 4. Enforces Initialization

Cannot start processing without proper initialization:

```rust
let setup = ProjectionProtocol::new(projection);
// Must configure and initialize before processing
let processing = setup.start_processing().await?; // Error if not initialized
```

## Migration Guide

To use the new protocol with existing projections:

```rust
// Before: Using ProjectionRunner directly
let runner = ProjectionRunner::new(projection);
runner.start(event_store).await?;
// ... manual lifecycle management ...

// After: Using ProjectionProtocol
let protocol = ProjectionProtocol::new(projection)
    .with_event_store(event_store)
    .load_checkpoint().await?
    .start_processing().await?;
// ... type-safe phase transitions ...
```

## Implementation Details

The protocol uses zero-sized phantom types for phases:

```rust
pub struct Setup;
pub struct Processing;
pub struct Checkpointing;
pub struct Shutdown;

pub struct ProjectionProtocol<P, E, Phase> {
    projection: Arc<P>,
    // ... other fields ...
    _phantom: PhantomData<Phase>,
}
```

Phase-specific methods are implemented only for the appropriate phantom type:

```rust
impl<P, E> ProjectionProtocol<P, E, Processing> {
    pub async fn process_events(&mut self, batch_size: usize) -> ProjectionResult<usize> {
        // Only available in Processing phase
    }
}
```

## Example Usage

See `eventcore/examples/projection_protocol_example.rs` for a complete working example demonstrating all phase transitions and operations.

## Future Enhancements

The phantom type pattern can be extended to:
- Encode subscription state in the type system
- Add more granular phases (e.g., `Initializing`, `Catching Up`, `Live`)
- Support typed error states that require specific recovery actions
- Enable compile-time verification of checkpoint frequency requirements