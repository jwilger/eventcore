# eventcore-examples

Complete example applications demonstrating EventCore patterns and best practices.

## Examples

### 1. Banking System

A multi-account banking system with transfers and balance tracking.

**Key concepts demonstrated:**
- Multi-stream atomic transfers
- Type-safe money handling
- Account balance projections
- Concurrency control

**Run the example:**
```bash
# Start PostgreSQL
docker-compose up -d

# Run banking example
cargo run --example banking
```

**Key files:**
- `src/banking/commands.rs` - Transfer and account commands
- `src/banking/projections.rs` - Balance tracking projection
- `src/banking/types.rs` - Domain types (Money, AccountId)

### 2. E-commerce Order System

Complete order workflow with inventory management and dynamic stream discovery.

**Key concepts demonstrated:**
- Dynamic stream discovery
- Complex multi-stream coordination
- Inventory tracking
- Order state machines

**Run the example:**
```bash
# Start PostgreSQL  
docker-compose up -d

# Run e-commerce example
cargo run --example ecommerce
```

**Key files:**
- `src/ecommerce/commands/` - Order lifecycle commands
- `src/ecommerce/projections/` - Inventory and analytics
- `src/ecommerce/types.rs` - Domain modeling

## Code Walkthrough

### Type-Safe Domain Modeling

```rust
// Domain types that can't be invalid
#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Money(i64);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50),
    derive(Debug, Clone, PartialEq, Eq, Hash)
)]
pub struct AccountId(String);
```

### Multi-Stream Commands

```rust
impl Command for TransferMoney {
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.from.as_stream_id(),  // Source account
            input.to.as_stream_id(),    // Destination account
        ]
    }

    async fn handle(...) -> CommandResult<Vec<StreamWrite<...>>> {
        // Validate business rules
        if !state.has_sufficient_funds(&input.from, &input.amount) {
            return Err(CommandError::InsufficientFunds);
        }

        // Return atomic events for both accounts
        Ok(vec![
            StreamWrite::new(&read_streams, from_stream, 
                MoneyWithdrawn { amount: input.amount })?,
            StreamWrite::new(&read_streams, to_stream,
                MoneyDeposited { amount: input.amount })?,
        ])
    }
}
```

### Dynamic Stream Discovery

```rust
impl Command for CancelOrder {
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<...>>> {
        // First execution: read order to find products
        let order = state.orders.get(&input.order_id)
            .ok_or(CommandError::OrderNotFound)?;

        // Discover we need product streams
        let product_streams: Vec<StreamId> = order.items
            .keys()
            .map(|id| id.as_stream_id())
            .collect();
            
        // Request additional streams
        stream_resolver.add_streams(product_streams);
        
        // Executor will re-run with all streams available
        // On second execution, we can update inventory
    }
}
```

### Projections

```rust
#[async_trait]
impl Projection for InventoryProjection {
    type Event = EcommerceEvent;
    type Error = ProjectionError;

    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            EcommerceEvent::ProductAdded { id, initial_stock } => {
                self.inventory.insert(id.clone(), *initial_stock);
            }
            EcommerceEvent::ProductReserved { id, quantity } => {
                if let Some(stock) = self.inventory.get_mut(id) {
                    *stock = stock.saturating_sub(*quantity);
                }
            }
            EcommerceEvent::ProductReleased { id, quantity } => {
                if let Some(stock) = self.inventory.get_mut(id) {
                    *stock += quantity;
                }
            }
            _ => {} // Ignore other events
        }
        Ok(())
    }
}
```

## Testing Patterns

The examples include comprehensive test suites:

```rust
#[tokio::test]
async fn concurrent_orders_respect_inventory() {
    let store = PostgresEventStore::new(test_config()).await.unwrap();
    let executor = CommandExecutor::new(store);
    
    // Add product with limited stock
    executor.execute(AddProduct {
        id: product_id(),
        initial_stock: Quantity::new(1).unwrap(),
    }).await.unwrap();
    
    // Try to order concurrently
    let order1 = executor.execute(PlaceOrder { ... });
    let order2 = executor.execute(PlaceOrder { ... });
    
    let (result1, result2) = tokio::join!(order1, order2);
    
    // Only one should succeed
    assert!(result1.is_ok() ^ result2.is_ok());
}
```

## Architecture Decisions

### Why Multi-Stream Event Sourcing?

Traditional event sourcing often limits atomic operations to single aggregates:
```rust
// Traditional: Aggregate limits what you can do atomically
class Order {
    // Can't atomically update inventory - different aggregate!
}
```

EventCore enables multi-stream atomic operations:
```rust
// EventCore: Atomically update order AND inventory
impl Command for PlaceOrder {
    fn read_streams(&self) -> Vec<StreamId> {
        vec![order_stream, inventory_stream, customer_stream]
    }
}
```

### Type Safety Throughout

1. **Parse at the boundary** - Invalid data can't enter the system
2. **Types encode rules** - Can't create invalid money amounts
3. **Compiler verification** - Can't write to undeclared streams

## Running Tests

```bash
# Unit tests
cargo test --lib

# Integration tests (requires PostgreSQL)
docker-compose up -d
cargo test --test '*'

# Specific example
cargo test --example banking
```

## Next Steps

1. **Explore the code** - Each example has extensive comments
2. **Run the examples** - See EventCore in action
3. **Modify examples** - Try adding new commands or projections
4. **Build your own** - Use these patterns in your domain

## Common Patterns

### Command Input Validation

```rust
impl OpenAccountInput {
    pub fn new(id: impl Into<String>, initial: i64) -> Result<Self, ValidationError> {
        Ok(Self {
            id: AccountId::new(id.into())?,
            initial_balance: Money::new(initial)?,
        })
    }
}
```

### Error Handling

```rust
match executor.execute(command).await {
    Ok(events) => println!("Wrote {} events", events.len()),
    Err(CommandError::BusinessRuleViolation(msg)) => {
        eprintln!("Business rule failed: {}", msg);
    }
    Err(CommandError::ConcurrencyConflict(_)) => {
        eprintln!("Another command modified the data");
    }
    Err(e) => eprintln!("Command failed: {}", e),
}
```

### Testing Helpers

```rust
use eventcore::testing::*;

// Generate test data
let account = AccountIdGenerator::new().next();
let amount = MoneyGenerator::new().valid_amount();

// Build test harness
let harness = CommandTestHarness::new()
    .given(initial_state())
    .when(command)
    .then_expect(expected_events());
```

## Questions?

- Check the inline documentation
- Review the test cases
- See the main [EventCore documentation](../README.md)