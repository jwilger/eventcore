# Migration Guide: Removal of Input Associated Type

This guide explains how to migrate from the previous Command API (with separate Input types) to the new simplified API where commands are always their own input.

## Overview of Changes

The `Input` associated type has been completely removed from the `CommandStreams` trait. Commands now always serve as their own input, containing all necessary data as fields. This eliminates a common source of boilerplate where input types merely duplicated command fields.

### Key Changes:

1. **No More Input Type**: The `Input` associated type is removed from `CommandStreams`
2. **Commands Contain Data**: All command data is stored directly in the command struct
3. **Simplified handle Method**: The `handle` method no longer takes an input parameter
4. **Simplified Executor**: The executor's `execute` method only takes the command parameter

## Migration Steps

### 1. Merge Input Type into Command Struct

**Before:**
```rust
pub struct TransferMoneyCommand;

pub struct TransferMoneyInput {
    pub from_account: AccountId,
    pub to_account: AccountId,
    pub amount: Money,
    pub description: Option<String>,
}

impl CommandStreams for TransferMoneyCommand {
    type Input = TransferMoneyInput;
    type StreamSet = ();
    
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::from(format!("account-{}", input.from_account)),
            StreamId::from(format!("account-{}", input.to_account)),
        ]
    }
}
```

**After:**
```rust
pub struct TransferMoneyCommand {
    pub from_account: AccountId,
    pub to_account: AccountId,
    pub amount: Money,
    pub description: Option<String>,
}

impl CommandStreams for TransferMoneyCommand {
    type StreamSet = ();
    
    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            StreamId::from(format!("account-{}", self.from_account)),
            StreamId::from(format!("account-{}", self.to_account)),
        ]
    }
}
```

### 2. Update CommandLogic Implementation

**Before:**
```rust
impl CommandLogic for TransferMoneyCommand {
    type State = TransferState;
    type Event = BankingEvent;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,  // Input parameter
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Access data via input parameter
        if input.amount > state.balance {
            return Err(CommandError::InsufficientFunds);
        }
        // ...
    }
}
```

**After:**
```rust
impl CommandLogic for TransferMoneyCommand {
    type State = TransferState;
    type Event = BankingEvent;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Access data via self
        if self.amount > state.balance {
            return Err(CommandError::InsufficientFunds);
        }
        // ...
    }
}
```

### 3. Update Executor Usage

**Before:**
```rust
let command = TransferMoneyCommand;
let input = TransferMoneyInput {
    from_account: AccountId::from("ACC-123"),
    to_account: AccountId::from("ACC-456"),
    amount: Money::from_cents(5000),
    description: Some("Payment".to_string()),
};

let result = executor.execute(&command, input, options).await?;
```

**After:**
```rust
let command = TransferMoneyCommand {
    from_account: AccountId::from("ACC-123"),
    to_account: AccountId::from("ACC-456"),
    amount: Money::from_cents(5000),
    description: Some("Payment".to_string()),
};

let result = executor.execute(command, options).await?;
```

## Using the Derive Macro

The `#[derive(Command)]` macro has been updated to work with the new API:

```rust
#[derive(Command, Clone)]
pub struct TransferMoneyCommand {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
    description: Option<String>,
}

// The macro generates:
// - impl CommandStreams (without Input type)
// - Proper read_streams() method using self
```

## Common Patterns

### Commands That Previously Used `Input = Self`

These commands require minimal changes:

**Before:**
```rust
impl CommandStreams for MyCommand {
    type Input = Self;  // Remove this line
    type StreamSet = ();
    
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        // Change to use self instead of input
    }
}
```

**After:**
```rust
impl CommandStreams for MyCommand {
    type StreamSet = ();
    
    fn read_streams(&self) -> Vec<StreamId> {
        // Use self directly
    }
}
```

### Commands with Validation

Move validation from input constructors into command constructors:

**Before:**
```rust
impl TransferMoneyInput {
    pub fn new(from: AccountId, to: AccountId, amount: Money) -> Result<Self, Error> {
        if from == to {
            return Err(Error::SameAccount);
        }
        Ok(Self { from_account: from, to_account: to, amount })
    }
}
```

**After:**
```rust
impl TransferMoneyCommand {
    pub fn new(from: AccountId, to: AccountId, amount: Money) -> Result<Self, Error> {
        if from == to {
            return Err(Error::SameAccount);
        }
        Ok(Self { from_account: from, to_account: to, amount })
    }
}
```

## Benefits

1. **Reduced Boilerplate**: No more duplicate struct definitions
2. **Simpler Mental Model**: Commands directly contain their data
3. **Better Ergonomics**: Creating and using commands is more straightforward
4. **Clearer Code**: Command requirements are visible in one place

## Potential Issues and Solutions

### Issue: Shared Input Types

If you had multiple commands sharing the same input type:

**Before:**
```rust
struct AccountInput {
    account_id: AccountId,
}

impl CommandStreams for DepositCommand {
    type Input = AccountInput;
    // ...
}

impl CommandStreams for WithdrawCommand {
    type Input = AccountInput;
    // ...
}
```

**Solution**: Each command should contain its own fields:

```rust
struct DepositCommand {
    account_id: AccountId,
    amount: Money,
}

struct WithdrawCommand {
    account_id: AccountId,
    amount: Money,
}
```

If you need to share validation logic, use a common constructor function or trait.

### Issue: Large Input Types

For commands with many fields, consider using builder patterns:

```rust
impl TransferMoneyCommand {
    pub fn builder() -> TransferMoneyCommandBuilder {
        TransferMoneyCommandBuilder::default()
    }
}

#[derive(Default)]
pub struct TransferMoneyCommandBuilder {
    // ... fields
}

impl TransferMoneyCommandBuilder {
    pub fn from_account(mut self, account: AccountId) -> Self {
        self.from_account = Some(account);
        self
    }
    
    pub fn build(self) -> Result<TransferMoneyCommand, Error> {
        // Validation and construction
    }
}
```

## Complete Example

See the updated examples in `eventcore-examples/`:
- `banking/commands.rs` - Shows the migration for banking commands
- `ecommerce/commands.rs` - Demonstrates multi-stream commands
- `sagas/commands.rs` - Long-running process commands

## Summary

This change simplifies the EventCore API by removing an unnecessary abstraction. Commands now directly contain their data, making the library easier to understand and use while reducing boilerplate code.