//! Checker scaling benchmarks. The fixture uses the public checker algorithm,
//! not a benchmark-only graph implementation.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use eventcore::model::{CheckOptions, Descriptor, check_descriptors};
use eventcore::{
    Command, CommandError, CommandLogic, Event, ModelCommand, ModelEvent, ModelState, NewEvents,
    StreamId,
    model::{ModelCommandLogic, Modeled, ModeledEvents},
};

fn leaked(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn linear_model(node_count: usize) -> Vec<Descriptor> {
    let root = leaked("BenchmarkInput.root".to_owned());
    let mut descriptors = vec![Descriptor::field("input", root, true)];
    let mut previous = root;

    for index in 0..node_count {
        let target = leaked(format!("BenchmarkNode{index}.value"));
        let sources = Box::leak(vec![previous].into_boxed_slice());
        let temporal = Box::leak(vec![false].into_boxed_slice());
        let name = leaked(format!("BenchmarkMapping{index}"));
        descriptors.push(Descriptor::field(
            if index + 1 == node_count {
                "output"
            } else {
                "read_model"
            },
            target,
            false,
        ));
        descriptors.push(Descriptor::mapping(name, sources, target, temporal));
        previous = target;
    }

    descriptors
}

fn bench_model_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("model/check_linear");
    for node_count in [100, 1_000, 10_000] {
        let descriptors = linear_model(node_count);
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &descriptors,
            |bench, descriptors| {
                bench.iter(|| {
                    check_descriptors(descriptors, CheckOptions::default())
                        .expect("generated linear provenance graph is complete");
                });
            },
        );
    }
    group.finish();
}

#[derive(Command)]
struct LegacyNoop {
    #[stream]
    stream: StreamId,
}

impl CommandLogic for LegacyNoop {
    type Event = BenchEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(NewEvents::default())
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, ModelEvent)]
struct BenchEvent {
    stream: StreamId,
}

impl Event for BenchEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream
    }
    fn event_type_name() -> &'static str {
        "bench-event"
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
    type Event = BenchEvent;
    type State = NoopState;

    fn evolve(&self, state: Modeled<Self::State>, _event: &Self::Event) -> Modeled<Self::State> {
        state
    }

    fn decide(
        &self,
        _state: Modeled<Self::State>,
    ) -> Result<ModeledEvents<Self::Event>, CommandError> {
        Ok(ModeledEvents::none("benchmark noop"))
    }
}

fn bench_modeled_command_logic(c: &mut Criterion) {
    let stream = StreamId::try_new("benchmark::stream".to_owned()).expect("valid stream id");
    let legacy = LegacyNoop {
        stream: stream.clone(),
    };
    let modeled = ModeledNoop::model_builder()
        .stream(eventcore::model::FieldValue::from_value(stream))
        .build();
    let mut group = c.benchmark_group("model/command_logic");
    group.bench_function("legacy", |bench| {
        bench.iter(|| CommandLogic::handle(black_box(&legacy), ()))
    });
    group.bench_function("modeled_wrapper", |bench| {
        bench.iter(|| CommandLogic::handle(black_box(&modeled), Default::default()))
    });
    group.finish();
}

criterion_group!(benches, bench_model_check, bench_modeled_command_logic);
criterion_main!(benches);
