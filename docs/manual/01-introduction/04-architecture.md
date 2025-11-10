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
                    │    Command Executor     │
                    │  (Validation & Retry)   │
                    └────────────┬────────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │                       │                       │
┌────────▼────────┐   ┌──────────▼──────────┐  ┌────────▼────────┐
│    Commands     │   │   Event Store       │  │  Projections    │
│  (Domain Logic) │   │  (PostgreSQL)       │  │  (Read Models)  │
└─────────────────┘   └─────────────────────┘  └─────────────────┘
```

## Key Components

### 1. Commands

Commands encapsulate business operations. They declare what streams they need and contain the business logic:

```rust
#[derive(Command, Clone)]
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

### 2. Command Executor

The executor orchestrates command execution with automatic retry logic:

```rust
let executor = CommandExecutor::builder()
    .with_store(event_store)
    .with_retry_policy(RetryPolicy::exponential_backoff())
    .build();

let result = executor.execute(&command).await?;
let stream_declarations = command.stream_declarations();
```

**Execution Flow:**

1. **Read Phase**: Fetch all declared streams
2. **Reconstruct State**: Apply events to build current state
3. **Execute Command**: Run business logic
4. **Write Phase**: Atomically write new events
5. **Retry on Conflict**: Handle optimistic concurrency

### 3. Event Store

The event store provides durable, ordered storage of events:

```rust
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn read_stream(&self, stream_id: &StreamId) -> Result<Vec<StoredEvent>>;
    async fn write_events(&self, events: Vec<EventToWrite>) -> Result<()>;
}
```

**Guarantees:**

- Atomic multi-stream writes
- Optimistic concurrency control
- Global ordering via UUIDv7 event IDs
- Exactly-once semantics

### 4. Projections

Projections build read models from events:

```rust
impl CqrsProjection for OrderSummaryProjection {
    type Event = OrderEvent;
    type Error = ProjectionError;

    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            OrderEvent::Approved { .. } => {
                self.approved_count += 1;
            }
            // Handle other events
        }
        Ok(())
    }
}
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
Executor.execute()
    ↓
Read Streams ──────────→ PostgreSQL: SELECT events WHERE stream_id IN (...)
    ↓
Reconstruct State ─────→ Fold events into current state
    ↓
Command.handle() ──────→ Business logic validates and generates events
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
Projection Runner ─────→ Subscribes to event streams
    ↓
Load Event
    ↓
Projection.apply() ────→ Update read model state
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
// Internally tracked by EventCore
struct StreamVersion {
    stream_id: StreamId,
    version: EventVersion,
}

// Automatic retry on conflicts
let result = executor
    .execute(&command)
    .await?;  // Retries handled internally
```

## Type Safety

EventCore leverages Rust's type system for correctness:

### Stream Access Control

```rust
// Compile-time enforcement
impl TransferMoney {
    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        let declarations = self.stream_declarations();

        // ✅ Can only emit events for declared streams
        let events = vec![BankEvent::TransferInitiated {
            from: self.from_account.clone(),
            to: self.to_account.clone(),
            amount: self.amount,
        }];

        // ❌ Infrastructure rejects events that target undeclared streams
        // BankEvent::AuditOnly { stream: other_stream } -> would fail validation

        Ok(NewEvents::from(events))
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
pub enum CommandError {
    ValidationFailed(String),      // Business rule violations
    ConcurrencyConflict,          // Version conflicts (retried)
    StreamNotFound(StreamId),     // Missing streams
    EventStoreFailed(String),     // Infrastructure errors
}
```

Errors are categorized for appropriate handling:

- **Retriable**: Concurrency conflicts, transient failures
- **Non-retriable**: Validation failures, business rule violations
- **Fatal**: Infrastructure failures, panic recovery

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
