# Chapter 1.4: Architecture Overview

This chapter provides a high-level view of EventCore's architecture, showing how commands, events, and projections work together to create robust event-sourced systems.

## Core Architecture

EventCore follows a clean, layered architecture:

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Application   │     │   Application   │     │   Application   │
│     (Axum)      │     │    (CLI)        │     │   (gRPC)        │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         └───────────────────────┴───────────────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │   execute() + Retry     │
                    │  (Validation & Retry)   │
                    └────────────┬────────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │                       │                       │
┌────────▼────────┐   ┌──────────▼──────────┐  ┌────────▼────────┐
│    Commands     │   │   Event Store       │  │  Projections    │
│  (Domain Logic) │   │(PG / SQLite / Mem)  │  │  (Read Models)  │
└─────────────────┘   └─────────────────────┘  └─────────────────┘
```

## Key Components

### 1. Commands

Commands encapsulate business operations. They declare what streams they need and contain the business logic:

```rust
#[derive(Command)]
struct ApproveOrder {
    #[stream]
    order: StreamId,
    #[stream]
    approver: StreamId,
    #[stream]
    inventory: StreamId,
}
```

**Responsibilities:**

- Declare stream dependencies via `#[stream]` attributes
- Implement business validation rules
- Generate events representing what happened
- Ensure consistency within their boundaries

### 2. Command Execution

The `execute()` free function orchestrates command execution with automatic retry logic:

```rust
use eventcore::{execute, RetryPolicy};

let result = execute(&store, command, RetryPolicy::new()).await?;
```

**Execution Flow:**

1. **Read Phase**: Fetch all declared streams
2. **Reconstruct State**: Apply events to build current state via `CommandLogic::apply()`
3. **Execute Command**: Run business logic via `CommandLogic::handle()`
4. **Write Phase**: Atomically write new events
5. **Retry on Conflict**: Handle optimistic concurrency

### 3. Event Store

The event store provides durable, ordered storage of events:

```rust
// The EventStore trait is defined in eventcore-types.
// Implementations exist for PostgreSQL, SQLite, and in-memory stores.
// See eventcore_memory::InMemoryEventStore for the simplest example.
```

**Guarantees:**

- Atomic multi-stream writes
- Optimistic concurrency control
- Global ordering via UUIDv7 event IDs
- Exactly-once semantics

### 4. Projections

Projections build read models from events. They implement the `Projector` trait
and are run via the `run_projection()` free function:

```rust
impl Projector for OrderSummaryProjection {
    type Event = OrderEvent;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        match &event {
            OrderEvent::Approved { .. } => {
                self.approved_count += 1;
            }
            // Handle other events
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "order-summary"
    }
}

// Run the projection against the store:
run_projection(projection, &store).await?;
```

**Capabilities:**

- Real-time updates from event streams
- Rebuild from any point in time
- Multiple projections from same events
- Optimized for specific queries

## Data Flow

### Write Path (Commands)

```
User Action
    ↓
HTTP Request
    ↓
Command Creation ──────→ #[derive(Command)] macro generates boilerplate
    ↓
execute(&store, cmd, policy)
    ↓
Read Streams ──────────→ PostgreSQL: SELECT events WHERE stream_id IN (...)
    ↓
Reconstruct State ─────→ Fold events via CommandLogic::apply()
    ↓
CommandLogic::handle() ─→ Business logic validates and generates events
    ↓
Write Events ──────────→ PostgreSQL: INSERT events (atomic transaction)
    ↓
Return Result
```

### Read Path (Projections)

```
Events Written
    ↓
Event Notification
    ↓
run_projection() ──────→ Polls event streams
    ↓
Load Event
    ↓
Projector.apply() ��────→ Update read model state
    ↓
Save Checkpoint ───────→ Track position for resume
    ↓
Query Read Model ──────→ Optimized for specific access patterns
```

## Multi-Stream Atomicity

EventCore's key innovation is atomic operations across multiple streams:

### Traditional Event Sourcing

```
Account A         Account B
    │                 │
    ├─ Withdraw?      │        ❌ Two separate operations
    │                 ├─ Deposit?   (not atomic!)
    ↓                 ↓
```

### EventCore Approach

```
        TransferMoney Command
               │
    ┌──────────┴──────────┐
    ↓                     ↓
Account A              Account B
    │                     │
    ├─ Withdrawn ←────────┤ Deposited    ✅ One atomic operation!
    ↓                     ↓
```

## Concurrency Model

EventCore uses optimistic concurrency control:

1. **Version Tracking**: Each stream has a version number
2. **Read Version**: Commands note the version when reading
3. **Conflict Detection**: Writes fail if version changed
4. **Automatic Retry**: Executor retries with fresh data

```rust
// Automatic retry on conflicts via RetryPolicy
let result = execute(&store, command, RetryPolicy::new()).await?;
// Retries are handled internally based on the RetryPolicy
```

## Type Safety

EventCore leverages Rust's type system for correctness:

### Stream Access Control

```rust
// Stream enforcement via #[derive(Command)] and the Event trait
impl CommandLogic for TransferMoney {
    type Event = BankEvent;
    type State = TransferState;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.apply(event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        // Events carry their stream_id via the Event trait.
        // The executor validates that each event's stream_id() matches
        // a declared #[stream] field.
        let events = vec![BankEvent::TransferInitiated {
            from: self.from_account.clone(),
            to: self.to_account.clone(),
            amount: self.amount,
        }];

        Ok(events.into())
    }
}
```

### Validated Types

```rust
// Parse, don't validate
#[nutype(validate(greater = 0))]
struct Money(u64);

// Once created, always valid
let amount = Money::try_new(100)?;  // Validated at boundary
transfer_money(amount);              // No validation needed
```

## Deployment Architecture

### Simple Deployment

```
┌─────────────┐     ┌──────────────┐
│  Your App   │────▶│  PostgreSQL  │
└─────────────┘     └──────────────┘
```

### Production Deployment

```
                    Load Balancer
                         │
        ┌────────────────┼────────────────┐
        ↓                ↓                ↓
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│   App Pod 1   │ │   App Pod 2   │ │   App Pod 3   │
└───────┬───────┘ └───────┬───────┘ └───────┬───────┘
        │                 │                 │
        └─────────────────┼─────────────────┘
                          ↓
                ┌─────────────────┐
                │   PostgreSQL    │
                │   (Primary)     │
                └────────┬────────┘
                         │
        ┌────────────────┼────────────────┐
        ↓                                  ↓
┌───────────────┐                 ┌───────────────┐
│  PG Replica 1 │                 │  PG Replica 2 │
└───────────────┘                 └───────────────┘
```

## Performance Characteristics

EventCore is optimized for correctness and developer productivity:

### Throughput

- **Single-stream commands**: ~83 ops/sec (PostgreSQL), 187,711 ops/sec (in-memory)
- **Multi-stream commands**: ~25-50 ops/sec (PostgreSQL)
- **Batch operations**: 750,000-820,000 events/sec (in-memory)

### Latency

- **Command execution**: 10-20ms (typical)
- **Conflict retry**: +5-10ms per retry
- **Projection lag**: <100ms (typical)

### Scaling Strategies

1. **Vertical**: Larger PostgreSQL instance
2. **Read Scaling**: PostgreSQL read replicas
3. **Stream Sharding**: Partition by stream ID
4. **Caching**: Read model caching layer

## Error Handling

EventCore provides structured error handling:

```rust
// CommandError variants (simplified):
// - BusinessRuleViolation(String) — Business rule violations
// - VersionConflict — Optimistic concurrency conflicts (retried automatically)
// - StoreError — Infrastructure errors from the EventStore
```

Errors are categorized for appropriate handling:

- **Retriable**: Version conflicts (handled automatically by RetryPolicy)
- **Non-retriable**: Business rule violations
- **Fatal**: Infrastructure/store errors

## Monitoring and Observability

Built-in instrumentation for production visibility:

```rust
// Automatic metrics
eventcore.commands.executed{command="TransferMoney", status="success"}
eventcore.events.written{stream="account-123"}
eventcore.retries{reason="concurrency_conflict"}

// Structured logging
{"level":"info", "command":"TransferMoney", "duration_ms":15, "events_written":2}

// OpenTelemetry traces
TransferMoney
  ├─ stream_declarations (5ms)
  ├─ reconstruct_state (2ms)
  ├─ handle_command (3ms)
  └─ write_events (5ms)
```

## Summary

EventCore's architecture provides:

1. **Clean Separation**: Commands, events, and projections have clear responsibilities
2. **Multi-Stream Atomicity**: Complex operations remain consistent
3. **Type Safety**: Rust's type system prevents errors
4. **Production Ready**: Built-in retry, monitoring, and error handling
5. **Flexible Deployment**: From simple to highly-scaled architectures

The architecture is designed to make the right thing easy and the wrong thing impossible.

Ready to build something? Continue to [Part 2: Getting Started](../02-getting-started/README.md) →
