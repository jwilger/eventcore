//! Chaos testing for EventCore backends.
//!
//! This module provides [`ChaosEventStore`], a wrapper around any `EventStore`
//! that injects probabilistic read/write failures and version conflicts. Use it
//! to verify that command retry logic handles transient errors correctly.
//!
//! The canonical entry point is the [`ChaosEventStoreExt::with_chaos`] extension
//! method, which is implemented for every `EventStore`:
//!
//! ```no_run
//! use eventcore_memory::InMemoryEventStore;
//! use eventcore_testing::chaos::{ChaosConfig, ChaosEventStoreExt};
//!
//! // Deterministic seed so failures are reproducible across runs.
//! let base_store = InMemoryEventStore::new();
//! let chaos_store =
//!     base_store.with_chaos(ChaosConfig::deterministic().with_failure_probability(0.5));
//!
//! // `chaos_store` is itself an `EventStore`, so it can be passed to
//! // `eventcore::execute()` like any other backend.
//! # let _ = chaos_store;
//! ```

use std::{future::Future, sync::Mutex};

use eventcore_types::{
    Event, EventStore, EventStoreError, EventStream, EventStreamSlice, Operation, StreamId,
    StreamVersion, StreamWrites,
};
use nutype::nutype;
use rand::{RngExt, SeedableRng, random, rngs::StdRng};

/// Probability of injecting read/write failures for chaos testing.
///
/// FailureProbability represents a value in the range [0.0, 1.0] where 0.0 means
/// never inject failures and 1.0 means always inject failures.
///
/// # Examples
///
/// ```no_run
/// use eventcore_testing::chaos::FailureProbability;
///
/// let never = FailureProbability::try_new(0.0).expect("0.0 is valid");
/// let sometimes = FailureProbability::try_new(0.5).expect("0.5 is valid");
/// let always = FailureProbability::try_new(1.0).expect("1.0 is valid");
/// # let _ = (never, sometimes, always);
///
/// // Values outside [0.0, 1.0] are rejected
/// assert!(FailureProbability::try_new(1.5).is_err());
/// assert!(FailureProbability::try_new(-0.1).is_err());
/// ```
#[nutype(
    validate(greater_or_equal = 0.0, less_or_equal = 1.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, Into)
)]
pub struct FailureProbability(f32);

/// Probability of injecting version conflicts for chaos testing.
///
/// VersionConflictProbability represents a value in the range [0.0, 1.0] where 0.0
/// means never inject conflicts and 1.0 means always inject conflicts.
///
/// # Examples
///
/// ```no_run
/// use eventcore_testing::chaos::VersionConflictProbability;
///
/// let never = VersionConflictProbability::try_new(0.0).expect("0.0 is valid");
/// let sometimes = VersionConflictProbability::try_new(0.5).expect("0.5 is valid");
/// let always = VersionConflictProbability::try_new(1.0).expect("1.0 is valid");
/// # let _ = (never, sometimes, always);
///
/// // Values outside [0.0, 1.0] are rejected
/// assert!(VersionConflictProbability::try_new(1.5).is_err());
/// assert!(VersionConflictProbability::try_new(-0.1).is_err());
/// ```
#[nutype(
    validate(greater_or_equal = 0.0, less_or_equal = 1.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, Into)
)]
pub struct VersionConflictProbability(f32);

/// Configuration controlling how a [`ChaosEventStore`] injects failures.
///
/// A `ChaosConfig` holds an optional deterministic seed plus the probabilities
/// of injecting store failures and version conflicts. The probabilities default
/// to `0.0` (no injection); use the builder methods to raise them.
///
/// Start from either [`ChaosConfig::default`] (random seed, no injection) or
/// [`ChaosConfig::deterministic`] (fixed seed for reproducible runs), then chain
/// [`with_failure_probability`](ChaosConfig::with_failure_probability) and
/// [`with_version_conflict_probability`](ChaosConfig::with_version_conflict_probability).
///
/// ```no_run
/// use eventcore_testing::chaos::ChaosConfig;
///
/// let config = ChaosConfig::deterministic()
///     .with_failure_probability(0.25)
///     .with_version_conflict_probability(0.1);
/// # let _ = config;
/// ```
#[derive(Debug, Clone)]
pub struct ChaosConfig {
    deterministic_seed: Option<u64>,
    failure_probability: FailureProbability,
    version_conflict_probability: VersionConflictProbability,
}

impl ChaosConfig {
    /// Creates a config with a fixed seed so injected failures are reproducible.
    ///
    /// The failure and version-conflict probabilities still default to `0.0`;
    /// chain [`with_failure_probability`](ChaosConfig::with_failure_probability)
    /// and
    /// [`with_version_conflict_probability`](ChaosConfig::with_version_conflict_probability)
    /// to enable injection.
    pub fn deterministic() -> Self {
        Self {
            deterministic_seed: Some(0),
            ..Self::default()
        }
    }

    /// Sets the probability of injecting a store failure on reads and appends.
    ///
    /// `probability` is clamped to the `[0.0, 1.0]` range, where `0.0` never
    /// injects a failure and `1.0` always does.
    pub fn with_failure_probability(mut self, probability: f32) -> Self {
        self.failure_probability = FailureProbability::try_new(probability.clamp(0.0, 1.0))
            .expect("clamped value is always valid");
        self
    }

    /// Sets the probability of injecting a version conflict on appends.
    ///
    /// `probability` is clamped to the `[0.0, 1.0]` range, where `0.0` never
    /// injects a conflict and `1.0` always does.
    pub fn with_version_conflict_probability(mut self, probability: f32) -> Self {
        self.version_conflict_probability =
            VersionConflictProbability::try_new(probability.clamp(0.0, 1.0))
                .expect("clamped value is always valid");
        self
    }
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            deterministic_seed: None,
            failure_probability: FailureProbability::try_new(0.0)
                .expect("0.0 is valid probability"),
            version_conflict_probability: VersionConflictProbability::try_new(0.0)
                .expect("0.0 is valid probability"),
        }
    }
}

/// Extension trait that wraps any `EventStore` in a [`ChaosEventStore`].
///
/// This is the canonical entry point for chaos testing. It is implemented for
/// every `EventStore`, so call `with_chaos` directly on a base store:
///
/// ```no_run
/// use eventcore_memory::InMemoryEventStore;
/// use eventcore_testing::chaos::{ChaosConfig, ChaosEventStoreExt};
///
/// let chaos_store =
///     InMemoryEventStore::new().with_chaos(ChaosConfig::deterministic().with_failure_probability(0.5));
/// # let _ = chaos_store;
/// ```
pub trait ChaosEventStoreExt: Sized {
    /// Wraps `self` in a [`ChaosEventStore`] configured by `config`.
    fn with_chaos(self, config: ChaosConfig) -> ChaosEventStore<Self>;
}

/// An `EventStore` wrapper that injects probabilistic failures and conflicts.
///
/// `ChaosEventStore` forwards reads and appends to the wrapped store, but first
/// rolls against the probabilities in its [`ChaosConfig`]: it may return a
/// `StoreFailure` on either operation, or a `VersionConflict` on appends. Because
/// it implements `EventStore`, it can be passed anywhere a backend is expected,
/// including `eventcore::execute()`.
///
/// Prefer constructing one via [`ChaosEventStoreExt::with_chaos`] rather than
/// calling [`ChaosEventStore::new`] directly.
pub struct ChaosEventStore<S> {
    store: S,
    config: ChaosConfig,
    rng: Mutex<StdRng>,
}

impl<S> ChaosEventStore<S> {
    /// Wraps `store` with chaos injection driven by `config`.
    ///
    /// If the config carries a deterministic seed, the internal RNG is seeded
    /// from it for reproducible failures; otherwise a random seed is used.
    /// Most callers should use [`ChaosEventStoreExt::with_chaos`] instead.
    pub fn new(store: S, config: ChaosConfig) -> Self {
        let rng = match config.deterministic_seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::seed_from_u64(random()),
        };

        Self {
            store,
            config,
            rng: Mutex::new(rng),
        }
    }

    fn should_inject<P: Into<f32>>(&self, probability: P) -> bool {
        let prob_f32: f32 = probability.into();

        if prob_f32 <= 0.0 {
            return false;
        }

        if prob_f32 >= 1.0 {
            return true;
        }

        let mut rng = self
            .rng
            .lock()
            .expect("chaos RNG mutex should not be poisoned");

        rng.random_bool(prob_f32 as f64)
    }
}

impl<S> EventStore for ChaosEventStore<S>
where
    S: EventStore + Sync,
{
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<EventStream<E>, EventStoreError>> + Send {
        let should_fail = self.should_inject(self.config.failure_probability);
        let store = &self.store;

        async move {
            if should_fail {
                return Err(EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                });
            }

            store.read_stream(stream_id).await
        }
    }

    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<Output = Result<EventStreamSlice, EventStoreError>> + Send {
        let should_conflict = self.should_inject(self.config.version_conflict_probability);
        let should_fail = self.should_inject(self.config.failure_probability);
        let store = &self.store;

        async move {
            if should_conflict {
                return Err(EventStoreError::VersionConflict {
                    stream_id: StreamId::try_new("chaos-conflict").expect("valid"),
                    expected: StreamVersion::new(0),
                    actual: StreamVersion::new(1),
                });
            }

            if should_fail {
                return Err(EventStoreError::StoreFailure {
                    operation: Operation::AppendEvents,
                });
            }

            store.append_events(writes).await
        }
    }
}

impl<S> ChaosEventStoreExt for S
where
    S: EventStore + Sync,
{
    fn with_chaos(self, config: ChaosConfig) -> ChaosEventStore<Self> {
        ChaosEventStore::new(self, config)
    }
}

#[cfg(test)]
#[path = "chaos.test.rs"]
mod tests;
