# EventCore

[![CI](https://github.com/jwilger/eventcore/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/jwilger/eventcore/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/eventcore.svg)](https://crates.io/crates/eventcore)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A type-safe event sourcing library implementing **multi-stream event sourcing** with dynamic consistency boundaries - commands that can atomically read from and write to multiple event streams.

## Why EventCore?

Traditional event sourcing forces you into rigid aggregate boundaries. EventCore breaks free with:

- **Multi-stream commands**: Read and write multiple streams atomically
- **Type-safe by design**: Illegal states are unrepresentable
- **Dynamic stream discovery**: Commands can discover streams at runtime
- **Zero boilerplate**: No aggregate classes, just commands and events

## Quick Start

EventCore and its companion crates are published on crates.io.

```toml
# Cargo.toml
[dependencies]
eventcore = "1.0"
# The in-memory store used below is a separate crate:
eventcore-memory = "1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

The in-memory store lives in the `eventcore-memory` crate. The PostgreSQL and
SQLite backends are enabled via the `postgres` and `sqlite` features on the
`eventcore` crate.

## Experimental model checking

`1.1.0-alpha.1` introduces an opt-in experiment for runtime-coupled Event
Modeling checks. It is not part of the stable API or an official release
direction. Enable `experimental-modeling` to use typed modeled builders and
`experimental-model-check` in test targets to validate executable field
provenance:

```toml
[dev-dependencies]
eventcore = { version = "=1.1.0-alpha.1", features = ["experimental-model-check"] }
```

See the [experimental model-checking manual chapter](docs/manual/05-advanced-topics/06-experimental-model-checking.md)
and the executable bank-transfer example in
`eventcore-examples/tests/experimental_modeling_test.rs`.

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
// - impl CommandStreams for TransferMoney, whose stream_declarations()
//   returns the StreamIds from every #[stream] field (here from_account
//   and to_account), establishing the command's atomic consistency boundary
```

### Dynamic Stream Discovery

Some commands only learn about additional streams after inspecting state (e.g., an order references a payment-method stream). Implement `StreamResolver<State>` and return `Some(self)` from `CommandLogic::stream_resolver()` to opt in:

```rust
impl CommandLogic for ProcessPayment {
    type State = CheckoutState;
    type Event = CheckoutEvent;

    fn stream_resolver(&self) -> Option<&(dyn StreamResolver<Self::State> + Sync)> {
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
eventcore-fs/           # File-based git-mergeable store (FileEventStore::open)
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
eventcore = "1.0"

# With PostgreSQL adapter
eventcore = { version = "1.0", features = ["postgres"] }

# Without macros (rare - for minimal builds)
eventcore = { version = "1.0", default-features = false }
```

## Examples

See [eventcore-examples/tests/](eventcore-examples/tests/) for working
integration test examples (banking-style deposit/withdraw commands):

- **Multi-stream atomic transfers** (`multi_stream_atomic_test`): atomic reads
  and writes across multiple account streams
- **Retry policy** (`retry_policy_test`): optimistic-concurrency retry behavior
- **BDD scenario** (`scenario_test`): given/when/then-style command flows
- **Single-stream command** (`single_stream_command_test`): the minimal
  single-stream case

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
