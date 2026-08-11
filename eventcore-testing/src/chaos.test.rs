use super::*;
use eventcore_memory::InMemoryEventStore;
use eventcore_types::StreamVersion;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PassthroughEvent {
    stream_id: StreamId,
}

impl Event for PassthroughEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "PassthroughEvent"
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

    assert!(!chaos_store.should_inject(FailureProbability::try_new(0.5).expect("0.5 is valid")));
}
