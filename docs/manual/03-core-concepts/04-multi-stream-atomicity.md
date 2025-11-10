# Chapter 3.4: Multi-Stream Atomicity

Multi-stream atomicity is EventCore's key innovation. Traditional event sourcing forces you to choose aggregate boundaries upfront. EventCore lets each command define its own consistency boundary dynamically.

## The Problem with Traditional Aggregates

In traditional event sourcing:

```rust
// Traditional approach - rigid boundaries
struct BankAccount {
    id: AccountId,
    balance: Money,
    // Can only modify THIS account atomically
}

// ❌ Cannot atomically transfer between accounts!
// Must use sagas, process managers, or eventual consistency
```

This leads to:

- **Complex workflows** for operations spanning aggregates
- **Eventual consistency** where immediate consistency is needed
- **Race conditions** between related operations
- **Difficult refactoring** when boundaries need to change

## EventCore's Solution

EventCore allows atomic operations across multiple streams:

```rust
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,   // Read and write this stream

    #[stream]
    to_account: StreamId,     // Read and write this stream too

    amount: Money,
}

// ✅ Both accounts updated atomically or not at all!
```

## How It Works

### 1. Stream Declaration

Commands declare all streams they need:

```rust
#[derive(Command, Clone)]
struct ProcessOrder {
    #[stream]
    order: StreamId,

    #[stream]
    inventory: StreamId,

    #[stream]
    customer: StreamId,

    #[stream]
    payment: StreamId,
}
```

### 2. Atomic Read Phase

EventCore reads all declared streams with version tracking:

```rust
// EventCore does this internally:
let declarations = command.stream_declarations();
let mut stream_data = HashMap::new();

for stream_id in declarations.iter() {
    let events = event_store.read_stream(stream_id.clone()).await?;
    stream_data.insert(stream_id.clone(), StreamData {
        version: events.version,
        events: events.events,
    });
}
```

### 3. State Reconstruction

State is built from all streams:

```rust
let mut state = OrderProcessingState::default();

for (stream_id, data) in &stream_data {
    for event in &data.events {
        command.apply(&mut state, event);
    }
}
```

### 4. Command Execution

Your business logic runs with full state:

```rust
fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // Validate across all streams
    require!(state.order.is_valid(), "Invalid order");
    require!(state.inventory.has_stock(&self.items), "Insufficient stock");
    require!(state.customer.can_purchase(), "Customer not authorized");
    require!(state.payment.has_funds(self.total), "Insufficient funds");

    // Generate events for multiple streams
    Ok(NewEvents::from(vec![
        OrderEvent::Confirmed { /* ... */ },
        InventoryEvent::Reserved { /* ... */ },
        CustomerEvent::OrderPlaced { /* ... */ },
        PaymentEvent::Charged { /* ... */ },
    ]))
}
```

### 5. Atomic Write Phase

All events written atomically with version checks:

```rust
// EventCore ensures all-or-nothing write
event_store.write_events(vec![
    EventToWrite {
        stream_id: order_stream,
        payload: order_event,
        expected_version: ExpectedVersion::Exact(order_version),
    },
    EventToWrite {
        stream_id: inventory_stream,
        payload: inventory_event,
        expected_version: ExpectedVersion::Exact(inventory_version),
    },
    // ... more events
]).await?;
```

## Consistency Guarantees

### Version Checking

EventCore prevents concurrent modifications:

```rust
// Command A reads order v5, inventory v10
// Command B reads order v5, inventory v10

// Command A writes first - succeeds
// Order → v6, Inventory → v11

// Command B tries to write - FAILS
// Version conflict detected!
```

### Automatic Retry

On version conflicts, EventCore:

1. Re-reads all streams
2. Rebuilds state with new events
3. Re-executes command logic
4. Attempts write again

```rust
// This happens automatically:
loop {
    let (state, versions) = read_and_build_state().await?;
    let events = command.handle(state)?; // synchronous, pure domain logic

    match write_with_version_check(events, versions).await {
        Ok(_) => return Ok(()),
        Err(VersionConflict) => continue, // Retry
        Err(e) => return Err(e),
    }
}
```

## Dynamic Stream Discovery

Commands can request additional streams during execution, but discovery is an executor concern. `handle()` remains focused on returning domain events. Example of a `handle` that indicates additional product-related events should be emitted:

```rust
fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // If product streams are relevant, return events that reference product stream IDs via event.stream_id().
    let product_events: Vec<Self::Event> = state.order.items
        .iter()
        .map(|item| ProductEvent::StockReserved { product_id: item.product_id, quantity: item.quantity })
        .collect();

    Ok(NewEvents::from(product_events))
}
```

The executor is responsible for collecting any dynamically discovered streams (if needed), re-reading them, and re-invoking `handle()` with the reconstructed state.

## Real-World Examples

### E-Commerce Checkout

```rust
#[derive(Command, Clone)]
struct CheckoutCart {
    #[stream]
    cart: StreamId,

    #[stream]
    customer: StreamId,

    #[stream]
    payment_method: StreamId,

    // Product streams discovered dynamically
}

fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // Validate everything atomically
    for (product_id, quantity) in &state.cart.items {
        let product_state = &state.products[product_id];
        require!(
            product_state.available_stock >= *quantity,
            "Insufficient stock for product {}", product_id
        );
    }

    // Generate events for all affected streams as domain events
    let mut events = vec![
        CartEvent::CheckedOut { order_id: /* ... */ },
        OrderEvent::Created { /* ... */ },
        PaymentEvent::Charged { amount: state.cart.total },
    ];

    // Reserve inventory from each product
    for (product_id, quantity) in &state.cart.items {
        events.push(ProductEvent::StockReserved { product_id: *product_id, quantity: *quantity });
    }

    Ok(NewEvents::from(events))
}
```

### Distributed Ledger

```rust
#[derive(Command, Clone)]
struct RecordTransaction {
    #[stream]
    ledger: StreamId,

    #[stream]
    account_a: StreamId,

    #[stream]
    account_b: StreamId,

    entry: LedgerEntry,
}

fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // Ensure double-entry bookkeeping consistency
    require!(
        self.entry.debits == self.entry.credits,
        "Debits must equal credits"
    );

    // Validate account states
    require!(
        state.account_a.is_active && state.account_b.is_active,
        "Both accounts must be active"
    );

    // Record atomically in all streams (domain events)
    Ok(NewEvents::from(vec![
        LedgerEvent::EntryRecorded { entry: self.entry.clone() },
        AccountEvent::Debited { amount: self.entry.debit_amount, reference: self.entry.id },
        AccountEvent::Credited { amount: self.entry.credit_amount, reference: self.entry.id },
    ]))
}
```

### Workflow Orchestration

```rust
#[derive(Command, Clone)]
struct CompleteWorkflowStep {
    #[stream]
    workflow: StreamId,

    #[stream]
    current_step: StreamId,

    // Next step stream discovered dynamically

    step_result: StepResult,
}

fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    // Determine next step based on current state and result
    let next_step_id = match (&state.current_step.step_type, &self.step_result) {
        (StepType::Approval, StepResult::Approved) => state.workflow.next_step,
        (StepType::Approval, StepResult::Rejected) => state.workflow.rejection_step,
        (StepType::Processing, StepResult::Success) => state.workflow.next_step,
        (StepType::Processing, StepResult::Error) => state.workflow.error_step,
        _ => None,
    };

    let mut events = vec![
        WorkflowEvent::StepCompleted { step_id: state.current_step.id, result: self.step_result.clone() },
        StepEvent::Completed { result: self.step_result.clone() },
    ];

    if let Some(next_id) = next_step_id {
        events.push(StepEvent::Activated { workflow_id: state.workflow.id, activation_time: Utc::now() });
    }

    Ok(NewEvents::from(events))
}
```

## Performance Considerations

### Stream Count Impact

Reading more streams has costs:

```rust
// Benchmark results (example):
// 1 stream:    5ms   average latency
// 5 streams:   12ms  average latency
// 10 streams:  25ms  average latency
// 50 streams:  150ms average latency

// Design commands to read only necessary streams
```

### Optimization Strategies

1. **Stream Partitioning**

```rust
// Instead of one hot stream
let stream = StreamId::from_static("orders");

// Partition by customer segment
let stream = StreamId::from(format!("orders-{}",
    customer_id.hash() % 16
));
```

2. **Lazy Stream Loading**

```rust
// Request additional streams via the executor; handle() remains focused on business logic and returns events.
if state.requires_detailed_check() {
    // executor may request detail streams and re-run handle()
}

// Continue with basic validation...
```

3. **Read Filtering**

```rust
// EventCore may support filtered reads (future feature)
let options = ReadOptions::default()
    .from_version(EventVersion::new(1000))  // Skip old events
    .event_types(&["OrderPlaced", "OrderShipped"]); // Only specific types
```

## Testing Multi-Stream Commands

### Integration Tests

```rust
#[tokio::test]
async fn test_multi_stream_atomicity() {
    let store = InMemoryEventStore::<BankEvent>::new();
    let executor = CommandExecutor::new(store.clone());

    // Setup initial state
    create_account(&executor, "account-1", 1000).await;
    create_account(&executor, "account-2", 500).await;

    // Execute transfer
    let transfer = TransferMoney {
        from_account: StreamId::from_static("account-1"),
        to_account: StreamId::from_static("account-2"),
        amount: 300,
    };

    executor.execute(&transfer).await.unwrap();

    // Verify both accounts updated atomically
    let account1 = get_balance(&store, "account-1").await;
    let account2 = get_balance(&store, "account-2").await;

    assert_eq!(account1, 700);  // 1000 - 300
    assert_eq!(account2, 800);  // 500 + 300
    assert_eq!(account1 + account2, 1500); // Total preserved
}
```

### Concurrent Modification Tests

```rust
#[tokio::test]
async fn test_concurrent_transfers() {
    let store = InMemoryEventStore::<BankEvent>::new();
    let executor = CommandExecutor::new(store);

    // Setup accounts
    create_account(&executor, "A", 1000).await;
    create_account(&executor, "B", 1000).await;
    create_account(&executor, "C", 1000).await;

    // Concurrent transfers forming a cycle
    let transfer_ab = TransferMoney {
        from_account: StreamId::from_static("A"),
        to_account: StreamId::from_static("B"),
        amount: 100,
    };

    let transfer_bc = TransferMoney {
        from_account: StreamId::from_static("B"),
        to_account: StreamId::from_static("C"),
        amount: 100,
    };

    let transfer_ca = TransferMoney {
        from_account: StreamId::from_static("C"),
        to_account: StreamId::from_static("A"),
        amount: 100,
    };

    // Execute concurrently
    let (r1, r2, r3) = tokio::join!(
        executor.execute(&transfer_ab),
        executor.execute(&transfer_bc),
        executor.execute(&transfer_ca),
    );

    // All should succeed (with retries)
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert!(r3.is_ok());

    // Total balance preserved
    let total = get_balance(&store, "A").await +
                get_balance(&store, "B").await +
                get_balance(&store, "C").await;
    assert_eq!(total, 3000);
}
```

## Common Patterns

### Read-Only Streams

Some streams are read but not written:

```rust
#[derive(Command, Clone)]
struct ValidateTransaction {
    #[stream]
    transaction: StreamId,

    #[stream]
    rules_engine: StreamId,  // Read-only for validation rules

    #[stream]
    fraud_history: StreamId, // Read-only for risk assessment
}

fn handle(/* ... */) -> Result<NewEvents<Self::Event>, CommandError> {
    // Use read-only streams for validation (state contains data from those streams)
    let risk_score = calculate_risk(&state.fraud_history);
    let applicable_rules = state.rules_engine.rules_for(&self.transaction);

    // Only write to transaction stream: return the domain event(s)
    Ok(NewEvents::from(vec![TransactionEvent::Validated { risk_score }]))
}
```

### Conditional Stream Writes

Write to streams based on business logic:

```rust
fn handle(/* ... */) -> Result<NewEvents<Self::Event>, CommandError> {
    let mut events = vec![];

    // Always update the main stream
    events.push(OrderEvent::Processed { /* ... */ });

    // Conditionally update other streams
    if state.customer.is_vip {
        events.push(CustomerEvent::VipPointsEarned { points: calculate_points() });
    }

    if state.requires_fraud_check() {
        events.push(FraudEvent::CheckRequested { /* ... */ });
    }

    Ok(NewEvents::from(events))
}
```

## Summary

Multi-stream atomicity in EventCore provides:

- ✅ **Dynamic boundaries** - Each command defines its consistency needs
- ✅ **True atomicity** - All streams updated together or not at all
- ✅ **Automatic retries** - Handle concurrent modifications gracefully
- ✅ **Stream discovery** - Add streams dynamically during execution
- ✅ **Type safety** - Compile-time guarantees about stream access

Best practices:

1. Declare minimal required streams upfront
2. Use dynamic discovery for conditional streams
3. Design for retry-ability (idempotent operations)
4. Test concurrent scenarios thoroughly
5. Monitor retry rates in production

Next, let's explore [Error Handling](./05-error-handling.md) →
