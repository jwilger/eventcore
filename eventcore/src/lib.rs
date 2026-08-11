//! # EventCore
//!
//! Type-safe, multi-stream event sourcing for Rust with dynamic consistency
//! boundaries.
//!
//! EventCore lets a single command read from and atomically write to multiple
//! event streams in one optimistic-concurrency-controlled transaction. You
//! describe *what* a command does — which streams it touches, how it folds past
//! events into state, and what new events it produces — and EventCore handles
//! the infrastructure: loading state, detecting concurrent writes, retrying on
//! conflict, and committing atomically.
//!
//! APIs exposed only through feature flags whose names start with
//! `experimental-` are disabled by default and are not covered by EventCore's
//! stable compatibility guarantee while they retain that prefix.
//!
//! ## Core concepts
//!
//! - **Stream** — an ordered, append-only sequence of events identified by a
//!   [`StreamId`]. Each stream has a version that increments with every append.
//! - **Command** — a unit of business logic implementing [`CommandLogic`].
//!   Its `apply` method folds events into command-local state (the *write
//!   model*); its `handle` method validates business rules and returns the new
//!   events to append. The streams a command may touch are declared with
//!   `#[derive(Command)]` and the `#[stream]` attribute.
//! - **[`execute`]** — the canonical entry point. It loads the declared
//!   streams, folds them into state, calls `handle`, and atomically appends the
//!   resulting events with optimistic concurrency control, retrying per the
//!   supplied [`RetryPolicy`].
//! - **Projection** — a *read model* built by replaying events. Implement
//!   [`Projector`] and drive it with [`run_projection`]. Read models and write
//!   models are kept on separate code paths.
//!
//! ## Quick start: your first command
//!
//! This example defines a `Deposit` command for a bank account, executes it
//! against the in-memory store, and is fully runnable. Add `eventcore` and
//! `eventcore-memory` to your `Cargo.toml`, then:
//!
//! ```
//! use eventcore::{
//!     execute, Command, CommandError, CommandLogic, Event, NewEvents, RetryPolicy, StreamId,
//! };
//! use eventcore_memory::InMemoryEventStore;
//! use serde::{Deserialize, Serialize};
//!
//! // 1. Define your domain events. Each event knows which stream it belongs to.
//! #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
//! enum BankAccountEvent {
//!     MoneyDeposited { account_id: StreamId, amount: u32 },
//! }
//!
//! impl Event for BankAccountEvent {
//!     fn stream_id(&self) -> &StreamId {
//!         match self {
//!             BankAccountEvent::MoneyDeposited { account_id, .. } => account_id,
//!         }
//!     }
//!     fn event_type_name() -> &'static str {
//!         "BankAccountEvent"
//!     }
//! }
//!
//! // 2. Define a command. `#[derive(Command)]` generates the stream
//! //    declarations from the `#[stream]`-tagged fields.
//! #[derive(Command)]
//! struct Deposit {
//!     #[stream]
//!     account_id: StreamId,
//!     amount: u32,
//! }
//!
//! // 3. Implement the business logic: how events fold into state (`apply`)
//! //    and what events the command produces (`handle`).
//! impl CommandLogic for Deposit {
//!     type Event = BankAccountEvent;
//!     type State = ();
//!
//!     fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
//!         state
//!     }
//!
//!     fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
//!         Ok(vec![BankAccountEvent::MoneyDeposited {
//!             account_id: self.account_id.clone(),
//!             amount: self.amount,
//!         }]
//!         .into())
//!     }
//! }
//!
//! // 4. Execute the command against a store.
//! # fn main() {
//! let rt = tokio::runtime::Runtime::new().expect("runtime");
//! rt.block_on(async {
//!     let store = InMemoryEventStore::new();
//!     let account_id = StreamId::try_new("account-42").expect("valid stream id");
//!
//!     let command = Deposit { account_id, amount: 100 };
//!     execute(&store, command, RetryPolicy::new())
//!         .await
//!         .expect("deposit to succeed");
//! });
//! # }
//! ```
//!
//! From here, see the [user manual](https://github.com/jwilger/eventcore)
//! for projections, multi-stream atomicity, and backend selection, or the
//! `eventcore-demo` crate for a complete bank application backed by PostgreSQL.
//!
//! ## Reading events directly
//!
//! Most applications never read events by hand — [`execute`] does it for you.
//! When you do need a stream's raw history (for tooling or a projection),
//! [`EventStore::read_stream`]
//! returns a lazy [`EventStream`], an async `Stream` of events. To materialize
//! the whole history into a `Vec`, pass it to the [`collect_events`] helper.
//!
//! ## Backends
//!
//! EventCore works with several [`EventStore`] implementations:
//!
//! - `eventcore-memory` — a separate zero-dependency crate added directly to
//!   your `Cargo.toml` (used in the quick start above) for tests and examples.
//! - `postgres` feature — PostgreSQL backend with ACID transactions,
//!   re-exported as `eventcore::postgres`.
//! - `sqlite` feature — SQLite backend with optional SQLCipher encryption,
//!   re-exported as `eventcore::sqlite`.
//!
//! ## Error handling
//!
//! - [`execute`] returns `Ok(`[`ExecutionResponse`]`)` on success; the
//!   response exposes [`ExecutionResponse::attempts`] so callers can observe
//!   how many retries occurred.
//! - [`CommandError`] is returned by [`execute`] on failure — business-rule
//!   violations, concurrency conflicts (after retries are exhausted), and
//!   store failures.
//! - `EventStoreError` (in `eventcore-types`) is returned by backend
//!   operations, including version conflicts and event deserialization
//!   failures.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

mod effects;
mod execute_pipeline;
mod projection;
mod projection_pipeline;

#[cfg(feature = "experimental-modeling")]
pub mod model;

// Re-export application-developer types from eventcore-types
pub use eventcore_types::{
    AttemptNumber, CommandError, CommandLogic, CommandStreams, DelayMilliseconds, Event,
    EventStream, FailureContext, FailureStrategy, NewEvents, Projector, StreamDeclarations,
    StreamId, StreamPosition, StreamResolver, collect_events,
};

// Internal imports for types used by this crate but not re-exported
use eventcore_types::{EventStore, MaxRetries, StreamVersion, StreamWrites};

// Re-export projection public API
pub use projection::{ProjectionConfig, ProjectionError, run_projection};

// Re-export Command derive macro when the "macros" feature is enabled (default)
// Users can disable with: eventcore = { version = "...", default-features = false }
#[cfg(feature = "macros")]
pub use eventcore_macros::Command;

#[cfg(feature = "experimental-modeling")]
pub use eventcore_macros::{
    ModelCommand, ModelEffect, ModelEvent, ModelInput, ModelOutput, ModelReadModel, ModelState,
    StreamIdentity,
};

#[cfg(feature = "experimental-modeling")]
pub use eventcore_macros::mapping;

/// Registration hook emitted by experimental model macros when the checker is
/// unavailable. Keeping this a macro avoids evaluating checker-only tokens in
/// ordinary production builds.
#[cfg(all(
    feature = "experimental-modeling",
    not(feature = "experimental-model-check")
))]
#[doc(hidden)]
#[macro_export]
macro_rules! __eventcore_register_model_descriptor {
    ($($tokens:tt)*) => {};
}

#[cfg(feature = "experimental-model-check")]
#[doc(hidden)]
pub mod __private {
    pub use inventory;
}

/// Registration hook emitted by experimental model macros when checking is
/// enabled. The generated metadata is intentionally absent without the feature.
#[cfg(feature = "experimental-model-check")]
#[doc(hidden)]
#[macro_export]
macro_rules! __eventcore_register_model_descriptor {
    (field, $role:expr, $owner:expr, $field:expr, $root:expr $(,)?) => {
        $crate::__private::inventory::submit! {
            $crate::model::Descriptor::field_at(
                $role,
                concat!($owner, ".", $field),
                $root,
                concat!(file!(), ":", line!()),
            )
        }
    };
    (mapping, $name:expr, $sources:expr, $target:expr, $temporal_sources:expr $(,)?) => {
        $crate::__private::inventory::submit! {
            $crate::model::Descriptor::mapping_at(
                $name,
                $sources,
                $target,
                $temporal_sources,
                concat!(file!(), ":", line!()),
            )
        }
    };
    (assumption, $name:expr, $target:expr $(,)?) => {
        $crate::__private::inventory::submit! {
            $crate::model::Descriptor::assumption_at(
                $name,
                $target,
                concat!(file!(), ":", line!()),
            )
        }
    };
}

// Re-export PostgreSQL backend when the "postgres" feature is enabled
#[cfg(feature = "postgres")]
pub use eventcore_postgres as postgres;

// Re-export SQLite backend when the "sqlite" feature is enabled
#[cfg(feature = "sqlite")]
pub use eventcore_sqlite as sqlite;

/// Validates a business rule condition and returns early with a
/// `CommandError` when the condition is false.
///
/// Designed for command handlers (or any function returning
/// `Result<_, CommandError>`) so domain invariants stay close to the logic
/// without verbose boilerplate.
///
/// # Examples
///
/// With a literal message:
/// ```
/// # use eventcore::{require, CommandError};
/// # fn check(balance: u64, amount: u64) -> Result<(), CommandError> {
/// require!(balance >= amount, "Insufficient funds");
/// # Ok(())
/// # }
/// ```
///
/// With a formatted message:
/// ```
/// # use eventcore::{require, CommandError};
/// # fn check(balance: u64, amount: u64) -> Result<(), CommandError> {
/// require!(
///     balance >= amount,
///     "Insufficient: have {}, need {}",
///     balance,
///     amount,
/// );
/// # Ok(())
/// # }
/// ```
///
/// With a typed error (any type implementing `Into<CommandError>`):
/// ```
/// # use eventcore::{require, CommandError};
/// # #[derive(Debug, thiserror::Error)]
/// # enum MyError { #[error("insufficient-funds")] InsufficientFunds }
/// # impl From<MyError> for CommandError {
/// #     fn from(e: MyError) -> Self { CommandError::BusinessRuleViolation(Box::new(e)) }
/// # }
/// # fn check(balance: u64, amount: u64) -> Result<(), CommandError> {
/// require!(balance >= amount, MyError::InsufficientFunds);
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! require {
    ($condition:expr, $error:expr $(,)?) => {
        if !$condition {
            return ::core::result::Result::Err(
                ::core::convert::Into::<$crate::CommandError>::into($error),
            );
        }
    };
    ($condition:expr, $format:expr, $($arg:expr),+ $(,)?) => {
        if !$condition {
            let message = ::std::format!($format, $($arg),+);
            return ::core::result::Result::Err(
                ::core::convert::Into::<$crate::CommandError>::into(message),
            );
        }
    };
}

/// The result of a successful [`execute`] call.
///
/// Returned when a command completes successfully, including state
/// reconstruction, business rule validation, and atomic event persistence.
/// Contains metadata about the execution, most notably how many attempts
/// were needed before the command was committed (1 on first success,
/// higher if optimistic-concurrency retries occurred).
#[derive(Debug)]
pub struct ExecutionResponse {
    attempts: NonZeroU32,
}

impl ExecutionResponse {
    pub(crate) fn new(attempts: NonZeroU32) -> Self {
        Self { attempts }
    }

    /// Returns the number of execution attempts made before the command committed.
    ///
    /// A value of `1` means the command succeeded on the first try. Values greater
    /// than `1` indicate that optimistic-concurrency conflicts triggered retries.
    pub fn attempts(&self) -> u32 {
        self.attempts.get()
    }
}

/// Defines the delay strategy between retry attempts.
///
/// Different backoff strategies are useful for different scenarios:
/// - Fixed: Predictable timing for rate-limited APIs
/// - Exponential: Reduces load during high-traffic periods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffStrategy {
    /// Fixed delay between all retry attempts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::{BackoffStrategy, DelayMilliseconds};
    /// let strategy = BackoffStrategy::Fixed {
    ///     delay_ms: DelayMilliseconds::new(50),
    /// };
    /// ```
    Fixed {
        /// Delay in milliseconds between each retry attempt
        delay_ms: DelayMilliseconds,
    },

    /// Exponential backoff with base delay multiplied by 2^attempt.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::{BackoffStrategy, DelayMilliseconds};
    /// let strategy = BackoffStrategy::Exponential {
    ///     base_ms: DelayMilliseconds::new(10),
    /// };
    /// ```
    Exponential {
        /// Base delay in milliseconds (multiplied by 2^attempt)
        base_ms: DelayMilliseconds,
    },
}

/// Callback trait for integrating with metrics systems during retry lifecycle.
///
/// Library consumers implement this trait to receive notifications about retry
/// attempts, enabling integration with metrics systems like Prometheus.
pub trait MetricsHook: Send + Sync {
    /// Called when a retry attempt is about to be made.
    ///
    /// # Parameters
    ///
    /// * `ctx` - Context about the retry attempt (streams, attempt number, delay)
    fn on_retry_attempt(&self, ctx: &RetryContext);
}

/// Context information passed to metrics hooks during retry lifecycle.
///
/// Provides structured data about the retry attempt for metrics collection.
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// The set of streams being retried (guaranteed non-empty)
    pub streams: Vec<StreamId>,
    /// The current retry attempt number (1-based)
    pub attempt: AttemptNumber,
    /// The delay in milliseconds before this retry attempt
    pub delay_ms: DelayMilliseconds,
}

/// Configuration for automatic retry behavior on concurrency conflicts.
///
/// RetryPolicy allows library consumers to customize how execute() handles
/// version conflicts during command execution. Uses method chaining for
/// ergonomic configuration.
///
/// # Examples
///
/// ```rust
/// # use eventcore::{RetryPolicy, BackoffStrategy, DelayMilliseconds};
/// // Custom retry policy with 2 retries (3 total attempts) instead of default 4 retries
/// let policy = RetryPolicy::new().max_retries(2);
///
/// // Custom retry policy with fixed backoff
/// let policy = RetryPolicy::new()
///     .max_retries(2)
///     .backoff_strategy(BackoffStrategy::Fixed {
///         delay_ms: DelayMilliseconds::new(50),
///     });
/// ```
#[derive(Clone)]
pub struct RetryPolicy {
    max_retries: MaxRetries,
    backoff_strategy: BackoffStrategy,
    metrics_hook: Option<Arc<dyn MetricsHook>>,
}

impl RetryPolicy {
    /// Create a new RetryPolicy with default values.
    ///
    /// Default configuration:
    /// - max_retries: 4 (5 total attempts including initial)
    /// - backoff_strategy: Exponential with 10ms base
    /// - jitter: ±20% (applied during execution)
    pub fn new() -> Self {
        Self {
            max_retries: MaxRetries::new(4),
            backoff_strategy: BackoffStrategy::Exponential {
                base_ms: DelayMilliseconds::new(10),
            },
            metrics_hook: None,
        }
    }

    /// Configure the maximum number of retry attempts.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::RetryPolicy;
    /// let policy = RetryPolicy::new().max_retries(2);
    /// ```
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = MaxRetries::new(retries);
        self
    }

    /// Configure the backoff strategy for retry delays.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::{RetryPolicy, BackoffStrategy, DelayMilliseconds};
    /// let policy = RetryPolicy::new()
    ///     .backoff_strategy(BackoffStrategy::Fixed {
    ///         delay_ms: DelayMilliseconds::new(50),
    ///     });
    /// ```
    pub fn backoff_strategy(mut self, strategy: BackoffStrategy) -> Self {
        self.backoff_strategy = strategy;
        self
    }

    /// Configure a metrics hook for retry lifecycle events.
    ///
    /// The hook will receive callbacks on each retry attempt with structured context data
    /// for metrics collection systems.
    ///
    /// Returns self for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use eventcore::{RetryPolicy, MetricsHook, RetryContext};
    /// struct MyMetricsHook;
    /// impl MetricsHook for MyMetricsHook {
    ///     fn on_retry_attempt(&self, _ctx: &RetryContext) {
    ///         // Record metrics
    ///     }
    /// }
    ///
    /// let policy = RetryPolicy::new()
    ///     .with_metrics_hook(MyMetricsHook);
    /// ```
    pub fn with_metrics_hook<H: MetricsHook + 'static>(mut self, hook: H) -> Self {
        self.metrics_hook = Some(Arc::new(hook));
        self
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate jitter factor from a random value in [0.0, 1.0].
///
/// Converts a uniformly distributed random value into a jitter factor
/// that provides ±20% variation around 1.0.
///
/// # Formula
///
/// `jitter_factor = 1.0 + (random_value - 0.5) * 0.4`
///
/// This produces a range of [0.8, 1.2]:
/// - When random_value = 0.0: jitter = 1.0 + (-0.5 * 0.4) = 0.8
/// - When random_value = 0.5: jitter = 1.0 + (0.0 * 0.4) = 1.0
/// - When random_value = 1.0: jitter = 1.0 + (0.5 * 0.4) = 1.2
///
/// # Arguments
///
/// * `random_value` - A uniformly distributed random value in [0.0, 1.0]
///
/// # Returns
///
/// A jitter factor in the range [0.8, 1.2]
fn calculate_jitter_factor(random_value: f64) -> f64 {
    1.0 + (random_value - 0.5) * 0.4
}

/// Apply jitter factor to a base delay value.
///
/// Multiplies the base delay by the jitter factor and converts to microseconds.
///
/// # Arguments
///
/// * `base_delay` - Base delay in milliseconds
/// * `jitter_factor` - Multiplicative factor to apply (typically in range [0.8, 1.2])
///
/// # Returns
///
/// Jittered delay in milliseconds as u64
fn apply_jitter(base_delay: u64, jitter_factor: f64) -> u64 {
    (base_delay as f64 * jitter_factor) as u64
}

pub(crate) fn build_stream_writes_from_events<C: CommandLogic>(
    events: Vec<C::Event>,
    expected_versions: HashMap<StreamId, StreamVersion>,
) -> Result<StreamWrites, CommandError> {
    expected_versions
        .into_iter()
        .try_fold(
            StreamWrites::new(),
            |writes, (stream_id, expected_version)| {
                writes.register_stream(stream_id, expected_version)
            },
        )
        .and_then(|writes| {
            events
                .into_iter()
                .try_fold(writes, |writes, event| writes.append(event))
        })
        .map_err(CommandError::EventStoreError)
}

pub(crate) fn compute_retry_delay_ms(
    strategy: &BackoffStrategy,
    attempt: u32,
) -> DelayMilliseconds {
    match strategy {
        BackoffStrategy::Fixed { delay_ms } => *delay_ms,
        BackoffStrategy::Exponential { base_ms } => {
            let base_ms_u64: u64 = (*base_ms).into();
            let base_delay = 2_u64
                .checked_pow(attempt)
                .and_then(|exp| base_ms_u64.checked_mul(exp))
                .unwrap_or(u64::MAX);
            let random_value = rand::random::<f64>();
            let jitter_factor = calculate_jitter_factor(random_value);
            DelayMilliseconds::new(apply_jitter(base_delay, jitter_factor))
        }
    }
}

/// Execute a command against the event store with a custom retry policy.
///
/// This is the primary entry point for EventCore. It orchestrates the complete
/// command execution workflow: loading state from multiple streams, validating
/// business rules, and atomically committing resulting events.
///
/// Internally, this function drives an `ExecutePipeline` state machine that
/// yields effects (read stream, append events, sleep). This function is the
/// thin shell loop that dispatches those effects to the `EventStore` trait.
///
/// # Type Parameters
///
/// * `C` - A command implementing [`CommandLogic`] that defines the business operation
/// * `S` - An event store implementing [`EventStore`] for persistence
///
/// # Parameters
///
/// * `store` - The event store for reading/writing events
/// * `command` - The command to execute
/// * `policy` - Retry policy configuration (max attempts, backoff strategy, etc.)
///
/// # Errors
///
/// Returns [`CommandError`] if:
/// - Stream resolution fails
/// - Event loading fails
/// - Business rule validation fails (via command's `handle()`)
/// - Event persistence fails
/// - Optimistic concurrency conflicts occur after exhausting retries
#[tracing::instrument(name = "execute", skip_all, fields())]
pub async fn execute<C, S>(
    store: S,
    command: C,
    policy: RetryPolicy,
) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    use effects::{StoreEffect, StoreEffectResult};
    use execute_pipeline::{ExecutePipeline, PipelineOutcome, PipelineStep};

    let mut pipeline = ExecutePipeline::new(command, policy);
    let mut step = pipeline.step();

    loop {
        match step {
            PipelineStep::Done(PipelineOutcome::Success(response)) => return Ok(response),
            PipelineStep::Done(PipelineOutcome::Error(err)) => return Err(err),
            PipelineStep::WaitForResult => {
                // The pipeline folded a streamed event and is ready for the
                // next item; the surrounding `ReadStream` arm owns the pump
                // loop, so reaching this arm at top level is a logic error.
                unreachable!("WaitForResult is consumed inside the ReadStream pump loop")
            }
            PipelineStep::Yield(StoreEffect::ReadStream { stream_id }) => {
                // Open the stream, then push events into the pipeline one at a
                // time so the fold happens incrementally. The whole stream is
                // never collected into a Vec here — that is the point of #364.
                step = pump_stream_reads::<C, S>(&store, &mut pipeline, stream_id).await;
            }
            PipelineStep::Yield(StoreEffect::AppendEvents { writes }) => {
                let result = store.append_events(writes).await;
                step = pipeline.resume(StoreEffectResult::EventsAppended(result));
            }
            PipelineStep::Yield(StoreEffect::Sleep { duration }) => {
                tokio::time::sleep(duration).await;
                step = pipeline.resume(StoreEffectResult::Slept);
            }
        }
    }
}

/// Open a stream and push its events into the pipeline one event at a time.
///
/// This is the heart of the #364 streaming-read change: instead of collecting
/// the entire stream into a `Vec` and folding it in one shot, the shell pulls
/// each event from the async stream and hands it to `pipeline.resume(...)`,
/// which folds it into the in-progress command state immediately. Only the
/// in-progress state is retained — the stream is never materialized here.
///
/// Returns the next `PipelineStep` for the outer execution loop to drive: the
/// step the pipeline reaches once the stream ends, or a `Done(Error)` if
/// opening the stream or decoding an event failed.
async fn pump_stream_reads<C, S>(
    store: &S,
    pipeline: &mut execute_pipeline::ExecutePipeline<C>,
    stream_id: StreamId,
) -> execute_pipeline::PipelineStep
where
    C: CommandLogic,
    S: EventStore,
{
    use effects::StoreEffectResult;
    use execute_pipeline::PipelineStep;
    use futures::StreamExt;

    let mut events = match store.read_stream::<C::Event>(stream_id).await {
        Ok(events) => events,
        Err(e) => return pipeline.resume(StoreEffectResult::StreamReadError(e)),
    };

    while let Some(item) = events.next().await {
        match item {
            Ok(event) => {
                let step = pipeline.resume(StoreEffectResult::StreamEvent(event));
                debug_assert!(matches!(step, PipelineStep::WaitForResult));
            }
            Err(e) => return pipeline.resume(StoreEffectResult::StreamReadError(e)),
        }
    }

    pipeline.resume(StoreEffectResult::StreamEnded)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
