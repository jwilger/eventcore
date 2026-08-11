use super::*;
use eventcore_types::{CommandStreams, Event, NewEvents, StreamDeclarations};
use serde::{Deserialize, Serialize};

// --- Test fixtures ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
    fn event_type_name() -> &'static str {
        "TestEvent"
    }
}

struct SuccessCommand {
    stream_id: StreamId,
}

impl CommandStreams for SuccessCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.stream_id.clone()]).expect("single stream")
    }
}

impl CommandLogic for SuccessCommand {
    type Event = TestEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![TestEvent {
            stream_id: self.stream_id.clone(),
        }]
        .into())
    }
}

fn test_stream_id() -> StreamId {
    StreamId::try_new("test/stream-1").expect("valid stream id")
}

/// Drive an empty stream through the pipeline: resume immediately with
/// `StreamEnded` (zero `StreamEvent` pushes), mirroring the shell pumping
/// an empty stream.
fn resume_empty_stream(
    pipeline: &mut ExecutePipeline<impl CommandLogic<Event = TestEvent>>,
) -> PipelineStep {
    pipeline.resume(StoreEffectResult::StreamEnded)
}

// --- Tests ---

#[test]
fn pipeline_yields_read_stream_then_append_then_success() {
    let stream_id = test_stream_id();
    let command = SuccessCommand {
        stream_id: stream_id.clone(),
    };
    let mut pipeline = ExecutePipeline::new(command, RetryPolicy::default());

    // Step 1: should yield ReadStream
    let step = pipeline.step();
    assert!(
        matches!(&step, PipelineStep::Yield(StoreEffect::ReadStream { stream_id: sid }) if *sid == stream_id)
    );

    // Resume with empty stream
    let step = resume_empty_stream(&mut pipeline);

    // Step 2: should yield AppendEvents
    assert!(matches!(
        &step,
        PipelineStep::Yield(StoreEffect::AppendEvents { .. })
    ));

    // Resume with successful append
    let step = pipeline.resume(StoreEffectResult::EventsAppended(Ok(
        eventcore_types::EventStreamSlice,
    )));

    // Should be done with success
    assert!(matches!(
        step,
        PipelineStep::Done(PipelineOutcome::Success(_))
    ));
}

#[test]
fn pipeline_retries_on_version_conflict() {
    let stream_id = test_stream_id();
    let command = SuccessCommand {
        stream_id: stream_id.clone(),
    };
    let mut pipeline = ExecutePipeline::new(command, RetryPolicy::default());

    // First attempt: read → append → conflict
    let _read = pipeline.step();
    let _append = resume_empty_stream(&mut pipeline);
    let step = pipeline.resume(StoreEffectResult::EventsAppended(Err(
        EventStoreError::VersionConflict {
            stream_id: StreamId::try_new("test-conflict").expect("valid stream id"),
            expected: StreamVersion::new(0),
            actual: StreamVersion::new(1),
        },
    )));

    // Should yield Sleep for retry backoff
    assert!(matches!(
        step,
        PipelineStep::Yield(StoreEffect::Sleep { .. })
    ));

    // Resume after sleep — should restart from ReadStream
    let step = pipeline.resume(StoreEffectResult::Slept);
    assert!(
        matches!(&step, PipelineStep::Yield(StoreEffect::ReadStream { stream_id: sid }) if *sid == stream_id)
    );

    // Complete second attempt successfully
    let _append = resume_empty_stream(&mut pipeline);
    let step = pipeline.resume(StoreEffectResult::EventsAppended(Ok(
        eventcore_types::EventStreamSlice,
    )));

    assert!(matches!(
        step,
        PipelineStep::Done(PipelineOutcome::Success(_))
    ));
}

#[test]
fn pipeline_returns_error_on_business_rule_violation() {
    let stream_id = test_stream_id();

    struct FailingCommand {
        stream_id: StreamId,
    }

    impl CommandStreams for FailingCommand {
        fn stream_declarations(&self) -> StreamDeclarations {
            StreamDeclarations::try_from_streams(vec![self.stream_id.clone()])
                .expect("single stream")
        }
    }

    impl CommandLogic for FailingCommand {
        type Event = TestEvent;
        type State = ();

        fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
            state
        }

        fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
            Err(CommandError::from("test-violation"))
        }
    }

    let command = FailingCommand {
        stream_id: stream_id.clone(),
    };
    let mut pipeline = ExecutePipeline::new(command, RetryPolicy::default());

    // Read stream
    let _read = pipeline.step();
    let step = resume_empty_stream(&mut pipeline);

    // Should be done with error (no append attempt)
    assert!(matches!(
        step,
        PipelineStep::Done(PipelineOutcome::Error(CommandError::BusinessRuleViolation(
            _
        )))
    ));
}

/// Command whose state counts how many events were folded, so a test can
/// prove the pipeline folds each pushed `StreamEvent` incrementally.
struct CountingCommand {
    stream_id: StreamId,
    observed: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl CommandStreams for CountingCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.stream_id.clone()]).expect("single stream")
    }
}

impl CommandLogic for CountingCommand {
    type Event = TestEvent;
    type State = usize;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state + 1
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        *self.observed.lock().expect("lock") = state;
        Ok(vec![TestEvent {
            stream_id: self.stream_id.clone(),
        }]
        .into())
    }
}

#[test]
fn pipeline_folds_each_streamed_event_incrementally() {
    let stream_id = test_stream_id();
    let observed = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let command = CountingCommand {
        stream_id: stream_id.clone(),
        observed: observed.clone(),
    };
    let mut pipeline = ExecutePipeline::new(command, RetryPolicy::default());

    // Yields ReadStream
    let _read = pipeline.step();

    // Push three events one at a time; each should fold and wait for more.
    for _ in 0..3 {
        let step = pipeline.resume(StoreEffectResult::StreamEvent(TestEvent {
            stream_id: stream_id.clone(),
        }));
        assert!(matches!(step, PipelineStep::WaitForResult));
    }

    // End of stream → handle() runs with the folded state, then append.
    let step = pipeline.resume(StoreEffectResult::StreamEnded);
    assert!(matches!(
        &step,
        PipelineStep::Yield(StoreEffect::AppendEvents { .. })
    ));

    // handle() observed all three folded events.
    assert_eq!(*observed.lock().expect("lock"), 3);
}

#[test]
fn pipeline_propagates_per_event_stream_error() {
    let stream_id = test_stream_id();
    let command = SuccessCommand {
        stream_id: stream_id.clone(),
    };
    let mut pipeline = ExecutePipeline::new(command, RetryPolicy::default());

    let _read = pipeline.step();

    // First event folds, then a per-event decode error terminates the read.
    let step = pipeline.resume(StoreEffectResult::StreamEvent(TestEvent {
        stream_id: stream_id.clone(),
    }));
    assert!(matches!(step, PipelineStep::WaitForResult));

    let step = pipeline.resume(StoreEffectResult::StreamReadError(
        EventStoreError::DeserializationFailed {
            stream_id,
            detail: "bad event".to_string(),
        },
    ));

    assert!(matches!(
        step,
        PipelineStep::Done(PipelineOutcome::Error(CommandError::EventStoreError(_)))
    ));
}
