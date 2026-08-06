//! Allocation control for the modeled command wrapper.
//!
//! This is the only test in its integration-test process so the test harness
//! cannot allocate concurrently while DHAT records the two hot loops.

use std::hint::black_box;

use eventcore::{
    Command, CommandError, CommandLogic, Event, ModelCommand, ModelEvent, ModelState, NewEvents,
    StreamId,
    model::{ModelCommandLogic, Modeled, ModeledEvents},
};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(Command)]
struct LegacyNoop {
    #[stream]
    stream: StreamId,
}

impl CommandLogic for LegacyNoop {
    type Event = AllocationEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(NewEvents::default())
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, ModelEvent)]
struct AllocationEvent {
    stream: StreamId,
}

impl Event for AllocationEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream
    }

    fn event_type_name() -> &'static str {
        "allocation-event"
    }
}

#[derive(ModelCommand)]
struct ModeledNoop {
    #[stream]
    stream: StreamId,
}

#[derive(ModelState)]
struct NoopState {
    #[model(default)]
    processed: bool,
}

impl ModelCommandLogic for ModeledNoop {
    type Event = AllocationEvent;
    type State = NoopState;

    fn evolve(&self, state: Modeled<Self::State>, _event: &Self::Event) -> Modeled<Self::State> {
        state
    }

    fn decide(
        &self,
        _state: Modeled<Self::State>,
    ) -> Result<ModeledEvents<Self::Event>, CommandError> {
        Ok(ModeledEvents::none("allocation benchmark noop"))
    }
}

#[test]
fn modeled_wrapper_matches_the_zero_allocation_legacy_decision_path() {
    let stream = StreamId::try_new("benchmark::allocation".to_owned()).expect("valid stream id");
    let legacy = LegacyNoop {
        stream: stream.clone(),
    };
    let modeled = ModeledNoop::model_builder()
        .stream(eventcore::model::FieldValue::from_value(stream))
        .build();

    // Warm both paths before starting the profiler so one-time runtime setup
    // cannot be attributed to either implementation.
    let _ = black_box(CommandLogic::handle(&legacy, ())).expect("legacy noop succeeds");
    let _ = black_box(CommandLogic::handle(&modeled, Default::default()))
        .expect("modeled noop succeeds");

    let _profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();

    for _ in 0..1_000 {
        let _ =
            black_box(CommandLogic::handle(black_box(&legacy), ())).expect("legacy noop succeeds");
    }
    let after_legacy = dhat::HeapStats::get();

    for _ in 0..1_000 {
        let _ = black_box(CommandLogic::handle(
            black_box(&modeled),
            Default::default(),
        ))
        .expect("modeled noop succeeds");
    }
    let after_modeled = dhat::HeapStats::get();

    let legacy_allocations = after_legacy.total_blocks - before.total_blocks;
    let modeled_allocations = after_modeled.total_blocks - after_legacy.total_blocks;

    dhat::assert_eq!(legacy_allocations, 0);
    dhat::assert_eq!(modeled_allocations, legacy_allocations);
}
