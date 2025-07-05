<div align="center">
  <img src="static/logo.png" alt="EventCore Logo" width="300">
  
  # EventCore
  
  **Multi-stream event sourcing with dynamic consistency boundaries**
  
  [![Crates.io](https://img.shields.io/crates/v/eventcore.svg)](https://crates.io/crates/eventcore)
  [![Documentation](https://docs.rs/eventcore/badge.svg)](https://docs.rs/eventcore)
  [![License](https://img.shields.io/crates/l/eventcore.svg)](https://github.com/jwilger/eventcore/blob/main/LICENSE)
  [![Build Status](https://github.com/jwilger/eventcore/workflows/CI/badge.svg)](https://github.com/jwilger/eventcore/actions)
</div>

---

## Why EventCore?

Traditional event sourcing forces you to choose aggregate boundaries upfront, leading to complex workarounds when business logic spans multiple aggregates. EventCore eliminates this constraint with **dynamic consistency boundaries** - each command defines exactly which streams it needs, enabling atomic operations across multiple event streams.

### 🚀 Key Features

<div class="features-grid">

**🔄 Multi-Stream Atomicity**  
Read from and write to multiple event streams in a single atomic operation. No more saga patterns for simple cross-aggregate operations.

**🎯 Type-Safe Commands**  
Leverage Rust's type system to ensure compile-time correctness. Illegal states are unrepresentable.

**⚡ High Performance**  
Optimized for both in-memory and PostgreSQL backends with sophisticated caching and batching strategies.

**🔍 Built-in CQRS**  
First-class support for projections and read models with automatic position tracking and replay capabilities.

**🛡️ Production Ready**  
Battle-tested with comprehensive observability, monitoring, and error recovery mechanisms.

**🧪 Testing First**  
Extensive testing utilities including property-based tests, chaos testing, and deterministic event stores.

</div>

## Quick Example

```rust
use eventcore::prelude::*;

#[derive(Command)]
#[command(event = "BankingEvent")]
struct TransferMoney {
    from_account: AccountId,
    to_account: AccountId,
    amount: Money,
}

impl TransferMoney {
    fn read_streams(&self) -> Vec<StreamId> {
        vec![
            self.from_account.stream_id(),
            self.to_account.stream_id(),
        ]
    }
}

#[async_trait]
impl CommandLogic for TransferMoney {
    type State = BankingState;
    type Event = BankingEvent;

    async fn handle(
        &self,
        _: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Validate business rules
        require!(state.balance(&self.from_account) >= self.amount,
            "Insufficient funds"
        );

        // Emit events - atomically written to both streams
        Ok(vec![
            emit!(self.from_account.stream_id(), 
                BankingEvent::Withdrawn { amount: self.amount }
            ),
            emit!(self.to_account.stream_id(),
                BankingEvent::Deposited { amount: self.amount }
            ),
        ])
    }
}
```

## Getting Started

<div class="cta-buttons">
  <a href="./quickstart.html" class="primary-button">Quick Start Guide</a>
  <a href="./manual/01-introduction/01-what-is-eventcore.html" class="secondary-button">Read the Manual</a>
  <a href="./api/eventcore/index.html" class="secondary-button">API Documentation</a>
</div>

## Use Cases

EventCore excels in domains where business operations naturally span multiple entities:

- **💰 Financial Systems**: Atomic transfers, double-entry bookkeeping, complex trading operations
- **🛒 E-Commerce**: Order fulfillment, inventory management, distributed transactions
- **🏢 Enterprise Applications**: Workflow engines, approval processes, resource allocation
- **🎮 Gaming**: Player interactions, economy systems, real-time state synchronization
- **📊 Analytics Platforms**: Event-driven architectures, audit trails, temporal queries

## Performance

<div class="performance-stats">
  <div class="stat">
    <div class="number">187,711</div>
    <div class="label">ops/sec (in-memory)</div>
  </div>
  <div class="stat">
    <div class="number">83</div>
    <div class="label">ops/sec (PostgreSQL)</div>
  </div>
  <div class="stat">
    <div class="number">12ms</div>
    <div class="label">avg latency</div>
  </div>
  <div class="stat">
    <div class="number">820,000</div>
    <div class="label">events/sec write</div>
  </div>
</div>

## Community

Join our growing community of developers building event-sourced systems:

- 📖 [Comprehensive Documentation](./manual/01-introduction/01-what-is-eventcore.html)
- 💬 [Discord Community](https://discord.gg/eventcore)
- 🐛 [Report Issues](https://github.com/jwilger/eventcore/issues)
- 🤝 [Contributing Guide](./contributing.html)

## Resources

- [GitHub Repository](https://github.com/jwilger/eventcore)
- [crates.io Package](https://crates.io/crates/eventcore)
- [API Documentation](./api/eventcore/index.html)

## Supported By

<div class="sponsors">
  <p>EventCore is an open-source project supported by the community.</p>
  <a href="https://github.com/sponsors/jwilger" class="sponsor-button">Become a Sponsor</a>
</div>

---

<footer>
  <p>Built with ❤️ by the EventCore community</p>
  <p>Released under the <a href="./license.html">MIT License</a></p>
</footer>