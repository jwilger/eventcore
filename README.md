# EventCore

[![CI](https://github.com/jwilger/eventcore/workflows/CI/badge.svg)](https://github.com/jwilger/eventcore/actions)
[![codecov](https://codecov.io/gh/jwilger/eventcore/branch/main/graph/badge.svg)](https://codecov.io/gh/jwilger/eventcore)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License: Apache](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

A type-safe event sourcing library implementing **multi-stream event sourcing** with dynamic consistency boundaries - commands that can atomically read from and write to multiple event streams.

## Why EventCore?

Traditional event sourcing forces you into rigid aggregate boundaries. EventCore breaks free with:

- **Multi-stream commands**: Read and write multiple streams atomically
- **Type-safe by design**: Illegal states are unrepresentable
- **Dynamic stream discovery**: Commands can discover streams at runtime
- **Zero boilerplate**: No aggregate classes, just commands and events

## Quick Start

```toml
# Cargo.toml
[dependencies]
eventcore = "0.1"
eventcore-postgres = "0.1"  # or your preferred adapter
```

```rust
use eventcore::{prelude::*, require, emit};
use eventcore_macros::Command;
use eventcore_postgres::PostgresEventStore;

#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}

#[async_trait]
impl Command for TransferMoney {
    type Input = Self;
    type State = AccountBalances;
    type Event = BankingEvent;
    type StreamSet = TransferMoneyStreamSet;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::MoneyTransferred { from, to, amount } => {
                state.debit(from, *amount);
                state.credit(to, *amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        require!(state.balance(&input.from_account) >= input.amount, "Insufficient funds");
        
        let mut events = vec![];
        emit!(events, &read_streams, input.from_account, BankingEvent::MoneyTransferred {
            from: input.from_account.to_string(),
            to: input.to_account.to_string(),
            amount: input.amount,
        });
        
        Ok(events)
    }
}

let store = PostgresEventStore::new(config).await?;
let executor = CommandExecutor::new(store);

let command = TransferMoney {
    from_account: StreamId::try_new("account-alice")?,
    to_account: StreamId::try_new("account-bob")?,
    amount: Money::from_cents(10000)?,
};

let result = executor.execute(&command, command).await?;
```

## Key Features

### Type-Safe Stream Access
The `#[derive(Command)]` macro automatically detects streams from `#[stream]` fields:

```rust
#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}
```

### Dynamic Stream Discovery
Discover additional streams during execution:

```rust
async fn handle(...) -> CommandResult<Vec<StreamWrite<...>>> {
    require!(state.is_valid(), "Invalid state");
    
    if state.requires_approval() {
        stream_resolver.add_streams(vec![approval_stream()]);
    }
    
    let mut events = vec![];
    emit!(events, &read_streams, input.account, AccountEvent::Updated { ... });
    Ok(events)
}
```

### Built-in Concurrency Control

Optimistic locking prevents conflicts automatically. Just execute your commands - version checking and retries are handled transparently.

## Architecture

```
eventcore/              # Core library - traits and types
eventcore-postgres/     # PostgreSQL adapter  
eventcore-memory/       # In-memory adapter for testing
eventcore-examples/     # Complete examples
```

## Examples

See [eventcore-examples/](eventcore-examples/) for complete working examples:

- **Banking**: Account transfers with balance tracking
- **E-commerce**: Order workflow with inventory management
- **Sagas**: Order fulfillment with distributed transaction coordination

## Documentation

- [Core Library](eventcore/README.md) - Types, traits, and patterns
- [PostgreSQL Adapter](eventcore-postgres/README.md) - Production event store
- [Testing Guide](eventcore-memory/README.md) - In-memory store for tests
- [Examples](eventcore-examples/README.md) - Complete applications

## Development

```bash
# Setup
nix develop              # Enter dev environment
docker-compose up -d     # Start PostgreSQL

# Test
cargo nextest run        # Fast parallel tests
cargo test              # Standard test runner

# Bench
cargo bench             # Performance benchmarks
```

## Performance

Based on current testing with PostgreSQL backend:

- **Single-stream commands**: 86 ops/sec (stable, reliable performance)
- **Multi-stream commands**: Full atomicity operational (estimated 25-50 ops/sec)
- **Batch event writes**: 9,000+ events/sec (excellent bulk throughput)
- **Latency**: P95 ~14ms (database-backed operations)

*Note: Performance optimized for correctness and multi-stream atomicity. See [Performance Report](docs/performance-report.md) for detailed benchmarks and system specifications.*

## Contributing

EventCore follows strict type-driven development. See [CLAUDE.md](CLAUDE.md) for our development philosophy.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.