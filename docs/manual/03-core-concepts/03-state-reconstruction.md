# Chapter 3.3: State Reconstruction

State reconstruction is the heart of event sourcing - rebuilding current state by replaying historical events. EventCore makes this process efficient, type-safe, and predictable.

## The Concept

Instead of storing current state in a database, event sourcing:

1. **Stores events** - The facts about what happened
2. **Rebuilds state** - By replaying events in order
3. **Guarantees consistency** - Same events always produce same state

Think of it like a bank account:
- Traditional: Store balance = $1000
- Event Sourcing: Store deposits and withdrawals, calculate balance

## How EventCore Reconstructs State

### The Apply Function

Every command defines how events modify state:

```rust
impl CommandLogic for TransferMoney {
    type State = AccountState;
    type Event = BankEvent;
    
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankEvent::AccountOpened { initial_balance, owner } => {
                state.exists = true;
                state.balance = *initial_balance;
                state.owner = owner.clone();
                state.opened_at = event.occurred_at;
            }
            BankEvent::MoneyDeposited { amount, .. } => {
                state.balance += amount;
                state.transaction_count += 1;
                state.last_activity = event.occurred_at;
            }
            BankEvent::MoneyWithdrawn { amount, .. } => {
                state.balance = state.balance.saturating_sub(*amount);
                state.transaction_count += 1;
                state.last_activity = event.occurred_at;
            }
        }
    }
}
```

### The Reconstruction Process

When a command executes, EventCore:

1. **Reads declared streams** - Gets all events from specified streams
2. **Creates default state** - Starts with `State::default()`
3. **Applies events in order** - Calls `apply()` for each event
4. **Passes state to handle** - Your business logic receives reconstructed state

```rust
// EventCore does this automatically:
let mut state = AccountState::default();
for event in events_from_streams {
    command.apply(&mut state, &event);
}
// Your handle() method receives the final state
```

## State Design Patterns

### Accumulator Pattern

Build up state incrementally:

```rust
#[derive(Default)]
struct OrderState {
    exists: bool,
    items: Vec<OrderItem>,
    total: Money,
    status: OrderStatus,
    customer: Option<CustomerId>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        OrderEvent::Created { customer_id } => {
            state.exists = true;
            state.customer = Some(*customer_id);
            state.status = OrderStatus::Draft;
        }
        OrderEvent::ItemAdded { item, price } => {
            state.items.push(item.clone());
            state.total += price;
        }
        OrderEvent::Placed { .. } => {
            state.status = OrderStatus::Placed;
        }
    }
}
```

### Snapshot Pattern

For expensive computations, pre-calculate during apply:

```rust
#[derive(Default)]
struct AnalyticsState {
    total_revenue: Money,
    transactions_by_day: HashMap<Date, Vec<TransactionSummary>>,
    customer_lifetime_values: HashMap<CustomerId, Money>,
    // Pre-computed aggregates
    daily_averages: HashMap<Date, Money>,
    top_customers: BTreeSet<(Money, CustomerId)>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        AnalyticsEvent::Purchase { customer, amount, date } => {
            // Update raw data
            state.total_revenue += amount;
            state.transactions_by_day
                .entry(*date)
                .or_default()
                .push(TransactionSummary { customer: *customer, amount: *amount });
            
            // Update pre-computed values
            *state.customer_lifetime_values.entry(*customer).or_default() += amount;
            
            // Maintain sorted top customers
            state.top_customers.insert((*amount, *customer));
            if state.top_customers.len() > 100 {
                state.top_customers.pop_first();
            }
            
            // Recalculate daily average for this date
            let daily_total: Money = state.transactions_by_day[date]
                .iter()
                .map(|t| t.amount)
                .sum();
            let tx_count = state.transactions_by_day[date].len();
            state.daily_averages.insert(*date, daily_total / tx_count as u64);
        }
    }
}
```

### State Machine Pattern

Track valid transitions:

```rust
#[derive(Default)]
struct WorkflowState {
    current_phase: WorkflowPhase,
    completed_phases: HashSet<WorkflowPhase>,
    phase_durations: HashMap<WorkflowPhase, Duration>,
    last_transition: DateTime<Utc>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        WorkflowEvent::PhaseCompleted { phase, started_at } => {
            // Record phase duration
            let duration = event.occurred_at - started_at;
            state.phase_durations.insert(*phase, duration);
            
            // Mark as completed
            state.completed_phases.insert(*phase);
            
            // Transition to next phase
            state.current_phase = phase.next_phase();
            state.last_transition = event.occurred_at;
        }
    }
}
```

## Multi-Stream State Reconstruction

When commands read multiple streams, state combines data from all:

```rust
#[derive(Command, Clone)]
struct ProcessPayment {
    #[stream]
    order_id: StreamId,
    
    #[stream]
    customer_id: StreamId,
    
    #[stream]
    payment_method_id: StreamId,
    
    amount: Money,
}

#[derive(Default)]
struct PaymentState {
    // From order stream
    order: OrderInfo,
    
    // From customer stream  
    customer: CustomerInfo,
    customer_payment_history: Vec<PaymentRecord>,
    
    // From payment method stream
    payment_method: PaymentMethodInfo,
    recent_charges: Vec<ChargeAttempt>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    // Events from different streams update different parts of state
    match (&event.stream_id, &event.payload) {
        (stream_id, PaymentEvent::Order(order_event)) 
            if stream_id == &self.order_id => {
            // Update order portion of state
            apply_order_event(&mut state.order, order_event);
        }
        (stream_id, PaymentEvent::Customer(customer_event)) 
            if stream_id == &self.customer_id => {
            // Update customer portion of state
            apply_customer_event(&mut state.customer, customer_event);
        }
        (stream_id, PaymentEvent::PaymentMethod(pm_event)) 
            if stream_id == &self.payment_method_id => {
            // Update payment method portion of state
            apply_payment_method_event(&mut state.payment_method, pm_event);
        }
        _ => {} // Ignore events from other streams
    }
}
```

## Performance Optimization

### Selective State Loading

Only reconstruct what you need:

```rust
#[derive(Default)]
struct AccountState {
    // Core fields - always loaded
    exists: bool,
    balance: Money,
    status: AccountStatus,
    
    // Optional expensive data
    transaction_history: Option<Vec<Transaction>>,
    statistics: Option<AccountStatistics>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    // Always update core fields
    match &event.payload {
        BankEvent::MoneyDeposited { amount, .. } => {
            state.balance += amount;
        }
        // ...
    }
    
    // Only build history if requested
    if state.transaction_history.is_some() {
        if let Some(tx) = event_to_transaction(&event) {
            state.transaction_history
                .as_mut()
                .unwrap()
                .push(tx);
        }
    }
}

// In handle(), decide what to load:
async fn handle(&self, /* ... */) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Enable history loading for this command
    let mut state = Self::State::default();
    if self.requires_history() {
        state.transaction_history = Some(Vec::new());
    }
    
    // State reconstruction will populate history
    // ...
}
```

### Event Filtering

Skip irrelevant events during reconstruction:

```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    // Skip old events for performance
    let cutoff_date = Utc::now() - Duration::days(90);
    if event.occurred_at < cutoff_date {
        return; // Skip events older than 90 days
    }
    
    match &event.payload {
        // Process only recent events
    }
}
```

### Memoization

Cache expensive calculations:

```rust
#[derive(Default)]
struct MemoizedState {
    balance: Money,
    // Cache expensive calculations
    #[serde(skip)]
    cached_risk_score: Option<(DateTime<Utc>, RiskScore)>,
}

impl MemoizedState {
    fn risk_score(&mut self) -> RiskScore {
        let now = Utc::now();
        
        // Check cache validity (1 hour)
        if let Some((cached_at, score)) = self.cached_risk_score {
            if now - cached_at < Duration::hours(1) {
                return score;
            }
        }
        
        // Calculate expensive risk score
        let score = calculate_risk_score(self);
        self.cached_risk_score = Some((now, score));
        score
    }
}
```

## Testing State Reconstruction

### Unit Testing Apply Functions

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::testing::builders::*;
    
    #[test]
    fn test_balance_calculation() {
        let command = TransferMoney { /* ... */ };
        let mut state = AccountState::default();
        
        // Create test events
        let events = vec![
            create_event(BankEvent::AccountOpened { 
                initial_balance: 1000,
                owner: "Alice".to_string(),
            }),
            create_event(BankEvent::MoneyDeposited { 
                amount: 500,
                reference: "Salary".to_string(),
            }),
            create_event(BankEvent::MoneyWithdrawn { 
                amount: 200,
                reference: "Rent".to_string(),
            }),
        ];
        
        // Apply events
        for event in events {
            command.apply(&mut state, &event);
        }
        
        // Verify final state
        assert_eq!(state.balance, 1300); // 1000 + 500 - 200
        assert_eq!(state.transaction_count, 2);
        assert!(state.exists);
    }
}
```

### Property-Based Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn balance_never_negative_with_saturating_sub(
        deposits in prop::collection::vec(1..1000u64, 0..10),
        withdrawals in prop::collection::vec(1..2000u64, 0..20),
    ) {
        let command = TransferMoney { /* ... */ };
        let mut state = AccountState::default();
        
        // Open account
        let open_event = create_event(BankEvent::AccountOpened {
            initial_balance: 0,
            owner: "Test".to_string(),
        });
        command.apply(&mut state, &open_event);
        
        // Apply deposits
        for amount in deposits {
            let event = create_event(BankEvent::MoneyDeposited {
                amount,
                reference: "Deposit".to_string(),
            });
            command.apply(&mut state, &event);
        }
        
        // Apply withdrawals
        for amount in withdrawals {
            let event = create_event(BankEvent::MoneyWithdrawn {
                amount,
                reference: "Withdrawal".to_string(),
            });
            command.apply(&mut state, &event);
        }
        
        // Balance should never be negative due to saturating_sub
        prop_assert!(state.balance >= 0);
    }
}
```

### Testing Event Order Independence

Some state calculations should be order-independent:

```rust
#[test]
fn test_commutative_operations() {
    let events = vec![
        create_tag_added_event("rust"),
        create_tag_added_event("async"),
        create_tag_added_event("eventstore"),
    ];
    
    // Apply in different orders
    let mut state1 = TagState::default();
    for event in &events {
        apply_tag_event(&mut state1, event);
    }
    
    let mut state2 = TagState::default();
    for event in events.iter().rev() {
        apply_tag_event(&mut state2, event);
    }
    
    // Final state should be the same
    assert_eq!(state1.tags, state2.tags);
}
```

## Common Pitfalls and Solutions

### 1. Mutable External State

❌ **Wrong**: Depending on external state
```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        OrderEvent::Created { .. } => {
            // DON'T DO THIS - external dependency!
            state.tax_rate = fetch_current_tax_rate();
        }
    }
}
```

✅ **Right**: Store everything in events
```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        OrderEvent::Created { tax_rate, .. } => {
            // Tax rate was captured when event was created
            state.tax_rate = *tax_rate;
        }
    }
}
```

### 2. Non-Deterministic Operations

❌ **Wrong**: Using current time
```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        OrderEvent::Created { .. } => {
            // DON'T DO THIS - non-deterministic!
            state.age_in_days = (Utc::now() - event.occurred_at).num_days();
        }
    }
}
```

✅ **Right**: Calculate in handle() if needed
```rust
async fn handle(
    &self,
    read_streams: ReadStreams<Self::StreamSet>,
    state: Self::State,
    _stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // Calculate age here, not in apply()
    let age_in_days = (Utc::now() - state.created_at).num_days();
    
    // Use for business logic...
}
```

### 3. Unbounded State Growth

❌ **Wrong**: Keeping everything forever
```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        LogEvent::Entry { message } => {
            // DON'T DO THIS - unbounded growth!
            state.all_log_entries.push(message.clone());
        }
    }
}
```

✅ **Right**: Keep bounded state
```rust
fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        LogEvent::Entry { message, level } => {
            // Keep only recent errors
            if *level == LogLevel::Error {
                state.recent_errors.push(message.clone());
                if state.recent_errors.len() > 100 {
                    state.recent_errors.remove(0);
                }
            }
            
            // Track counts instead of full data
            *state.entries_by_level.entry(*level).or_default() += 1;
        }
    }
}
```

## Advanced Patterns

### Temporal State

Track state changes over time:

```rust
#[derive(Default)]
struct TemporalState {
    current_value: i32,
    history: BTreeMap<DateTime<Utc>, i32>,
    transitions: Vec<StateTransition>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    let old_value = state.current_value;
    
    match &event.payload {
        ValueEvent::Changed { new_value } => {
            state.current_value = *new_value;
            state.history.insert(event.occurred_at, *new_value);
            state.transitions.push(StateTransition {
                at: event.occurred_at,
                from: old_value,
                to: *new_value,
                event_id: event.id,
            });
        }
    }
}

impl TemporalState {
    /// Get value at a specific point in time
    fn value_at(&self, timestamp: DateTime<Utc>) -> Option<i32> {
        self.history
            .range(..=timestamp)
            .next_back()
            .map(|(_, &value)| value)
    }
}
```

### Derived State

Calculate derived values efficiently:

```rust
#[derive(Default)]
struct DerivedState {
    // Raw data
    orders: Vec<Order>,
    
    // Derived data (calculated in apply)
    total_revenue: Money,
    average_order_value: Option<Money>,
    orders_by_status: HashMap<OrderStatus, usize>,
}

fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    match &event.payload {
        OrderEvent::Placed { order } => {
            // Update raw data
            state.orders.push(order.clone());
            
            // Update derived data incrementally
            state.total_revenue += order.total;
            state.average_order_value = Some(
                state.total_revenue / state.orders.len() as u64
            );
            *state.orders_by_status
                .entry(OrderStatus::Placed)
                .or_default() += 1;
        }
    }
}
```

## Summary

State reconstruction in EventCore:

- ✅ **Deterministic** - Same events always produce same state
- ✅ **Type-safe** - State structure defined by types
- ✅ **Efficient** - Only reconstruct what you need
- ✅ **Testable** - Easy to verify with known events
- ✅ **Flexible** - Support any state structure

Best practices:
1. Keep apply() functions pure and deterministic
2. Pre-calculate expensive derived data
3. Design state for your command's needs
4. Test state reconstruction thoroughly
5. Optimize for your access patterns

Next, let's explore [Multi-Stream Atomicity](./04-multi-stream-atomicity.md) →