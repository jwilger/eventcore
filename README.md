# EventCore

[![CI](https://github.com/jwilger/eventcore/workflows/CI/badge.svg)](https://github.com/jwilger/eventcore/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

> **⚠️ EXPERIMENTAL - NOT READY FOR USE**
>
> This project is in early development. APIs are unstable and subject to breaking changes.
> The library is not yet published to crates.io, and referenced packages/examples may be incomplete or non-existent.
>
> **Do not use this in production or depend on it for any real projects.**

A type-safe event sourcing library implementing **multi-stream event sourcing** with dynamic consistency boundaries - commands that can atomically read from and write to multiple event streams.

## Why EventCore?

Traditional event sourcing forces you into rigid aggregate boundaries. EventCore breaks free with:

- **Multi-stream commands**: Read and write multiple streams atomically
- **Type-safe by design**: Illegal states are unrepresentable
- **Dynamic stream discovery**: Commands can discover streams at runtime
- **Zero boilerplate**: No aggregate classes, just commands and events

## Quick Start

> **Note:** The following is a design vision, not current reality. Packages are not yet published.

```toml
# Cargo.toml (EXAMPLE - not yet available on crates.io)
[dependencies]
eventcore = { version = "0.6", features = ["memory"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

```rust
use eventcore::{
    Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy,
    StreamId, execute,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};

// Define your events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum BankAccountEvent {
    MoneyDeposited { account_id: StreamId, amount: u64 },
}

impl Event for BankAccountEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            Self::MoneyDeposited { account_id, .. } => account_id,
        }
    }
    fn event_type_name() -> &'static str { "BankAccountEvent" }
}

// Define your command with #[derive(Command)]
#[derive(Command)]
struct DepositMoney {
    #[stream]
    account_id: StreamId,
    amount: u64,
}

impl CommandLogic for DepositMoney {
    type Event = BankAccountEvent;
    type State = ();

    // apply takes OWNED state, returns OWNED state (pure fold)
    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    // handle is SYNC, returns Result<NewEvents<...>, CommandError>
    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![BankAccountEvent::MoneyDeposited {
            account_id: self.account_id.clone(),
            amount: self.amount,
        }].into())
    }
}

// execute() is a free function — no CommandExecutor needed
let store = InMemoryEventStore::new();
let command = DepositMoney {
    account_id: StreamId::try_new("account-alice")?,
    amount: 10000,
};
execute(&store, command, RetryPolicy::new()).await?;
```

## Key Features

### Type-Safe Stream Access

The `#[derive(Command)]` macro automatically generates boilerplate from `#[stream]` fields:

```rust
#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}

// Automatically generates:
// - TransferMoneyStreamSet phantom type for compile-time stream safety
// - Helper method __derive_read_streams() for stream extraction
// - Enables type Input = Self pattern for simple commands
```

### Dynamic Stream Discovery

Some commands only learn about additional streams after inspecting state (e.g., an order references a payment-method stream). Implement `StreamResolver<State>` and return `Some(self)` from `CommandLogic::stream_resolver()` to opt in:

```rust
impl CommandLogic for ProcessPayment {
    type State = CheckoutState;
    type Event = CheckoutEvent;

    fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>> {
        Some(self)
    }

    // apply + handle omitted
}

impl StreamResolver<CheckoutState> for ProcessPayment {
    fn discover_related_streams(&self, state: &CheckoutState) -> Vec<StreamId> {
        state.payment_method_stream.clone().into_iter().collect()
    }
}
```

The executor deduplicates IDs returned by `discover_related_streams`, reads each stream exactly once, and includes every visited stream in the same optimistic concurrency check as the statically declared streams.

### Built-in Concurrency Control

Optimistic locking prevents conflicts automatically. Just execute your commands - version checking and retries are handled transparently.

## Architecture

```
eventcore/              # Core library - re-exports types, macros, and optional adapters
eventcore-types/        # Shared vocabulary - traits and types (StreamId, Event, EventStore)
eventcore-macros/       # Derive macros (re-exported by eventcore)
eventcore-postgres/     # PostgreSQL adapter (enabled via feature flag)
eventcore-sqlite/       # SQLite adapter with optional SQLCipher encryption
eventcore-memory/       # In-memory store for tests and development
eventcore-testing/      # Contract tests, EventCollector, TestScenario
eventcore-examples/     # Integration test examples
```

## Feature Flags

| Feature    | Default | Description                                               |
| ---------- | ------- | --------------------------------------------------------- |
| `macros`   | Yes     | Re-exports `#[derive(Command)]` from `eventcore-macros`   |
| `postgres` | No      | Re-exports `PostgresEventStore` from `eventcore-postgres` |
| `sqlite`   | No      | Re-exports `SqliteEventStore` from `eventcore-sqlite`     |

```toml
# Default (includes macros)
eventcore = "0.6"

# With PostgreSQL adapter
eventcore = { version = "0.6", features = ["postgres"] }

# Without macros (rare - for minimal builds)
eventcore = { version = "0.6", default-features = false }
```

## Examples

See [eventcore-examples/](eventcore-examples/) for complete working examples:

- **Banking**: Account transfers with balance tracking
- **E-commerce**: Order workflow with inventory management
- **Sagas**: Order fulfillment with distributed transaction coordination
- **Web Framework Integration**: REST API with Axum (task management system)

## Documentation

- [User Manual](docs/manual/README.md) - Comprehensive guide from basics to production
- [Architecture Overview](docs/manual/01-introduction/04-architecture.md) - System design
- [Examples](eventcore-examples/tests/) - Working integration test examples
- [Architecture Decision Records](docs/adr/) - Design decisions and rationale

## Development

```bash
# Setup
nix develop              # Enter dev environment
docker-compose up -d     # Start PostgreSQL

# Test
cargo nextest run --workspace  # Fast parallel tests
cargo test --workspace         # Fallback test runner
```

## Contributing

EventCore follows strict type-driven development. See [CLAUDE.md](CLAUDE.md) for our development philosophy.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
