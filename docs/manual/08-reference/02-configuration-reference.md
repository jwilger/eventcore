# Chapter 7.2: Configuration Reference

This chapter provides a complete reference for all EventCore configuration options. Use this as a lookup guide when setting up and tuning your EventCore applications.

EventCore deliberately keeps its configuration surface small. There is no global `EventCoreConfig`, no configuration-file loader, and no environment-variable parsing built into the library. Each backend has a small, explicit config struct, and execution/projection behavior is tuned through the `RetryPolicy` and `ProjectionConfig` builders. Wiring those values together — and reading them from files or the environment — is the responsibility of your application.

## Cargo feature flags

| Feature                    | Default | Effect                                                        |
| -------------------------- | ------- | ------------------------------------------------------------- |
| `macros`                   | Yes     | Re-exports the stable `Command` derive                        |
| `postgres`                 | No      | Re-exports the PostgreSQL adapter                             |
| `sqlite`                   | No      | Re-exports the SQLite adapter                                 |
| `experimental-modeling`    | No      | Enables runtime modeled builders, mappings, and wrappers      |
| `experimental-model-check` | No      | Adds checker registration and graph analysis for test targets |

Features whose names retain the `experimental-` prefix are outside the stable
compatibility guarantee even though they ship in ordinary semantic-versioned
releases. Keep `experimental-model-check` in dev dependencies; deployed
artifacts should not be built with that feature or with `--all-features`.
See [Experimental Model Checking](../05-advanced-topics/06-experimental-model-checking.md)
for setup and guarantee boundaries.

## Core Configuration

### EventStore Configuration

Configuration for event store implementations.

#### PostgresConfig

Configuration for the PostgreSQL connection pool. It has three fields, all with defaults.

```rust
use std::time::Duration;
use eventcore_postgres::MaxConnections;

/// Configuration for PostgresEventStore connection pool.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: MaxConnections,
    /// Timeout for acquiring a connection from the pool (default: 30 seconds)
    pub acquire_timeout: Duration,
    /// Idle timeout for connections in the pool (default: 10 minutes)
    pub idle_timeout: Duration,
}
```

`MaxConnections` is a validated newtype wrapping `NonZeroU32` (the pool must
have at least one connection). `PostgresConfig` implements `Default`
(`max_connections = 10`, `acquire_timeout = 30s`, `idle_timeout = 600s`), so
you only override the fields you care about.

The store is constructed with one of three constructors:

```rust
use std::num::NonZeroU32;
use std::time::Duration;
use eventcore_postgres::{MaxConnections, PostgresConfig, PostgresEventStore};

// Default configuration (10 connections, 30s acquire, 10m idle).
let store = PostgresEventStore::new("postgresql://localhost/eventcore").await?;

// Custom configuration via PostgresConfig.
let config = PostgresConfig {
    max_connections: MaxConnections::new(
        NonZeroU32::new(20).expect("20 is non-zero"),
    ),
    acquire_timeout: Duration::from_secs(10),
    idle_timeout: Duration::from_secs(300),
};
let store =
    PostgresEventStore::with_config("postgresql://localhost/eventcore", config).await?;

// Apply the bundled schema migrations.
store.migrate().await;

// Optionally verify connectivity (panics if the database is unreachable).
store.ping().await;
```

If you need pool knobs beyond these three fields, build an `sqlx::Pool<Postgres>`
yourself and hand it to `PostgresEventStore::from_pool(pool)`. That gives you
full control over the underlying `sqlx` pool configuration while still using
EventCore's storage logic.

**Tuning Guidelines:**

- **max_connections**: 2-4x CPU cores for CPU-bound workloads, higher for
  I/O-bound. The pool must hold at least one connection.
- **acquire_timeout**: how long a caller waits for a free connection before
  failing. Raise it for bursty workloads, lower it to fail fast under
  saturation.
- **idle_timeout**: how long an unused connection is kept open. Shorter values
  release database resources sooner; longer values reduce reconnect churn.

#### SqliteConfig

Configuration for the SQLite event store.

```rust
use std::path::PathBuf;

/// Configuration for SqliteEventStore.
///
/// `Debug` is hand-implemented so the encryption key is never printed.
#[derive(Clone)]
pub struct SqliteConfig {
    /// Path to the SQLite database file.
    pub path: PathBuf,
    /// Optional SQLCipher encryption key.
    pub encryption_key: Option<String>,
}
```

`SqliteConfig` derives only `Clone`; its `Debug` implementation redacts
`encryption_key` (printed as `[REDACTED]`) so secrets do not leak into logs.

```rust
use eventcore_sqlite::{SqliteConfig, SqliteEventStore};
use std::path::PathBuf;

// File-backed store.
let config = SqliteConfig {
    path: PathBuf::from("./events.db"),
    encryption_key: None,
};
let store = SqliteEventStore::new(config)?;
store.migrate().await?;

// In-memory store (for testing).
let store = SqliteEventStore::in_memory()?;
store.migrate().await?;
```

`SqliteEventStore::new` and `in_memory` are synchronous and return
`Result<Self, SqliteEventStoreError>`; `migrate` is async. You can also wrap an
existing `rusqlite::Connection` with `SqliteEventStore::from_connection(conn)`.

**Notes:**

- SQLite uses a single-writer model — appropriate for single-process apps.
- WAL mode is enabled for better read concurrency.
- In-process projector coordination is used (no advisory locks needed).
- **Encryption is feature-gated.** SQLCipher at-rest encryption requires the
  non-default `encryption` Cargo feature
  (`features = ["encryption"]`). With the default `bundled` feature only, the
  database is plaintext and any `encryption_key` you set is silently ignored.
  Enable `encryption` whenever you populate `encryption_key`.

#### Other backends

- **In-memory:** `eventcore_memory::InMemoryEventStore::new()` — a
  zero-dependency store for tests and development. It takes no configuration.
- **File-based:** `eventcore_fs::FileEventStore::open(path)?` or
  `::open_with_config(FsConfig)?` for a git-mergeable, file-backed store.

### Command Execution Configuration

EventCore uses the free function `execute(store, command, RetryPolicy)` as its
canonical entry point. There is no separate executor-config struct, timeout
struct, or concurrency struct — execution behavior is tuned entirely through
`RetryPolicy`, and each command declares its own consistency boundaries via
`#[derive(Command)]`.

```rust
use eventcore::{execute, RetryPolicy};

// Execute with the default retry policy.
let response = execute(store, command, RetryPolicy::new()).await?;

// `ExecutionResponse` exposes how many attempts the command took.
println!("committed after {} attempt(s)", response.attempts());
```

`execute` takes the `store` and `command` **by value** and returns
`Result<ExecutionResponse, CommandError>`. `ExecutionResponse` carries a single
piece of information — `attempts() -> u32` — which is `1` on first-try success
and higher when optimistic-concurrency retries occurred.

#### RetryPolicy

`RetryPolicy` controls how `execute()` retries optimistic-concurrency conflicts.
It is a builder-style struct, not an enum: construct it with `RetryPolicy::new()`
(equivalent to `RetryPolicy::default()`) and refine it with `max_retries`,
`backoff_strategy`, and `with_metrics_hook`.

```rust
use eventcore::{BackoffStrategy, DelayMilliseconds, RetryPolicy};

// Default: 4 retries (5 total attempts) with exponential backoff, 10ms base.
let default = RetryPolicy::new();

// Disable retries entirely (the outer caller owns retry behavior).
let no_retry = RetryPolicy::new().max_retries(0);

// Fixed backoff for rate-limited backends.
let fixed = RetryPolicy::new()
    .max_retries(5)
    .backoff_strategy(BackoffStrategy::Fixed {
        delay_ms: DelayMilliseconds::new(100),
    });

// Exponential backoff (base delay doubled per attempt) for high-traffic systems.
let exponential = RetryPolicy::new()
    .max_retries(3)
    .backoff_strategy(BackoffStrategy::Exponential {
        base_ms: DelayMilliseconds::new(10),
    });
```

`BackoffStrategy` has two variants:

- **`Fixed { delay_ms }`** — the same delay between every retry attempt;
  predictable timing for rate-limited APIs.
- **`Exponential { base_ms }`** — `base_ms * 2^attempt`; reduces load
  during high-traffic periods (the default). EventCore applies jitter during
  execution to avoid synchronized retries.

EventCore retries optimistic-concurrency conflicts only; business-rule
violations and other errors surface immediately without retrying. When retries
are exhausted, `execute()` returns `CommandError::ConcurrencyError(attempts)`.

#### Metrics integration (MetricsHook)

EventCore does **not** ship a metrics subsystem, a `MetricsConfig`, or a
`monitoring` module. Instead it exposes a single callback trait, `MetricsHook`,
which you attach to a `RetryPolicy`. Your application owns the actual metrics
backend (Prometheus, OpenTelemetry, StatsD, logs, etc.).

```rust
use eventcore::{MetricsHook, RetryContext, RetryPolicy};

struct MyMetricsHook;

impl MetricsHook for MyMetricsHook {
    fn on_retry_attempt(&self, ctx: &RetryContext) {
        // ctx.streams: Vec<StreamId> being retried
        // ctx.attempt: AttemptNumber (1-based)
        // ctx.delay_ms: DelayMilliseconds before this retry
        // Record these in your own metrics backend.
    }
}

let policy = RetryPolicy::new().with_metrics_hook(MyMetricsHook);
```

For tracing and structured logs, EventCore emits `tracing` spans and events
(including `delay_ms`, `attempt`, and `stream_id` on retry warnings). Configure
a `tracing` subscriber in your application to collect them; EventCore does not
configure logging on your behalf.

## Projection Configuration

### ProjectionConfig

`ProjectionConfig` controls how `run_projection` polls and processes events. It
has private fields and a builder API; construct it with
`ProjectionConfig::default()` and refine it with builder methods. The default is
**batch mode** — process all currently available events, then stop.

```rust
use std::time::Duration;
use eventcore::{run_projection, ProjectionConfig};

// Default: batch mode.
let config = ProjectionConfig::default();

// Continuous mode with a custom poll interval.
let config = ProjectionConfig::default()
    .continuous()
    .poll_interval(Duration::from_millis(200));

// `run_projection` takes the projector by value, the backend by reference,
// and the config by value (three arguments).
run_projection(my_projector, &backend, config).await?;
```

Builder methods (each returns `Self` for chaining):

- **`.continuous()`** — switch from batch mode to continuous polling. The runner
  keeps polling for new events until stopped.
- **`.poll_interval(Duration)`** — interval between polls when events are
  available.
- **`.empty_poll_backoff(Duration)`** — additional delay when a poll finds no
  events.
- **`.poll_failure_backoff(Duration)`** — additional delay after a poll failure.
- **`.max_consecutive_poll_failures(MaxConsecutiveFailures)`** — how many
  consecutive poll failures are tolerated before the runner stops.
- **`.event_retry_max_attempts(MaxRetryAttempts)`** — retry attempts for a
  failing event.
- **`.event_retry_delay(Duration)`** — initial delay between event retries.
- **`.event_retry_backoff_multiplier(BackoffMultiplier)`** — exponential
  multiplier applied to the event-retry delay.
- **`.event_retry_max_delay(Duration)`** — cap on the event-retry delay.

The validated newtypes used by the retry/poll tuning methods
(`MaxConsecutiveFailures`, `MaxRetryAttempts`, `BackoffMultiplier`) live in
`eventcore_types`; import them from there when you need to override those
defaults.

### Per-event failure handling (FailureStrategy)

How a single failing event is handled is decided by the projector, not by
`ProjectionConfig`. A `Projector` inspects a `FailureContext` and returns a
`FailureStrategy`:

- **`FailureStrategy::Fatal`** — stop processing and surface the error
  (unrecoverable or corrupting failures).
- **`FailureStrategy::Skip`** — skip the event and continue (poison/malformed
  events that are safe to ignore).
- **`FailureStrategy::Retry`** — retry the event according to the
  `ProjectionConfig` event-retry settings (likely-transient failures).

`run_projection` returns `Result<(), ProjectionError>`.

## What EventCore Does Not Configure

The following configuration mechanisms **do not exist** in EventCore. If you
need any of these capabilities, they belong to your application, not the
library.

### No environment-variable configuration

EventCore reads no environment variables and recognizes no `EVENTCORE_*`
prefix. If you want to drive configuration from the environment, read the
variables in your application and translate them into `PostgresConfig`,
`SqliteConfig`, `RetryPolicy`, and `ProjectionConfig` values yourself.

```rust
// Application-level: you own the env-var reading.
use std::num::NonZeroU32;
use eventcore_postgres::{MaxConnections, PostgresConfig, PostgresEventStore};

let url = std::env::var("DATABASE_URL")?;
let max = std::env::var("DB_MAX_CONNECTIONS")
    .ok()
    .and_then(|s| s.parse::<u32>().ok())
    .and_then(NonZeroU32::new)
    .map(MaxConnections::new)
    .unwrap_or_else(|| PostgresConfig::default().max_connections);

let config = PostgresConfig { max_connections: max, ..PostgresConfig::default() };
let store = PostgresEventStore::with_config(url, config).await?;
```

### No configuration files or config loader

There is no `EventCoreConfig`, `ConfigBuilder`, or built-in support for
`eventcore.toml` / `eventcore.yaml` / JSON config files, and no four-level
precedence chain. Use whatever configuration crate you prefer (for example
`config`, `figment`, or `serde`-based loading) in your application and construct
the small EventCore config structs from the result. EventCore intentionally
leaves configuration sourcing and precedence to the host application.

### No CLI

There is no `eventcore-cli` crate and no EventCore command-line binary. Database
migrations are applied programmatically through the backend's `migrate()` method
(`PostgresEventStore::migrate().await`,
`SqliteEventStore::migrate().await?`).

### No built-in monitoring, security, or auth subsystems

EventCore has no `monitoring` module and no `MetricsConfig`, `TracingConfig`,
`LoggingConfig`, `SecurityConfig`, `TlsConfig`, `AuthConfig`, or
`EncryptionConfig` types. Observability is integrated through the `MetricsHook`
trait (above) plus the `tracing` ecosystem, and transport security /
authentication are concerns of your deployment (for example, TLS terminated at
your database connection string or a proxy, and authorization enforced in your
command layer). The only built-in at-rest encryption is SQLCipher for SQLite,
enabled via the `encryption` Cargo feature and `SqliteConfig::encryption_key`
(see [SqliteConfig](#sqliteconfig)).

### No event-schema registry or upcasting

EventCore has no `SchemaRegistry`, `EventSerializer`, or upcasting subsystem.
Per [ADR-0035](../../adr/), schema evolution is handled with `serde`:

- **Additive changes** (new optional fields): add the field with
  `#[serde(default)]` so existing serialized events still deserialize.
- **Incompatible changes**: introduce a **new event variant**. Handlers and
  projectors match all variants, so old events keep deserializing into the old
  variant while new logic emits the new one.

### The write path is `handle()` + `execute()`

There are no `EventToWrite`, `EventMetadata`, `ExpectedVersion`, `EventVersion`,
`EventId`, `StreamEvents`, `WriteResult`, or `ReadOptions` types, and no
`write_events` / `read_streams` / `write_versioned_events` methods to configure.
A command's `handle()` returns `NewEvents`, and `execute()` appends them
atomically with optimistic concurrency control. Internally the `EventStore`
trait uses `append_events(StreamWrites)` and `read_stream(StreamId) ->
EventStream`; versions are `StreamVersion` and projection cursors are
`StreamPosition`. Consumers do not construct these directly — `execute()` and
`run_projection()` own that machinery.

## Errors

Command execution returns `CommandError`, whose variants are:

- **`BusinessRuleViolation(Box<dyn Error + Send + Sync>)`** — a business rule
  failed (typically produced by the `require!` macro or a typed command error).
- **`ConcurrencyError(u32)`** — optimistic-concurrency conflict persisted after
  exhausting the configured retries; the `u32` is the attempt count.
- **`EventStoreError(EventStoreError)`** — a backend storage failure.
- **`ValidationError(String)`** — the command reached an invalid execution state.

`EventStoreError` (from the backend) includes variants such as
`ConflictingExpectedVersions`, `UndeclaredStream`, `SerializationFailed`,
`DeserializationFailed`, `StoreFailure`, and `VersionConflict`. See the
[Error Reference](./03-error-reference.md) for the full breakdown.

This completes the configuration reference. Every EventCore configuration knob
is documented above with its real shape, defaults, and tuning guidance.

Next, explore [Error Reference](./03-error-reference.md) →
