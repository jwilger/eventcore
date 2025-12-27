use std::{future::Future, sync::Mutex};

use eventcore_types::{
    Event, EventStore, EventStoreError, EventStreamReader, EventStreamSlice, Operation, StreamId,
    StreamWrites,
};
use nutype::nutype;
use rand::{Rng, SeedableRng, random, rngs::StdRng};

/// Probability value for chaos engineering injection rates.
///
/// Probability represents a value in the range [0.0, 1.0] where 0.0 means
/// never inject failures and 1.0 means always inject failures.
///
/// # Examples
///
/// ```ignore
/// use eventcore_testing::chaos::Probability;
///
/// let never = Probability::try_new(0.0).unwrap();
/// let sometimes = Probability::try_new(0.5).unwrap();
/// let always = Probability::try_new(1.0).unwrap();
///
/// // Values outside [0.0, 1.0] are rejected
/// assert!(Probability::try_new(1.5).is_err());
/// assert!(Probability::try_new(-0.1).is_err());
/// ```
#[nutype(
    validate(greater_or_equal = 0.0, less_or_equal = 1.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, Into)
)]
pub struct Probability(f32);

#[derive(Debug, Clone)]
pub struct ChaosConfig {
    deterministic_seed: Option<u64>,
    failure_probability: Probability,
    version_conflict_probability: Probability,
}

impl ChaosConfig {
    pub fn deterministic() -> Self {
        Self {
            deterministic_seed: Some(0),
            ..Self::default()
        }
    }

    pub fn with_failure_probability(mut self, probability: f32) -> Self {
        self.failure_probability = Probability::try_new(probability.clamp(0.0, 1.0))
            .expect("clamped value is always valid");
        self
    }

    pub fn with_version_conflict_probability(mut self, probability: f32) -> Self {
        self.version_conflict_probability = Probability::try_new(probability.clamp(0.0, 1.0))
            .expect("clamped value is always valid");
        self
    }
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            deterministic_seed: None,
            failure_probability: Probability::try_new(0.0).expect("0.0 is valid probability"),
            version_conflict_probability: Probability::try_new(0.0)
                .expect("0.0 is valid probability"),
        }
    }
}

pub trait ChaosEventStoreExt: Sized {
    fn with_chaos(self, config: ChaosConfig) -> ChaosEventStore<Self>;
}

pub struct ChaosEventStore<S> {
    store: S,
    config: ChaosConfig,
    rng: Mutex<StdRng>,
}

impl<S> ChaosEventStore<S> {
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

    fn should_inject(&self, probability: Probability) -> bool {
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
    ) -> impl Future<Output = Result<EventStreamReader<E>, EventStoreError>> + Send {
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
                return Err(EventStoreError::VersionConflict);
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
mod tests {
    use super::*;
    use eventcore::StreamVersion;
    use eventcore_memory::InMemoryEventStore;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct PassthroughEvent {
        stream_id: StreamId,
    }

    impl Event for PassthroughEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    #[test]
    fn deterministic_config_sets_seed() {
        let default_is_none = ChaosConfig::default().deterministic_seed.is_none();
        let deterministic_is_some = ChaosConfig::deterministic().deterministic_seed.is_some();

        assert!(default_is_none && deterministic_is_some);
    }

    #[tokio::test]
    async fn zero_probability_passthrough_allows_normal_operations() {
        let stream_id = StreamId::try_new("zero-probability-stream").expect("valid stream id");
        let append_writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| {
                writes.append(PassthroughEvent {
                    stream_id: stream_id.clone(),
                })
            })
            .expect("writes builder should succeed");

        let base_store = InMemoryEventStore::new();
        let chaos_store = base_store.with_chaos(ChaosConfig::default());
        let append_result = chaos_store.append_events(append_writes).await;
        let read_result = chaos_store.read_stream::<PassthroughEvent>(stream_id).await;

        assert!(append_result.is_ok() && read_result.is_ok());
    }

    #[test]
    fn deterministic_half_probability_does_not_inject_immediately() {
        let chaos_store = ChaosEventStore::new(
            InMemoryEventStore::new(),
            ChaosConfig::deterministic().with_failure_probability(0.5),
        );

        assert!(!chaos_store.should_inject(Probability::try_new(0.5).unwrap()));
    }
}
