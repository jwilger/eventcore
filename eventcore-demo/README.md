# EventCore Bank Demo

A small, runnable application that demonstrates [EventCore](../README.md) — a
type-safe, multi-stream event-sourcing library for Rust — backed by
PostgreSQL.

It models a bank and shows the patterns a downstream consumer uses in practice:

- **Commands** with pure `apply`/`handle` logic, declared via
  `#[derive(Command)]` and run through `eventcore::execute()`.
- **Multi-stream atomic writes** — the headline EventCore capability — via a
  `Transfer` command that debits one account stream and credits another in a
  single optimistic-concurrency transaction. Either both events are persisted
  or neither is.
- **Read models / projections** built on a separate code path from the command
  write models, via the `Projector` trait and `eventcore::run_projection()`.
- A **PostgreSQL event store** for durable, ACID-backed persistence.

This demo intentionally uses EventCore's stable command and projection APIs.
The additive, feature-gated modeled version is kept side by side in
[`eventcore-examples/tests/experimental_modeling_test.rs`](../eventcore-examples/tests/experimental_modeling_test.rs).
That executable example follows input → command → event → read model → output,
and runs the information-completeness checker over the same mappings used at
runtime.

## What the demo does

`cargo run -p eventcore-demo` executes a scripted scenario:

1. Opens two accounts (Alice and Bob), each on its own event stream.
2. Deposits money into both.
3. Performs an **atomic multi-stream transfer** from Alice to Bob.
4. Runs a projection to build a `TransactionHistory` read model.
5. Prints the resulting balances and the transaction log.

## Domain model

| Concept           | Type                          | Notes                                               |
| ----------------- | ----------------------------- | --------------------------------------------------- |
| Account stream id | `eventcore::StreamId`         | One stream per account (`new_account_id()`)         |
| Money             | `MoneyAmount` (nutype, `> 0`) | Whole cents; always strictly positive               |
| Account holder    | `AccountHolder` (nutype)      | Trimmed, non-empty name                             |
| Events            | `BankEvent`                   | `AccountOpened`, `MoneyDeposited`, `MoneyWithdrawn` |

### Commands (write model)

- `OpenAccount` — single stream; emits `AccountOpened`; rejects re-opening.
- `Deposit` — single stream; emits `MoneyDeposited`; rejects if not open.
- `Withdraw` — single stream; emits `MoneyWithdrawn`; rejects overdrafts / not open.
- `Transfer` — **multi-stream atomic**: declares both the source and
  destination streams, validates the source has sufficient funds, and emits a
  withdrawal on the source plus a deposit on the destination in one
  `execute()`.

### Projection (read model)

`TransactionHistoryProjector` implements `eventcore::Projector` and folds events
into a `TransactionHistory` (per-account balances + an ordered transaction
log). This is intentionally a separate fold from the commands' write-model
`apply`, following EventCore's CQRS separation: read and write models evolve
independently.

## Prerequisites

- Rust toolchain (see the workspace `rust-toolchain.toml`; `nix develop`
  provides a pinned toolchain).
- Docker / Docker Compose for the PostgreSQL instance.

## Running

The repository root ships a `docker-compose.yml` that runs PostgreSQL on port
`5433` with user/password/database all set to `postgres`. The demo reuses it —
no separate compose file is needed.

```bash
# From the repository root: start PostgreSQL
docker-compose up -d

# Run the demo (defaults to postgres://postgres:postgres@localhost:5433/postgres)
cargo run -p eventcore-demo
```

Point the demo at a different database with `DATABASE_URL`:

```bash
DATABASE_URL="postgres://user:pass@host:5432/dbname" cargo run -p eventcore-demo
```

The demo creates its schema on startup via the idempotent
`PostgresEventStore::migrate()`.

## Tests

The integration tests double as usage examples and exercise only the public
API (`execute`, `run_projection`, the command derive macro, the `Projector`
trait).

```bash
# Fast, deterministic tests against the in-memory store (always run in CI)
cargo test -p eventcore-demo --test bank_account_test

# End-to-end test against the real PostgreSQL backend (needs Postgres running)
cargo test -p eventcore-demo --test postgres_e2e_test
```

`tests/bank_account_test.rs` covers the open → deposit → withdraw happy path,
insufficient-funds and unopened-account rejections, and — most importantly —
the multi-stream `Transfer`: that both balances reflect a successful transfer
and that a _failed_ transfer leaves **both** streams unchanged (proving the
all-or-nothing atomicity guarantee).

`tests/postgres_e2e_test.rs` runs the same flow against PostgreSQL to prove the
backend wiring.

## Code structure

| File                 | Responsibility                                                                 |
| -------------------- | ------------------------------------------------------------------------------ |
| `src/domain.rs`      | Semantic value types (`MoneyAmount`, `AccountHolder`) and the `BankEvent` enum |
| `src/commands.rs`    | Commands + write-model state + typed business-rule errors                      |
| `src/projections.rs` | `TransactionHistory` read model + `TransactionHistoryProjector`                |
| `src/lib.rs`         | Public facade re-exporting the demo's API                                      |
| `src/main.rs`        | Runnable scenario against `PostgresEventStore`                                 |
| `tests/`             | Public-API integration tests (in-memory + Postgres)                            |

## Further reading

- EventCore architecture: [`docs/manual/01-introduction/04-architecture.md`](../docs/manual/01-introduction/04-architecture.md)
- Decision history: [`docs/adr/`](../docs/adr/)
- System blueprints: [`blueprints/`](../blueprints/)
