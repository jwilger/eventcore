# Banking Example

The banking example demonstrates EventCore's multi-stream atomic operations by implementing a double-entry bookkeeping system.

## Key Features

- **Atomic Transfers**: Move money between accounts with ACID guarantees
- **Balance Validation**: Prevent overdrafts with compile-time safe types
- **Audit Trail**: Complete history of all transactions
- **Account Lifecycle**: Open, close, and freeze accounts

## Running the Example

```bash
cargo run --example banking
```

## Code Structure

The example includes:

- `types.rs` - Domain types with validation (AccountId, Money, etc.)
- `events.rs` - Account events (Opened, Deposited, Withdrawn, etc.)
- `commands.rs` - Business operations (OpenAccount, Transfer, etc.)
- `projections.rs` - Read models for account balances and history

[View Source Code](https://github.com/eventcore-rs/eventcore/tree/main/eventcore-examples/src/banking)
