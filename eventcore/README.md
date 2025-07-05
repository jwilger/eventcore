# eventcore

Core library for EventCore - the multi-stream event sourcing framework.

## Installation

```toml
[dependencies]
eventcore = "0.1"
eventcore-macros = "0.1"  # For #[derive(Command)] and helper macros
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
- Declares which streams it reads (automatically with `#[derive(Command)]`)
- Rebuilds state from events
- Executes business logic
- Returns events to append

#### The Modern Way: Using Derive Macros

```rust
use eventcore::{prelude::*, require, emit, CommandLogic};
use eventcore_macros::Command;

#[derive(Command)]
struct TransferMoney {
    #[stream]  // Automatically included in read_streams()
    from_account: StreamId,
    #[stream]  // Automatically included in read_streams()
    to_account: StreamId,
    amount: Money,
}

#[async_trait]
impl CommandLogic for TransferMoney {
    type State = TransferState;
    type Event = BankingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::Deposited { account, amount } => {
                state.credit(account, *amount);
            }
            BankingEvent::Withdrawn { account, amount } => {
                state.debit(account, *amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        require!(state.has_account(&self.from_account), "Source account not found");
        require!(state.has_account(&self.to_account), "Target account not found");
        require!(state.balance(&self.from_account) >= self.amount, "Insufficient funds");

        let mut events = vec![];
        
        emit!(events, &read_streams, self.from_account, 
            BankingEvent::Withdrawn { 
                account: self.from_account.to_string(), 
                amount: self.amount 
            });
            
        emit!(events, &read_streams, self.to_account,
            BankingEvent::Deposited { 
                account: self.to_account.to_string(), 
                amount: self.amount 
            });

        Ok(events)
    }
}
```

#### The Classic Way: Manual Implementation

If you prefer explicit control, you can implement everything manually:

```rust
#[derive(Clone)]
struct TransferMoney {
    from: StreamId,
    to: StreamId,
    amount: Money,
}

// Manual implementation of CommandStreams
impl CommandStreams for TransferMoney {
    type StreamSet = (); // Your own phantom type
    
    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.from.clone(), self.to.clone()]
    }
}

// Implementation of CommandLogic
#[async_trait]
impl CommandLogic for TransferMoney {
    type State = TransferState;
    type Event = BankingEvent;
    
    // ... rest of implementation
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
    require!(state.is_valid(), "Invalid state");
    
    if input.amount > Money::from_cents(1000000)? {
        let approval_stream = StreamId::try_new(format!("approval-{}", input.from))?;
        stream_resolver.add_streams(vec![approval_stream]);
    }
    
    let mut events = vec![];
    emit!(events, &read_streams, input.account, AccountEvent::Updated { 
        amount: input.amount 
    });
    
    Ok(events)
}
```

### Event Store Usage

```rust
use eventcore::{CommandExecutor, EventStore};
use eventcore_macros::Command;
use eventcore_postgres::PostgresEventStore;

// Initialize store
let store = PostgresEventStore::new(config).await?;
store.initialize().await?;

#[derive(Command)]
struct OpenAccount {
    #[stream]
    account_id: StreamId,
    holder_name: String,
    initial_deposit: Money,
}

let executor = CommandExecutor::new(store);
let command = OpenAccount {
    account_id: StreamId::try_new("account-12345")?,
    holder_name: "Alice".to_string(),
    initial_deposit: Money::from_cents(50000)?,
};

let result = executor.execute(&command, command).await?;
```

## API Reference

### Core Traits

- `Command` - Business logic implementation (use `#[derive(Command)]` for less boilerplate)
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

### Helper Macros

- `require!(condition, message)` - Business rule validation
- `emit!(events, read_streams, stream, event)` - Type-safe event generation
- `#[derive(Command)]` - Generates complete `CommandStreams` implementation:
  - Automatically sets `type Input = Self`
  - Creates `type StreamSet = CommandNameStreamSet`
  - Implements `read_streams()` based on `#[stream]` fields
  - Enables implementation of just `CommandLogic` trait for 50% less boilerplate

## Testing

EventCore provides comprehensive testing utilities that work seamlessly with derived commands:

```rust
use eventcore::testing::*;
use eventcore_macros::Command;

#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream] 
    to_account: StreamId,
    amount: Money,
}

#[tokio::test]
async fn test_transfer_with_macros() {
    let alice = StreamId::try_new("account-alice").unwrap();
    let bob = StreamId::try_new("account-bob").unwrap();
    
    let harness = CommandTestHarness::new()
        .given_events(vec![
            AccountOpened { account: alice.clone(), initial: Money::from_cents(100000) },
            AccountOpened { account: bob.clone(), initial: Money::zero() },
        ])
        .when(TransferMoney { 
            from_account: alice.clone(), 
            to_account: bob.clone(), 
            amount: Money::from_cents(10000),
        })
        .then_expect_events(vec![
            Withdrawn { account: alice, amount: Money::from_cents(10000) },
            Deposited { account: bob, amount: Money::from_cents(10000) },
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