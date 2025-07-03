# Tutorial: Writing Your First Command

This tutorial will guide you through creating your first EventCore command step by step. We'll build a simple bank account system to demonstrate the core concepts.

## Prerequisites

Make sure you have the following dependencies in your `Cargo.toml`:

```toml
[dependencies]
eventcore = "0.1"
eventcore-macros = "0.1"  # For #[derive(Command)] and helper macros
eventcore-memory = "0.1"  # For testing
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
nutype = { version = "0.6.1", features = ["serde"] }  # For validated types
```

## Step 1: Define Your Domain Events

Events represent what has happened in your system. They should be immutable and contain all the information needed to understand the change.

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BankEvent {
    AccountOpened {
        owner: String,
        initial_balance: u64,
    },
    MoneyDeposited {
        amount: u64,
        description: String,
    },
    MoneyWithdrawn {
        amount: u64,
        description: String,
    },
}

// Required for the event store
impl TryFrom<&BankEvent> for BankEvent {
    type Error = std::convert::Infallible;
    
    fn try_from(value: &BankEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}
```

**Key principles for events:**
- Use past tense names (what happened, not what should happen)
- Include all relevant data (avoid references to external state)
- Never modify events once they're written

## Step 2: Create Self-Validating Domain Types

Use `nutype` to create domain types that validate at construction time, following the "parse, don't validate" principle.

```rust
use eventcore::StreamId;
use nutype::nutype;

// Define a validated Money type
#[nutype(
    validate(greater = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)
)]
pub struct Money(u64);

// Define a validated Description type
#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Serialize, Deserialize)
)]
pub struct Description(String);
```

**Key principles for domain types:**
- Validate at construction time (fail fast)
- Once constructed, the data is guaranteed valid
- Use `nutype` for automatic validation and error handling

## Step 3: Define Your Command State

State represents the current view of your data after applying all events. It's built by "folding" events.

```rust
#[derive(Default)]
pub struct AccountState {
    exists: bool,
    owner: String,
    balance: u64,
    transaction_count: u64,
}

impl AccountState {
    pub fn exists(&self) -> bool { self.exists }
    pub fn owner(&self) -> &str { &self.owner }
    pub fn balance(&self) -> u64 { self.balance }
    pub fn transaction_count(&self) -> u64 { self.transaction_count }
}
```

## Step 4: Define Your Command with Macros

Now we'll create our deposit command using the `#[derive(Command)]` macro:

```rust
use eventcore::{prelude::*, require, emit, CommandLogic};
use eventcore_macros::Command;
use async_trait::async_trait;

#[derive(Command, Clone)]
pub struct DepositMoney {
    #[stream]
    pub account_id: StreamId,
    pub amount: Money,
    pub description: Description,
}

#[async_trait]
impl CommandLogic for DepositMoney {
    type State = AccountState;
    type Event = BankEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankEvent::AccountOpened { owner, initial_balance } => {
                state.exists = true;
                state.owner = owner.clone();
                state.balance = *initial_balance;
                state.transaction_count = 1;
            }
            BankEvent::MoneyDeposited { amount, .. } => {
                state.balance += amount;
                state.transaction_count += 1;
            }
            BankEvent::MoneyWithdrawn { amount, .. } => {
                state.balance = state.balance.saturating_sub(*amount);
                state.transaction_count += 1;
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        require!(state.exists(), 
            format!("Account {} does not exist", input.account_id));

        let mut events = vec![];
        emit!(events, &read_streams, input.account_id.clone(), 
            BankEvent::MoneyDeposited {
                amount: input.amount.into_inner(),
                description: input.description.as_ref().to_string(),
            }
        );

        Ok(events)
    }
}
```

**Key benefits of the new macro approach:**
- `#[derive(Command)]` generates the complete `CommandStreams` implementation
- No need to specify `type Input = Self` or `type StreamSet = ...` manually
- `#[stream]` marks fields that should be included in the consistency boundary
- `require!` macro provides clean business rule validation
- `emit!` macro ensures type-safe event generation
- 50% less boilerplate, more focus on business logic

## Step 5: Set Up the Event Store and Executor

```rust
use eventcore::CommandExecutor;
use eventcore_memory::InMemoryEventStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_store = InMemoryEventStore::<BankEvent>::new();
    let executor = CommandExecutor::new(event_store);
    
    let command = DepositMoney {
        account_id: StreamId::try_new("account-123")?,
        amount: Money::try_new(1000)?,
        description: Description::try_new("Initial deposit")?,
    };
    
    let result = executor
        .execute(&command, command, ExecutionOptions::default())
        .await?;
    
    println!("✅ Deposit successful! {} events written", result.events_written.len());
    
    Ok(())
}
```

## Step 6: Add Tests

Always test your commands to ensure they behave correctly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_memory::InMemoryEventStore;
    
    #[tokio::test]
    async fn test_deposit_money_success() {
        let event_store = InMemoryEventStore::<BankEvent>::new();
        let executor = CommandExecutor::new(event_store);
        
        let account_id = StreamId::try_new("test-account").unwrap();
        
        let command = DepositMoney {
            account_id: account_id.clone(),
            amount: Money::try_new(500).unwrap(),
            description: Description::try_new("Test deposit").unwrap(),
        };
        
        let result = executor
            .execute(&command, command, ExecutionOptions::default())
            .await;
        
        assert!(result.is_err());
        
        if let Err(CommandError::BusinessRuleViolation(msg)) = result {
            assert!(msg.contains("does not exist"));
        } else {
            panic!("Expected BusinessRuleViolation");
        }
    }
    
    #[test]
    fn test_domain_type_validation() {
        assert!(Money::try_new(0).is_err());
        assert!(Description::try_new("  ").is_err());
        assert!(StreamId::try_new("").is_err());
        
        assert!(Money::try_new(100).is_ok());
        assert!(Description::try_new("Test").is_ok());
        assert!(StreamId::try_new("account-1").is_ok());
    }
}
```

## What You've Learned

1. **Events** represent what happened in your system
2. **Domain types** validate data at construction time using `nutype`
3. **State** is rebuilt by folding events
4. **Commands** are simplified with `#[derive(Command)]` macro
5. **Helper macros** (`require!` and `emit!`) reduce boilerplate
6. **Stream access** is type-safe and automatically managed

## Next Steps

- Try creating an `OpenAccount` command using `#[derive(Command)]`
- Add a `WithdrawMoney` command with `require!` for validation
- Explore multi-stream commands with multiple `#[stream]` fields
- Learn about the [macro DSL tutorial](macro-dsl.md) for advanced patterns
- Discover how projections work for building read models

## Common Patterns

### Domain Type Validation with `nutype`
```rust
#[nutype(
    validate(len_char_min = 3, len_char_max = 50),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Serialize, Deserialize)
)]
pub struct Username(String);

#[nutype(
    validate(greater = 0, less_or_equal = 1_000_000),
    derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)
)]
pub struct Amount(u64);
```

### Command Definition with Macros
```rust
#[derive(Command)]
struct YourCommand {
    #[stream]  // Automatically included in read_streams()
    primary_stream: StreamId,
    #[stream]  // Can have multiple streams
    secondary_stream: StreamId,
    data: YourDataType,  // Non-stream fields
}
```

### Business Rules with `require!` Macro
```rust
async fn handle(&self, ...) -> CommandResult<Vec<StreamWrite<...>>> {
    // Clean validation with require! macro
    require!(state.is_active(), "Account must be active");
    require!(state.balance >= input.amount, "Insufficient funds");
    require!(input.amount <= daily_limit, "Exceeds daily limit");
    
    // Generate events with emit! macro
    let mut events = vec![];
    emit!(events, &read_streams, stream_id, YourEvent { ... });
    Ok(events)
}
```

This tutorial covers the fundamentals of EventCore. The key insight is that commands define their own consistency boundaries and can read from and write to multiple streams atomically, making complex business operations simple and reliable.