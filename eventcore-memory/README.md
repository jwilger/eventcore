# eventcore-memory

In-memory event store adapter for EventCore - perfect for testing and development.

## Features

- **Zero setup** - No database required
- **Thread-safe** - Safe for concurrent testing
- **Fast** - No I/O overhead
- **Deterministic** - Consistent test results
- **Full API compatibility** - Drop-in replacement

## Installation

```toml
[dev-dependencies]
eventcore-memory = "0.1"
```

## Usage in Tests

```rust
use eventcore_memory::MemoryEventStore;
use eventcore::{CommandExecutor, testing::*};

#[tokio::test]
async fn test_my_command() {
    // Create store - no setup needed!
    let store = MemoryEventStore::new();
    let executor = CommandExecutor::new(store);
    
    // Test your commands
    let result = executor.execute(MyCommand { ... }).await;
    assert!(result.is_ok());
}
```

## Test Patterns

### Given-When-Then Testing

```rust
use eventcore::testing::CommandTestHarness;

#[tokio::test]
async fn transfer_should_move_money() {
    CommandTestHarness::new()
        .given_events(vec![
            AccountOpened { id: "alice", balance: Money::new(1000) },
            AccountOpened { id: "bob", balance: Money::new(0) },
        ])
        .when(TransferMoney { 
            from: "alice", 
            to: "bob", 
            amount: Money::new(100) 
        })
        .then_expect_events(vec![
            MoneyWithdrawn { account: "alice", amount: Money::new(100) },
            MoneyDeposited { account: "bob", amount: Money::new(100) },
        ])
        .run()
        .await
        .unwrap();
}
```

### Testing Concurrency

```rust
#[tokio::test]
async fn concurrent_transfers_should_not_overdraw() {
    let store = MemoryEventStore::new();
    let executor = CommandExecutor::new(store);
    
    // Setup account
    executor.execute(OpenAccount { 
        id: "alice", 
        initial: Money::new(100) 
    }).await.unwrap();
    
    // Try concurrent transfers
    let transfer1 = executor.execute(TransferMoney {
        from: "alice", to: "bob", amount: Money::new(60)
    });
    
    let transfer2 = executor.execute(TransferMoney {
        from: "alice", to: "charlie", amount: Money::new(60)
    });
    
    let (result1, result2) = tokio::join!(transfer1, transfer2);
    
    // One should succeed, one should fail
    assert!(result1.is_ok() ^ result2.is_ok());
}
```

### Testing Projections

```rust
#[tokio::test]
async fn projection_should_track_balances() {
    let store = MemoryEventStore::new();
    let mut projection = BalanceProjection::new();
    
    // Apply events
    let events = vec![
        AccountOpened { id: "alice", balance: Money::new(1000) },
        MoneyWithdrawn { account: "alice", amount: Money::new(100) },
    ];
    
    for event in events {
        projection.apply(&event).await.unwrap();
    }
    
    // Check projection state
    assert_eq!(projection.balance("alice"), Money::new(900));
}
```

## Limitations

The memory adapter is **for testing only**:

- ❌ No persistence - data lost on restart
- ❌ No distributed transactions
- ❌ Limited concurrency control compared to PostgreSQL
- ❌ No query capabilities beyond basic operations

For production, use [eventcore-postgres](../eventcore-postgres/).

## Advanced Testing

### Snapshot Testing

```rust
#[test]
fn test_with_snapshot() {
    let store = MemoryEventStore::new();
    
    // Take snapshot
    let snapshot = store.snapshot();
    
    // Make changes
    store.append_events(...).await.unwrap();
    
    // Restore snapshot
    store.restore(snapshot);
    
    // Store is back to original state
}
```

### Chaos Testing

```rust
#[test]
async fn test_random_failures() {
    let store = MemoryEventStore::with_chaos(ChaosConfig {
        failure_rate: 0.1,  // 10% chance of failure
        latency: Some(Duration::from_millis(100)),
    });
    
    // Test your error handling
}
```

### Performance Testing

```rust
#[test]
async fn benchmark_command_throughput() {
    let store = MemoryEventStore::new();
    let executor = CommandExecutor::new(store);
    
    let start = Instant::now();
    for i in 0..10_000 {
        executor.execute(TestCommand { id: i }).await.unwrap();
    }
    let elapsed = start.elapsed();
    
    println!("Commands/sec: {}", 10_000.0 / elapsed.as_secs_f64());
}
```

## Integration with Test Frameworks

### With `rstest`

```rust
use rstest::*;

#[fixture]
fn event_store() -> MemoryEventStore {
    MemoryEventStore::new()
}

#[fixture]
fn executor(event_store: MemoryEventStore) -> CommandExecutor<MemoryEventStore> {
    CommandExecutor::new(event_store)
}

#[rstest]
#[tokio::test]
async fn test_with_fixtures(executor: CommandExecutor<MemoryEventStore>) {
    // Your test here
}
```

### With `proptest`

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn transfer_properties(
        amount in 1..1000u64,
        initial in 1000..10000u64
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryEventStore::new();
            // Property-based testing
        });
    }
}
```

## Debugging

Enable trace logging to see all operations:

```rust
#[test]
fn debug_test() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .try_init();
        
    let store = MemoryEventStore::new();
    // All operations will be logged
}
```

## See Also

- [EventCore Core](../eventcore/) - Core library documentation
- [Test Utilities](../eventcore/src/testing/) - Testing helpers
- [Examples](../eventcore-examples/) - Complete test examples