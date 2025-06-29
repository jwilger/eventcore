# eventcore

Core library for EventCore - the multi-stream event sourcing framework.

## Installation

```toml
[dependencies]
eventcore = "0.1"
```

You'll also need an event store adapter:

```toml
eventcore-postgres = "0.1"  # For production
# or
eventcore-memory = "0.1"    # For testing
```

## Core Concepts

### Commands

Commands are the heart of EventCore. Each command:
- Declares which streams it reads
- Rebuilds state from events
- Executes business logic
- Returns events to append

```rust
use eventcore::{Command, CommandResult, StreamId, StreamWrite};

struct TransferMoney {
    from: AccountId,
    to: AccountId,
    amount: Money,
}

#[async_trait]
impl Command for TransferMoney {
    type Input = TransferMoney;
    type State = TransferState;
    type Event = BankingEvent;
    type StreamSet = BankingStreams;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            input.from.as_stream_id(),
            input.to.as_stream_id(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::Deposited { account, amount } => {
                state.balances.insert(account.clone(), 
                    state.balances.get(account).unwrap_or(&Money::zero()) + amount);
            }
            BankingEvent::Withdrawn { account, amount } => {
                state.balances.insert(account.clone(),
                    state.balances.get(account).unwrap_or(&Money::zero()) - amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check business rules
        let from_balance = state.balances.get(&input.from)
            .ok_or(CommandError::BusinessRuleViolation("Account not found".into()))?;
        
        if from_balance < &input.amount {
            return Err(CommandError::BusinessRuleViolation("Insufficient funds".into()));
        }

        // Return events
        Ok(vec![
            StreamWrite::new(&read_streams, input.from.as_stream_id(), 
                BankingEvent::Withdrawn { account: input.from, amount: input.amount })?,
            StreamWrite::new(&read_streams, input.to.as_stream_id(),
                BankingEvent::Deposited { account: input.to, amount: input.amount })?,
        ])
    }
}
```

### Type-Safe Domain Modeling

Use `nutype` for domain types that validate at construction:

```rust
use nutype::nutype;

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Deref)
)]
pub struct AccountId(String);

#[nutype(
    validate(greater_or_equal = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)
)]
pub struct Money(i64);
```

### Dynamic Stream Discovery

Commands can discover additional streams during execution:

```rust
async fn handle(
    &self,
    read_streams: ReadStreams<Self::StreamSet>,
    state: Self::State,
    input: Self::Input,
    stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Discover we need approval
    if input.amount > Money::new(10000)? {
        let approval_stream = StreamId::new(format!("approval-{}", input.from))?;
        stream_resolver.add_streams(vec![approval_stream]);
        // Executor will re-run with expanded streams
    }
    
    // Continue with logic...
}
```

### Event Store Usage

```rust
use eventcore::{CommandExecutor, EventStore};
use eventcore_postgres::PostgresEventStore;

// Initialize store
let store = PostgresEventStore::new(config).await?;
store.initialize().await?; // Create schema

// Execute commands
let executor = CommandExecutor::new(store);
let events = executor.execute(command).await?;
```

## API Reference

### Core Traits

- `Command` - Business logic implementation
- `EventStore` - Event persistence abstraction
- `Projection` - Read model builders

### Core Types

- `StreamId` - Validated stream identifier  
- `EventId` - UUIDv7 for chronological ordering
- `EventVersion` - Optimistic concurrency control
- `CommandError` - Typed error handling

### Utilities

- `CommandExecutor` - Handles execution flow
- `StreamResolver` - Dynamic stream discovery
- `ReadStreams<T>` - Type-safe stream access
- `StreamWrite<T,E>` - Type-safe event writing

## Testing

EventCore provides comprehensive testing utilities:

```rust
use eventcore::testing::*;

#[test]
async fn test_transfer() {
    let harness = CommandTestHarness::new()
        .given_events(vec![
            AccountOpened { account: alice, initial: Money::new(1000) },
        ])
        .when(TransferMoney { from: alice, to: bob, amount: Money::new(100) })
        .then_expect_events(vec![
            Withdrawn { account: alice, amount: Money::new(100) },
            Deposited { account: bob, amount: Money::new(100) },
        ]);
        
    harness.run().await.unwrap();
}
```

## Performance

EventCore is designed for high throughput:

- Zero-copy event processing where possible
- Efficient stream merging algorithms
- Connection pooling for database adapters
- Automatic retry with exponential backoff

## See Also

- [PostgreSQL Adapter](../eventcore-postgres/) - Production event store
- [Memory Adapter](../eventcore-memory/) - Testing event store
- [Examples](../eventcore-examples/) - Complete applications