use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, NewEvents, RetryPolicy, StreamDeclarations,
    StreamId, StreamResolver,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};

#[tokio::test]
async fn execute_future_is_send() {
    // Given: A minimal command that implements StreamResolver (returning Some(self))
    //        and an in-memory event store, which together exercise the code path
    //        where &dyn StreamResolver is held across .await points.
    let store = InMemoryEventStore::new();
    let stream =
        StreamId::try_new("test/stream-1".to_string()).expect("valid stream id for test fixture");
    let command = SendCheckCommand { stream };

    // When: We obtain the future returned by execute() and assert it is Send.
    fn assert_send<T: Send>(_: &T) {}
    let future = eventcore::execute(store, command, RetryPolicy::new());
    assert_send(&future);

    // Then: The future satisfies the Send bound (compilation succeeds).
    //       We also await it to prevent unused-future warnings.
    let _result = future.await;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TestEvent {
    Happened { stream: StreamId },
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            TestEvent::Happened { stream } => stream,
        }
    }

    fn event_type_name() -> &'static str {
        "TestEvent"
    }
}

#[derive(Debug, Default, Clone)]
struct TestState;

struct SendCheckCommand {
    stream: StreamId,
}

impl CommandStreams for SendCheckCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.stream.clone()])
            .expect("send check command declares one stream")
    }
}

impl CommandLogic for SendCheckCommand {
    type Event = TestEvent;
    type State = TestState;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![TestEvent::Happened {
            stream: self.stream.clone(),
        }]
        .into())
    }

    fn stream_resolver(&self) -> Option<&(dyn StreamResolver<Self::State> + Sync)> {
        Some(self)
    }
}

impl StreamResolver<TestState> for SendCheckCommand {
    fn discover_related_streams(&self, _state: &TestState) -> Vec<StreamId> {
        vec![]
    }
}
