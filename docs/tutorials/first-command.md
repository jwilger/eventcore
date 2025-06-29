# Tutorial: Writing Your First Command

This tutorial will guide you through creating your first EventCore command step by step. We'll build a simple bank account system to demonstrate the core concepts.

## Prerequisites

Make sure you have the following dependencies in your `Cargo.toml`:

```toml
[dependencies]
eventcore = "0.1"
eventcore-memory = "0.1"  # For testing
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
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

## Step 2: Create a Self-Validating Input Type

Input types should validate themselves at construction time, following the "parse, don't validate" principle.

```rust
use eventcore::StreamId;

#[derive(Clone)]
pub struct DepositMoneyInput {
    account_id: StreamId,
    amount: u64,
    description: String,
}

impl DepositMoneyInput {
    /// Smart constructor that validates inputs
    pub fn new(account_id: &str, amount: u64, description: &str) -> Result<Self, String> {
        // Validate business rules at construction
        if amount == 0 {
            return Err("Deposit amount must be greater than zero".to_string());
        }
        
        if description.trim().is_empty() {
            return Err("Description cannot be empty".to_string());
        }
        
        let stream_id = StreamId::try_new(account_id)
            .map_err(|e| format!("Invalid account ID: {}", e))?;
        
        Ok(Self {
            account_id: stream_id,
            amount,
            description: description.trim().to_string(),
        })
    }
    
    // Getters for the command to use
    pub fn account_id(&self) -> &StreamId { &self.account_id }
    pub fn amount(&self) -> u64 { self.amount }
    pub fn description(&self) -> &str { &self.description }
}
```

**Key principles for input types:**
- Validate at construction time (fail fast)
- Once constructed, the data is guaranteed valid
- Use smart constructors instead of public fields

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

## Step 4: Implement Your Command

Now we'll implement the `Command` trait for our deposit operation:

```rust
use eventcore::prelude::*;
use eventcore::{ReadStreams, StreamWrite, StreamResolver};
use async_trait::async_trait;

pub struct DepositMoney;

#[async_trait]
impl Command for DepositMoney {
    type Input = DepositMoneyInput;
    type State = AccountState;
    type Event = BankEvent;
    type StreamSet = (); // Phantom type for compile-time stream access control

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Define which streams this command needs to read
        // This creates the consistency boundary
        vec![input.account_id().clone()]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        // Fold events into state - this rebuilds current state from history
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
        // Business logic goes here
        
        // Check if account exists
        if !state.exists() {
            return Err(CommandError::BusinessRuleViolation(
                format!("Account {} does not exist", input.account_id())
            ));
        }

        // Create the event with type-safe stream access
        let event = StreamWrite::new(
            &read_streams,
            input.account_id().clone(),
            BankEvent::MoneyDeposited {
                amount: input.amount(),
                description: input.description().to_string(),
            }
        )?;

        Ok(vec![event])
    }
}
```

**Key concepts in the Command trait:**
- `read_streams()`: Defines the consistency boundary
- `apply()`: Rebuilds state from events (pure function)
- `handle()`: Contains business logic and produces new events

## Step 5: Set Up the Event Store and Executor

```rust
use eventcore::CommandExecutor;
use eventcore_memory::InMemoryEventStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up the event store (in-memory for this example)
    let event_store = InMemoryEventStore::<BankEvent>::new();
    let executor = CommandExecutor::new(event_store);
    
    // Create a valid input
    let input = DepositMoneyInput::new("account-123", 1000, "Initial deposit")?;
    
    // Execute the command
    let result = executor
        .execute(&DepositMoney, input, ExecutionOptions::default())
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
        
        // First, create an account
        let account_id = StreamId::try_new("test-account").unwrap();
        // (In a real system, you'd have an OpenAccount command)
        
        // Test depositing money
        let input = DepositMoneyInput::new("test-account", 500, "Test deposit").unwrap();
        let result = executor
            .execute(&DepositMoney, input, ExecutionOptions::default())
            .await;
        
        // This will fail because account doesn't exist - that's expected!
        assert!(result.is_err());
        
        // The error should be a business rule violation
        if let Err(CommandError::BusinessRuleViolation(msg)) = result {
            assert!(msg.contains("does not exist"));
        } else {
            panic!("Expected BusinessRuleViolation");
        }
    }
    
    #[test]
    fn test_input_validation() {
        // Test validation in the input constructor
        assert!(DepositMoneyInput::new("account-1", 0, "test").is_err());
        assert!(DepositMoneyInput::new("account-1", 100, "").is_err());
        assert!(DepositMoneyInput::new("", 100, "test").is_err());
        
        // Valid input should succeed
        assert!(DepositMoneyInput::new("account-1", 100, "test").is_ok());
    }
}
```

## What You've Learned

1. **Events** represent what happened in your system
2. **Input types** validate data at construction time
3. **State** is rebuilt by folding events
4. **Commands** implement business logic and produce events
5. **Stream access** is type-safe and declared upfront

## Next Steps

- Try creating an `OpenAccount` command
- Add a `WithdrawMoney` command with insufficient funds checking
- Explore multi-stream commands for transfers between accounts
- Learn about projections for building read models

## Common Patterns

### Input Validation Pattern
```rust
impl YourInput {
    pub fn new(field: &str) -> Result<Self, String> {
        // Validate here
        if field.is_empty() {
            return Err("Field cannot be empty".to_string());
        }
        Ok(Self { field: field.to_string() })
    }
}
```

### Business Rule Pattern
```rust
async fn handle(&self, ...) -> CommandResult<Vec<StreamWrite<...>>> {
    // Check business rules using current state
    if !state.meets_condition() {
        return Err(CommandError::BusinessRuleViolation("Rule explanation".to_string()));
    }
    
    // Create events
    Ok(vec![/* events */])
}
```

### Event Folding Pattern
```rust
fn apply(&self, state: &mut State, event: &StoredEvent<Event>) {
    match &event.payload {
        Event::SomethingHappened { data } => {
            // Update state based on what happened
            state.field = data.clone();
        }
    }
}
```

This tutorial covers the fundamentals of EventCore. The key insight is that commands define their own consistency boundaries and can read from and write to multiple streams atomically, making complex business operations simple and reliable.