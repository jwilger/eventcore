# Chapter 1.1: What is EventCore?

EventCore is a Rust library that implements **multi-stream event sourcing** - a powerful pattern that eliminates the traditional constraints of aggregate boundaries while maintaining strong consistency guarantees.

## The Problem with Traditional Event Sourcing

Traditional event sourcing forces you to define rigid aggregate boundaries upfront:

```rust
// Traditional approach - forced aggregate boundaries
struct BankAccount {
    id: AccountId,
    balance: Money,
    // Can only modify THIS account
}

// Problem: How do you transfer money atomically?
// Option 1: Two separate commands (not atomic!)
// Option 2: Process managers/sagas (complex!)
// Option 3: Eventual consistency (risky!)
```

These boundaries often don't match real business requirements:

- **Money transfers** need to modify two accounts atomically
- **Order fulfillment** needs to update inventory, orders, and shipping together
- **User registration** might need to create accounts, profiles, and notifications

## The EventCore Solution

EventCore introduces **dynamic consistency boundaries** - each command defines which streams it needs:

```rust
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,  // Read and write this stream
    #[stream]  
    to_account: StreamId,    // Read and write this stream too
    amount: Money,
}

// This command atomically:
// 1. Reads both account streams
// 2. Validates the business rules
// 3. Writes events to both streams
// 4. All in ONE atomic transaction!
```

## Key Concepts

### 1. **Event Streams**
Instead of aggregates, EventCore uses streams - ordered sequences of events identified by a StreamId:

```rust
// Streams are just identifiers
let alice_account = StreamId::from_static("account-alice");
let bob_account = StreamId::from_static("account-bob");
let order_123 = StreamId::from_static("order-123");
```

### 2. **Multi-Stream Commands**
Commands can read from and write to multiple streams atomically:

```rust
// A command that involves multiple business entities
#[derive(Command, Clone)]
struct FulfillOrder {
    #[stream]
    order_id: StreamId,       // The order to fulfill
    #[stream]
    inventory_id: StreamId,   // The inventory to deduct from
    #[stream]
    shipping_id: StreamId,    // Create shipping record
}
```

### 3. **Type-Safe Stream Access**
The macro system ensures you can only write to streams you declared:

```rust
// In your handle method:
let events = vec![
    StreamWrite::new(
        &read_streams,
        self.order_id.clone(),      // ✅ OK - declared with #[stream]
        OrderEvent::Fulfilled
    )?,
    StreamWrite::new(
        &read_streams,
        some_other_stream,           // ❌ Compile error! Not declared
        SomeEvent::Happened
    )?,
];
```

### 4. **Optimistic Concurrency Control**
EventCore tracks stream versions to detect conflicts:

1. Command reads streams at specific versions
2. Command produces new events
3. Write only succeeds if streams haven't changed
4. Automatic retry on conflicts

## Benefits

1. **Simplified Architecture**
   - No aggregate boundaries to design upfront
   - No process managers for cross-aggregate operations
   - No eventual consistency complexity

2. **Strong Consistency**
   - All changes are atomic
   - No partial failures between streams
   - Transactions that match business requirements

3. **Type Safety**
   - Commands declare their streams at compile time
   - Illegal operations won't compile
   - Self-documenting code

4. **Performance**
   - ~100 operations/second with PostgreSQL
   - Optimized for correctness over raw throughput
   - Batched operations for better performance

## How It Works

1. **Command Declaration**: Use `#[derive(Command)]` to declare which streams you need
2. **State Reconstruction**: EventCore reads all requested streams and builds current state
3. **Business Logic**: Your command validates rules and produces events
4. **Atomic Write**: All events are written in a single transaction
5. **Optimistic Retry**: On conflicts, EventCore retries automatically

## Example: Complete Money Transfer

```rust
use eventcore::prelude::*;
use eventcore_macros::Command;

#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}

#[async_trait]
impl CommandLogic for TransferMoney {
    type State = AccountBalances;
    type Event = BankingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Update state based on events
        match &event.payload {
            BankingEvent::MoneyWithdrawn { amount, .. } => {
                state.debit(&event.stream_id, *amount);
            }
            BankingEvent::MoneyDeposited { amount, .. } => {
                state.credit(&event.stream_id, *amount);
            }
            _ => {}
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check balance
        let from_balance = state.balance(&self.from_account);
        require!(
            from_balance >= self.amount.value(),
            "Insufficient funds: balance={}, requested={}",
            from_balance,
            self.amount
        );

        // Create atomic events for both accounts
        Ok(vec![
            StreamWrite::new(
                &read_streams,
                self.from_account.clone(),
                BankingEvent::MoneyWithdrawn {
                    amount: self.amount.value(),
                    to: self.to_account.to_string(),
                }
            )?,
            StreamWrite::new(
                &read_streams,
                self.to_account.clone(),
                BankingEvent::MoneyDeposited {
                    amount: self.amount.value(),
                    from: self.from_account.to_string(),
                }
            )?,
        ])
    }
}
```

## Next Steps

Now that you understand what EventCore is, let's explore [when to use it](./02-when-to-use-eventcore.md) →