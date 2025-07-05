# Order Fulfillment Saga Example

This example demonstrates the **saga pattern** using EventCore, showcasing how to coordinate complex distributed transactions across multiple services while maintaining consistency and providing compensation logic for failure scenarios.

## Overview

The saga pattern is essential for distributed systems where traditional ACID transactions don't scale across service boundaries. This example implements a realistic e-commerce order fulfillment workflow that coordinates:

- **Payment Processing**: Authorization and capture of customer payments
- **Inventory Management**: Stock reservation and release
- **Shipping Coordination**: Shipment creation and dispatch
- **Order Management**: Overall workflow orchestration

## Key Features Demonstrated

### 🎯 **Multi-Stream Atomicity**
- Commands read from and write to multiple event streams atomically
- Guaranteed consistency across distributed state
- Dynamic stream discovery as workflow requirements evolve

### 🔄 **Saga Orchestration**
- Central coordinator manages the entire workflow
- Type-safe state transitions prevent invalid operations
- Clear separation between happy path and compensation logic

### 🛡️ **Failure Handling**
- Comprehensive compensation actions for each workflow step
- Automatic rollback when any step fails
- Graceful degradation strategies

### 📊 **Complete Audit Trail**
- Every action is recorded as an immutable event
- Full transaction history for compliance and debugging
- Business intelligence insights from event patterns

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Saga Coordinator                              │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │              OrderFulfillmentSaga                           │ │
│  │  • Orchestrates entire workflow                            │ │
│  │  • Handles state transitions                               │ │
│  │  • Triggers compensation on failure                        │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                                │
         ┌──────────────────────┼──────────────────────┐
         │                      │                      │
 ┌───────▼────────┐   ┌─────────▼────────┐   ┌────────▼────────┐
 │    Payment     │   │    Inventory     │   │    Shipping     │
 │    Service     │   │    Service       │   │    Service      │
 │                │   │                  │   │                 │
 │ • Authorize    │   │ • Check Stock    │   │ • Create        │
 │ • Capture      │   │ • Reserve Items  │   │   Shipment      │
 │ • Refund       │   │ • Release Items  │   │ • Dispatch      │
 └────────────────┘   └──────────────────┘   └─────────────────┘
```

## Workflow Steps

### 1. Order Submission
```rust
let order_input = OrderFulfillmentInput {
    order_id: OrderId::generate(),
    customer_id: CustomerId::generate(),
    items: vec![
        OrderItem::new(product_id, quantity, unit_price),
    ],
    shipping_address: address,
    payment_method: PaymentMethod::CreditCard { last_four: "4242" },
};
```

### 2. Payment Processing
- Authorize payment for total order amount
- Capture funds if authorization succeeds
- Record payment status and transaction details

### 3. Inventory Reservation
- Check stock availability for all items
- Reserve inventory for the order
- Handle insufficient stock scenarios

### 4. Shipping Arrangement
- Create shipment with carrier
- Generate tracking number
- Schedule pickup and dispatch

### 5. Order Completion
- Mark order as fulfilled
- Notify customer of completion
- Update all relevant systems

## Compensation Logic

When any step fails, the saga automatically triggers compensation:

```rust
pub enum CompensationAction {
    RefundPayment { payment_id: PaymentId },
    ReleaseInventory { reservations: Vec<InventoryReservation> },
    CancelShipment { shipment_id: ShipmentId },
}
```

### Failure Scenarios Handled
- **Payment Declined**: No compensation needed (nothing to rollback)
- **Insufficient Inventory**: Refund authorized payments
- **Shipping Unavailable**: Refund payments and release inventory
- **System Failures**: Complete rollback of all completed steps

## Type Safety Features

### Domain Types
All business concepts use validated types that make illegal states impossible:

```rust
#[nutype(validate(greater = 0))]
pub struct Quantity(u32);  // Cannot be zero or negative

#[nutype(validate(greater_or_equal = 0))]  
pub struct Money(u64);  // Cannot be negative

pub enum SagaStatus {
    Started,
    PaymentCompleted,
    InventoryReserved,
    Completed,
    Failed { reason: String },
    Compensated,
}
```

### Command Safety
EventCore's type system ensures commands can only write to streams they've declared:

```rust
async fn handle(
    &self,
    read_streams: ReadStreams<Self::StreamSet>,
    // ...
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Can only write to streams that were read
    let event = StreamWrite::new(&read_streams, stream_id, event)?;
    Ok(vec![event])
}
```

## Running the Example

### Basic Example
```rust
use eventcore_examples::sagas::run_example;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_example().await
}
```

### Full Demo
```bash
# Run the comprehensive saga demonstration
cargo run --example sagas
```

### Individual Commands
```rust
// Execute individual saga components
let executor = Executor::new(event_store);

// Process payment
let payment_result = executor.execute(&ProcessPaymentCommand, payment_input).await?;

// Reserve inventory  
let inventory_result = executor.execute(&ReserveInventoryCommand, inventory_input).await?;

// Arrange shipping
let shipping_result = executor.execute(&ArrangeShippingCommand, shipping_input).await?;
```

## Testing

The example includes comprehensive tests covering:

### Unit Tests
- Money calculations and type safety
- Event utility functions
- Domain type validation

### Integration Tests
- Complete saga workflows
- Individual service commands
- State reconstruction from events

### Property-Based Tests
- Money arithmetic properties
- Event invariants
- State transition validations

### Failure Tests
- Compensation scenarios
- Partial failure handling
- Concurrent execution safety

### Performance Tests
- Concurrent saga execution
- Large order processing
- Stress testing scenarios

```bash
# Run all saga tests
cargo test sagas

# Run specific test categories  
cargo test sagas::unit_tests
cargo test sagas::integration_tests
cargo test sagas::property_tests
cargo test sagas::failure_tests
cargo test sagas::performance_tests
```

## Real-World Applications

This pattern is applicable to many distributed transaction scenarios:

### E-commerce
- Order fulfillment (demonstrated here)
- Return processing
- Subscription management
- Multi-vendor marketplaces

### Financial Services
- Money transfers between accounts
- Loan origination workflows
- Insurance claim processing
- Trade settlement

### Travel & Hospitality
- Trip booking (flight + hotel + car)
- Event planning coordination
- Resource reservation systems

### Supply Chain
- Multi-supplier procurement
- Manufacturing workflows
- Quality assurance processes
- Logistics coordination

## EventCore Benefits for Sagas

### Traditional Challenges
- **Distributed State**: Hard to maintain consistency across services
- **Partial Failures**: Network issues leave systems in inconsistent states  
- **Compensation Logic**: Manual rollback procedures are error-prone
- **Observability**: Difficult to track transaction state across services

### EventCore Solutions
- **Multi-stream Atomicity**: Consistent reads and writes across multiple streams
- **Event Sourcing**: Complete audit trail of all transaction steps
- **Type Safety**: Compile-time prevention of invalid state transitions
- **Stream Discovery**: Dynamic coordination as workflow requirements change

### Performance Characteristics
- **Single-stream commands**: 86 ops/sec (stable, reliable performance)
- **Multi-stream sagas**: estimated 25-50 ops/sec
- **Event store writes**: 9,000+ events/sec (batched)
- **Compensation latency**: < 100ms for most scenarios

## Advanced Features

### Dynamic Stream Discovery
Sagas can dynamically request additional streams during execution:

```rust
// After analyzing order state, request product-specific streams
let product_streams: Vec<StreamId> = order.items.keys()
    .map(|id| StreamId::try_new(format!("product-{}", id)).unwrap())
    .collect();
    
stream_resolver.add_streams(product_streams);
// Executor automatically re-reads all streams and rebuilds state
```

### Event-Driven Projections
Create read models from saga events:

```rust
impl Projection for OrderStatusProjection {
    fn apply(&mut self, event: &StoredEvent<SagaEvent>) {
        match &event.event {
            SagaEvent::SagaStarted { order_id, .. } => {
                self.orders.insert(order_id.clone(), OrderStatus::Processing);
            }
            SagaEvent::SagaCompleted { saga_id, .. } => {
                // Update order status to completed
            }
            // ... handle other events
        }
    }
}
```

### Monitoring and Observability
Built-in metrics and tracing for saga operations:

```rust
// Automatic metrics collection
- saga_duration_seconds
- saga_success_rate  
- compensation_trigger_rate
- step_failure_reasons

// Distributed tracing
- Full span hierarchy for saga execution
- Step-by-step timing and causation
- Error context and stack traces
```

## Comparison with Alternatives

| Approach | Consistency | Complexity | Observability | Performance |
|----------|-------------|------------|---------------|-------------|
| **EventCore Sagas** | ✅ Strong | 🟡 Medium | ✅ Excellent | ✅ High |
| Traditional 2PC | ✅ Strong | 🔴 High | 🔴 Poor | 🔴 Low |
| Choreography | 🟡 Eventual | 🔴 High | 🟡 Medium | ✅ High |
| Manual Coordination | 🔴 Weak | 🔴 Very High | 🔴 Poor | 🟡 Medium |

## Next Steps

1. **Customize the Example**: Adapt the order fulfillment workflow to your specific domain
2. **Add Failure Injection**: Implement failure scenarios to test compensation logic
3. **Scale Testing**: Run performance tests with realistic workloads
4. **Add Monitoring**: Integrate with your observability stack
5. **Production Hardening**: Add circuit breakers, timeouts, and retry logic

This example provides a solid foundation for implementing robust distributed transactions using EventCore's saga pattern capabilities.