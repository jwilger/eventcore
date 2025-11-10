# Chapter 3.1: Commands and the Macro System

This chapter explores how EventCore's command system works, focusing on the `#[derive(Command)]` macro that eliminates boilerplate while maintaining type safety.

## The Command Pattern

Commands in EventCore represent user intentions - things that should happen in your system. They:

1. **Declare required streams** - What data they need access to
2. **Validate business rules** - Ensure operations are allowed
3. **Generate events** - Record what actually happened
4. **Maintain consistency** - All changes are atomic

## Anatomy of a Command

Let's dissect a command to understand each part:

```rust
#[derive(Command, Clone)]         // 1. Derive macro generates boilerplate
struct TransferMoney {
    #[stream]                     // 2. Declares this field is a stream
    from_account: StreamId,

    #[stream]
    to_account: StreamId,

    amount: Money,                // 3. Regular fields for command data
    reference: String,
}
```

### What the Macro Generates

The `#[derive(Command)]` macro generates several things:

```rust
// 1. A phantom type for compile-time stream tracking
#[derive(Debug, Clone, Copy, Default)]
pub struct TransferMoneyStreamSet;

// 2. Implementation of CommandStreams trait
impl CommandStreams for TransferMoney {
    type StreamSet = TransferMoneyStreamSet;

    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![
            self.from_account.clone(),
            self.to_account.clone(),
        ]).expect("valid stream declarations")
    }
}

// 3. Blanket implementation gives you Command trait
// (because TransferMoney also implements CommandLogic)
```

## The Two-Trait Design

EventCore splits the Command pattern into two traits:

### CommandStreams (Generated)

Handles infrastructure concerns:

```rust
pub trait CommandStreams: Send + Sync + Clone {
    /// Phantom type for compile-time stream access control
    type StreamSet: Send + Sync;

    /// Returns the declared streams for this command as a StreamDeclarations value
    fn stream_declarations(&self) -> StreamDeclarations;
}
```

### CommandLogic (You Implement)

Contains your domain logic:

```rust
#[async_trait]
pub trait CommandLogic: CommandStreams {
    /// State type that will be reconstructed from events
    type State: Default + Send + Sync;

    /// Event type this command produces
    type Event: Send + Sync;

    /// Apply an event to update state (event sourcing fold)
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>);

    /// Business logic that validates and produces events
    async fn handle(
        &self,
        stream_declarations: StreamDeclarations,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
}
```

## Stream Declaration Patterns

### Basic Stream Declaration

```rust
#[derive(Command, Clone)]
struct UpdateProfile {
    #[stream]
    user_id: StreamId,  // Single stream
}
```

### Multiple Streams

```rust
#[derive(Command, Clone)]
struct ProcessOrder {
    #[stream]
    order_id: StreamId,

    #[stream]
    customer_id: StreamId,

    #[stream]
    inventory_id: StreamId,

    #[stream]
    payment_id: StreamId,
}
```

### Stream Arrays (Planned Feature)

```rust
#[derive(Command, Clone)]
struct BulkUpdate {
    #[stream("items")]
    item_ids: Vec<StreamId>,  // Multiple streams of same type
}
```

### Conditional Streams

For streams discovered at runtime:

```rust
async fn handle(
    &self,
    stream_declarations: StreamDeclarations,
    state: Self::State,
    stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Discover we need another stream based on state
    if state.requires_approval {
        let approver_stream = StreamId::from_static("approver-stream");
        stream_resolver.add_streams(vec![approver_stream]);
        // EventCore will re-execute with the additional stream
    }

    // Continue with logic...
}
```

## Type-Safe Stream Access

The `StreamDeclarations` value ensures you can only write to declared streams:

```rust
// In your handle method:
async fn handle(
    &self,
    stream_declarations: StreamDeclarations,
    state: Self::State,
    _stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // ✅ This works - from_account was declared with #[stream]
    let withdraw_event = StreamWrite::new(
        &stream_declarations,
        self.from_account.clone(),
        BankEvent::MoneyWithdrawn { amount: self.amount }
    )?;

    // ❌ This won't compile - random_stream wasn't declared
    let invalid = StreamWrite::new(
        &stream_declarations,
        StreamId::from_static("random-stream"),
        SomeEvent {}
    )?; // Compile error!

    Ok(vec![withdraw_event])
}
```

## State Reconstruction

The `apply` method builds state by folding events:

```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        BankEvent::AccountOpened { balance, .. } => {
            state.exists = true;
            state.balance = *balance;
        }
        BankEvent::MoneyDeposited { amount, .. } => {
            state.balance += amount;
        }
        BankEvent::MoneyWithdrawn { amount, .. } => {
            state.balance = state.balance.saturating_sub(*amount);
        }
    }
}
```

This is called for each event in sequence to rebuild current state.

## Command Validation Patterns

### Using the `require!` Macro

```rust
async fn handle(
    &self,
    stream_declarations: StreamDeclarations,
    state: Self::State,
    _stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Business rule validation with good error messages
    require!(
        state.balance >= self.amount,
        "Insufficient funds: balance={}, requested={}",
        state.balance,
        self.amount
    );

    require!(
        self.amount > 0,
        "Transfer amount must be positive"
    );

    require!(
        self.from_account != self.to_account,
        "Cannot transfer to same account"
    );

    // Generate events after validation passes
    Ok(vec![/* events */])
}
```

### Custom Validation Functions

```rust
impl TransferMoney {
    fn validate_transfer_limits(&self, state: &AccountState) -> CommandResult<()> {
        const DAILY_LIMIT: u64 = 10_000;

        let daily_total = state.transfers_today + self.amount;
        require!(
            daily_total <= DAILY_LIMIT,
            "Daily transfer limit exceeded: {} > {}",
            daily_total,
            DAILY_LIMIT
        );

        Ok(())
    }
}
```

## Advanced Macro Features

### Custom Stream Names

```rust
#[derive(Command, Clone)]
struct ComplexCommand {
    #[stream(name = "primary")]
    main_stream: StreamId,

    #[stream(name = "secondary", optional = true)]
    optional_stream: Option<StreamId>,
}
```

### Computed Streams

```rust
impl ComplexCommand {
    fn compute_streams(&self) -> Vec<StreamId> {
        let mut streams = vec![self.main_stream.clone()];

        if let Some(ref optional) = self.optional_stream {
            streams.push(optional.clone());
        }

        streams
    }
}
```

## Command Composition

Commands can be composed for complex operations:

```rust
#[derive(Command, Clone)]
struct CompleteOrderWorkflow {
    #[stream]
    order_id: StreamId,

    // Sub-commands to execute
    payment: ProcessPayment,
    fulfillment: FulfillOrder,
    notification: SendNotification,
}

impl CommandLogic for CompleteOrderWorkflow {
    // ... implementation delegates to sub-commands
}
```

## Performance Optimizations

### Pre-computed State

For expensive computations:

```rust
#[derive(Default)]
struct PrecomputedState {
    balance: u64,
    transaction_count: u64,
    daily_totals: HashMap<Date, u64>,  // Pre-aggregated
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    // Update pre-computed values incrementally
    match &event.payload {
        BankEvent::MoneyTransferred { amount, date, .. } => {
            state.balance -= amount;
            *state.daily_totals.entry(*date).or_insert(0) += amount;
        }
        // ...
    }
}
```

### Lazy State Loading

For large states:

```rust
struct LazyState {
    core: AccountCore,           // Always loaded
    history: Option<Box<TransactionHistory>>,  // Load on demand
}

async fn handle(
    &self,
    stream_declarations: StreamDeclarations,
    mut state: Self::State,
    _stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Load history only if needed
    if self.requires_history_check() {
        state.load_history().await?;
    }

    // Continue...
}
```

## Testing Commands

### Unit Testing

```rust
#[test]
fn test_command_stream_declaration() {
    let cmd = TransferMoney {
        from_account: StreamId::from_static("account-1"),
        to_account: StreamId::from_static("account-2"),
        amount: 100,
        reference: "test".to_string(),
    };

    let streams = cmd.stream_declarations();
    assert_eq!(streams.len(), 2);
    assert!(streams.contains(&StreamId::from_static("account-1")));
    assert!(streams.contains(&StreamId::from_static("account-2")));
}
```

### Testing State Reconstruction

```rust
#[test]
fn test_apply_events() {
    let cmd = TransferMoney { /* ... */ };
    let mut state = AccountState::default();

    let event = create_test_event(BankEvent::AccountOpened {
        balance: 1000,
        owner: "alice".to_string(),
    });

    cmd.apply(&mut state, &event);

    assert_eq!(state.balance, 1000);
    assert!(state.exists);
}
```

## Common Patterns

### Idempotent Commands

Make commands idempotent by checking for duplicate operations:

```rust
async fn handle(/* ... */) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Check if operation was already performed
    if state.transfers.contains(&self.reference) {
        // Already processed - return success with no new events
        return Ok(vec![]);
    }

    // Process normally...
}
```

### Command Versioning

Handle command evolution:

```rust
#[derive(Command, Clone)]
#[command(version = 2)]
struct TransferMoneyV2 {
    #[stream]
    from_account: StreamId,

    #[stream]
    to_account: StreamId,

    amount: Money,
    reference: String,

    // New in V2
    category: TransferCategory,
}
```

## Summary

The EventCore command system provides:

- ✅ **Zero boilerplate** through `#[derive(Command)]`
- ✅ **Type-safe stream access** preventing invalid writes
- ✅ **Clear separation** between infrastructure and domain logic
- ✅ **Flexible validation** with the `require!` macro
- ✅ **Extensibility** through the two-trait design

Key takeaways:

1. Use `#[derive(Command)]` to eliminate boilerplate
2. Declare streams with `#[stream]` attributes
3. Implement business logic in `CommandLogic`
4. Leverage type safety for compile-time guarantees
5. Commands are just data - easy to test and reason about

Next, let's explore [Events and Event Stores](./02-events-and-stores.md) →
