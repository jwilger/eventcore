# Distributed Systems with EventCore

This tutorial demonstrates how to use EventCore in a distributed microservices architecture.

## Overview

The distributed e-commerce example (`examples/distributed_ecommerce_example.rs`) showcases how EventCore's multi-stream atomicity and event sourcing capabilities work in a distributed system with multiple services.

## Architecture

The example implements a distributed e-commerce system with four services:

- **Order Service**: Manages customer orders and orchestrates workflows
- **Inventory Service**: Handles product stock and reservations
- **Payment Service**: Processes payments and refunds
- **Shipping Service**: Manages order fulfillment and delivery

## Key Patterns Demonstrated

### 1. Distributed Sagas

The order processing workflow is implemented as a distributed saga that spans multiple services:

```rust
// Step 1: Create order
executor.execute(CreateOrder { ... }).await?;

// Step 2: Reserve inventory for each item
for (product_id, quantity) in &order.items {
    executor.execute(ReserveStock { ... }).await?;
}

// Step 3: Process payment
executor.execute(ProcessPayment { ... }).await?;

// Step 4: Create shipment
executor.execute(CreateShipment { ... }).await?;
```

### 2. Event Choreography

Services communicate through events, enabling loose coupling:

- Order Service emits `OrderCreated` event
- Inventory Service reacts by reserving stock
- Payment Service processes payment after inventory is reserved
- Shipping Service creates shipment after payment succeeds

### 3. Compensating Transactions

When a step fails, the saga orchestrator triggers compensating actions:

```rust
if payment_fails {
    // Release reserved inventory
    executor.execute(ReleaseStock { ... }).await?;
    
    // Cancel the order
    executor.execute(CancelOrder { ... }).await?;
}
```

### 4. Service Boundaries

Each service maintains its own event streams:

- Order Service: `order-aggregate`, `order-{id}`
- Inventory Service: `inventory-aggregate`, `inventory-product-{id}`
- Payment Service: `payment-aggregate`
- Shipping Service: `shipping-aggregate`

### 5. Multi-Stream Atomicity

Commands can atomically update multiple streams within a service boundary:

```rust
fn read_streams(&self) -> Vec<StreamId> {
    vec![
        // Service's aggregate stream
        StreamId::from_static("inventory-aggregate"),
        // Product-specific stream
        StreamId::new(format!("inventory-product-{}", self.product_id)).unwrap(),
        // Cross-service notification stream
        StreamId::new(format!("order-{}", self.order_id)).unwrap(),
    ]
}
```

### 6. Idempotency

The example demonstrates idempotent command handling:

```rust
// Check if payment already processed for this order
if state.order_payments.contains_key(&self.order_id) {
    return Ok(vec![]); // Idempotent - already processed
}
```

## Benefits of EventCore in Distributed Systems

1. **Atomic Updates**: Each service can atomically update multiple streams, ensuring consistency within service boundaries

2. **Event History**: Complete audit trail of all actions across all services

3. **Failure Recovery**: Event sourcing enables replay and recovery from any point

4. **Eventual Consistency**: Services maintain their own consistency while coordinating through events

5. **Testing**: In-memory event store enables comprehensive testing of distributed workflows

## Running the Example

```bash
cargo run --example distributed_ecommerce_example
```

The example demonstrates:
- Successful order processing through all services
- Handling of insufficient inventory
- Idempotent command processing
- Compensating transactions for failures

## Extending the Pattern

To adapt this pattern for your distributed system:

1. **Define Service Boundaries**: Identify your microservices and their responsibilities

2. **Design Event Flow**: Map out how events flow between services

3. **Implement Sagas**: Create orchestrators for complex workflows

4. **Add Compensations**: Define rollback actions for each step

5. **Handle Failures**: Implement retry logic and circuit breakers

6. **Monitor Events**: Use EventCore's event history for debugging and monitoring

## Best Practices

1. **Keep Services Autonomous**: Each service should function independently

2. **Use Correlation IDs**: Track requests across services (e.g., order_id)

3. **Implement Timeouts**: Add timeouts for long-running operations

4. **Version Events**: Plan for event schema evolution

5. **Test Failure Scenarios**: Thoroughly test compensating transactions

## See Also

- [CQRS Integration](../cqrs-design.md) - Building read models from events
- [Schema Evolution](../schema-evolution.md) - Handling event versioning
- [Performance Characteristics](../performance-characteristics.md) - Understanding multi-stream performance