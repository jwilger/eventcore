use criterion::{criterion_group, criterion_main, Criterion};
use eventcore::{
    command::{Command, CommandResult},
    event::Event,
    event_store::EventStore,
    executor::CommandExecutor,
    metadata::EventMetadata,
    types::{EventId, EventVersion, StreamId},
};
use eventcore_memory::InMemoryEventStore;
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Simple test command for benchmarking
#[derive(Clone)]
struct SimpleCommand;

#[derive(Clone)]
struct SimpleInput {
    target_stream: StreamId,
    value: i64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SimpleEvent {
    value: i64,
}

#[derive(Default, Clone)]
struct SimpleState {
    total: i64,
}

#[async_trait::async_trait]
impl Command for SimpleCommand {
    type Input = SimpleInput;
    type State = SimpleState;
    type Event = SimpleEvent;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![input.target_stream.clone()]
    }

    fn apply(
        &self,
        state: &mut Self::State,
        event: &eventcore::event_store::StoredEvent<Self::Event>,
    ) {
        state.total += event.payload.value;
    }

    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        let event = SimpleEvent {
            value: input.value + state.total,
        };

        Ok(vec![(input.target_stream, event)])
    }
}

/// Benchmark simple command execution
fn bench_simple_command(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let event_store = Arc::new(InMemoryEventStore::new());
    let executor = CommandExecutor::new(event_store);

    c.bench_function("simple_command_execution", |b| {
        b.to_async(&rt).iter(|| async {
            let command = SimpleCommand;
            let stream_id = StreamId::try_new("test-stream").unwrap();

            let input = SimpleInput {
                target_stream: stream_id,
                value: black_box(42),
            };

            let metadata = EventMetadata::new();

            black_box(executor.execute(&command, input, metadata).await.unwrap())
        });
    });
}

criterion_group!(benches, bench_simple_command);
criterion_main!(benches);
