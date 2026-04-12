use std::collections::{HashMap, HashSet, VecDeque};

use eventcore_types::{CommandError, CommandLogic, EventStoreError, StreamId, StreamVersion};

use crate::effects::{StoreEffect, StoreEffectResult};
use crate::{ExecutionResponse, RetryPolicy};

/// A step yielded by the `ExecutePipeline` state machine.
pub(crate) enum PipelineStep {
    /// The pipeline needs an I/O effect dispatched before it can continue.
    Yield(StoreEffect),
    /// The pipeline has completed with a final outcome.
    Done(PipelineOutcome),
}

impl std::fmt::Debug for PipelineStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yield(effect) => f.debug_tuple("Yield").field(effect).finish(),
            Self::Done(outcome) => f.debug_tuple("Done").field(outcome).finish(),
        }
    }
}

/// The final outcome of the pipeline.
pub(crate) enum PipelineOutcome {
    /// Command completed successfully.
    Success(ExecutionResponse),
    /// Command failed with an error.
    Error(CommandError),
}

impl std::fmt::Debug for PipelineOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success(r) => f.debug_tuple("Success").field(r).finish(),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
        }
    }
}

/// Pure state machine for command execution.
///
/// This state machine encapsulates the entire execution pipeline:
/// 1. Stream resolution (BFS discovery)
/// 2. Reading each stream
/// 3. State reconstruction via `apply()`
/// 4. Business logic via `handle()`
/// 5. Building `StreamWrites`
/// 6. Appending events
/// 7. Retry on version conflict
///
/// It yields `StoreEffect` values and accepts `StoreEffectResult` values,
/// never performing I/O itself.
pub(crate) struct ExecutePipeline<C: CommandLogic> {
    command: C,
    policy: RetryPolicy,
    phase: Phase<C>,
    attempt: u32,
}

enum Phase<C: CommandLogic> {
    /// Initial phase: enqueue declared streams for reading.
    Init,
    /// Reading streams one at a time for state reconstruction.
    ReadingStreams {
        queue: VecDeque<StreamId>,
        visited: HashSet<StreamId>,
        scheduled: HashSet<StreamId>,
        stream_ids: Vec<StreamId>,
        expected_versions: HashMap<StreamId, StreamVersion>,
        state: C::State,
    },
    /// Waiting for a stream read result.
    AwaitingStreamRead {
        current_stream: StreamId,
        queue: VecDeque<StreamId>,
        visited: HashSet<StreamId>,
        scheduled: HashSet<StreamId>,
        stream_ids: Vec<StreamId>,
        expected_versions: HashMap<StreamId, StreamVersion>,
        state: C::State,
    },
    /// All streams read; call handle() and build writes.
    Handling {
        stream_ids: Vec<StreamId>,
        expected_versions: HashMap<StreamId, StreamVersion>,
        state: C::State,
    },
    /// Waiting for append result.
    AwaitingAppend { stream_ids: Vec<StreamId> },
    /// Waiting for retry sleep to complete.
    AwaitingRetrySleep,
    /// Terminal state — pipeline has produced its outcome.
    Done,
}

impl<C: CommandLogic> ExecutePipeline<C> {
    pub(crate) fn new(command: C, policy: RetryPolicy) -> Self {
        Self {
            command,
            policy,
            phase: Phase::Init,
            attempt: 0,
        }
    }

    /// Advance the pipeline by one step.
    ///
    /// Call this in a loop. When it returns `PipelineStep::Yield(effect)`,
    /// dispatch the effect and call `resume(result)`. When it returns
    /// `PipelineStep::Done(outcome)`, the pipeline is finished.
    pub(crate) fn step(&mut self) -> PipelineStep {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Init => {
                let declared_streams = self.command.stream_declarations();
                let mut scheduled: HashSet<StreamId> =
                    HashSet::with_capacity(declared_streams.len());
                let mut queue: VecDeque<StreamId> = VecDeque::with_capacity(declared_streams.len());

                for stream_id in declared_streams.iter() {
                    let stream_id = stream_id.clone();
                    if scheduled.insert(stream_id.clone()) {
                        queue.push_back(stream_id);
                    }
                }

                self.phase = Phase::ReadingStreams {
                    queue,
                    visited: HashSet::new(),
                    scheduled,
                    stream_ids: Vec::new(),
                    expected_versions: HashMap::new(),
                    state: C::State::default(),
                };
                self.step()
            }

            Phase::ReadingStreams {
                mut queue,
                mut visited,
                scheduled,
                stream_ids,
                expected_versions,
                state,
            } => {
                // Find next unvisited stream
                while let Some(stream_id) = queue.pop_front() {
                    if visited.insert(stream_id.clone()) {
                        self.phase = Phase::AwaitingStreamRead {
                            current_stream: stream_id.clone(),
                            queue,
                            visited,
                            scheduled,
                            stream_ids,
                            expected_versions,
                            state,
                        };
                        return PipelineStep::Yield(StoreEffect::ReadStream { stream_id });
                    }
                }

                // All streams read — proceed to handling
                self.phase = Phase::Handling {
                    stream_ids,
                    expected_versions,
                    state,
                };
                self.step()
            }

            Phase::AwaitingStreamRead { .. } => {
                panic!("step() called while awaiting a result — call resume() instead")
            }

            Phase::Handling {
                stream_ids,
                expected_versions,
                state,
            } => {
                match self.command.handle(state) {
                    Ok(events) => {
                        // Build stream writes from events
                        match crate::build_stream_writes_from_events::<C>(
                            Vec::from(events),
                            expected_versions,
                        ) {
                            Ok(writes) => {
                                self.phase = Phase::AwaitingAppend { stream_ids };
                                PipelineStep::Yield(StoreEffect::AppendEvents { writes })
                            }
                            Err(e) => PipelineStep::Done(PipelineOutcome::Error(e)),
                        }
                    }
                    Err(e) => PipelineStep::Done(PipelineOutcome::Error(e)),
                }
            }

            Phase::AwaitingAppend { .. } => {
                panic!("step() called while awaiting a result — call resume() instead")
            }

            Phase::AwaitingRetrySleep => {
                panic!("step() called while awaiting a result — call resume() instead")
            }

            Phase::Done => {
                panic!("step() called on a completed pipeline")
            }
        }
    }

    /// Resume the pipeline after an effect has been dispatched.
    pub(crate) fn resume(&mut self, result: StoreEffectResult<C::Event>) -> PipelineStep {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::AwaitingStreamRead {
                current_stream,
                mut queue,
                visited,
                mut scheduled,
                mut stream_ids,
                mut expected_versions,
                mut state,
            } => {
                let reader = match result {
                    StoreEffectResult::StreamRead(Ok(reader)) => reader,
                    StoreEffectResult::StreamRead(Err(e)) => {
                        return PipelineStep::Done(PipelineOutcome::Error(
                            CommandError::EventStoreError(e),
                        ));
                    }
                    _ => panic!("expected StreamRead result"),
                };

                let expected_version = StreamVersion::new(reader.len());
                let _ = expected_versions.insert(current_stream.clone(), expected_version);
                state = reader
                    .into_iter()
                    .fold(state, |acc, event| self.command.apply(acc, &event));
                stream_ids.push(current_stream);

                // Dynamic stream discovery
                if let Some(resolver) = self.command.stream_resolver() {
                    for related_stream in resolver.discover_related_streams(&state) {
                        if scheduled.insert(related_stream.clone()) {
                            queue.push_back(related_stream);
                        }
                    }
                }

                self.phase = Phase::ReadingStreams {
                    queue,
                    visited,
                    scheduled,
                    stream_ids,
                    expected_versions,
                    state,
                };
                self.step()
            }

            Phase::AwaitingAppend { stream_ids } => {
                let append_result = match result {
                    StoreEffectResult::EventsAppended(r) => r,
                    _ => panic!("expected EventsAppended result"),
                };

                match append_result {
                    Ok(_) => {
                        tracing::info!("command execution succeeded");
                        PipelineStep::Done(PipelineOutcome::Success(ExecutionResponse::new(
                            std::num::NonZeroU32::new(self.attempt + 1)
                                .expect("attempts are 1-based"),
                        )))
                    }
                    Err(EventStoreError::VersionConflict { .. }) => {
                        if self.attempt < self.policy.max_retries.into() {
                            let delay_ms = crate::compute_retry_delay_ms(
                                &self.policy.backoff_strategy,
                                self.attempt,
                            );
                            let attempt_number = self.attempt + 1;
                            let attempt_number_domain = eventcore_types::AttemptNumber::new(
                                std::num::NonZeroU32::new(attempt_number)
                                    .expect("attempt_number is always > 0"),
                            );

                            tracing::warn!(
                                attempt = attempt_number,
                                delay_ms = delay_ms.into_inner(),
                                streams = ?stream_ids.as_slice(),
                                "retrying command after concurrency conflict"
                            );

                            if let Some(hook) = &self.policy.metrics_hook {
                                let ctx = crate::RetryContext {
                                    streams: stream_ids,
                                    attempt: attempt_number_domain,
                                    delay_ms,
                                };
                                hook.on_retry_attempt(&ctx);
                            }

                            let duration = std::time::Duration::from_millis(delay_ms.into());
                            self.attempt += 1;
                            self.phase = Phase::AwaitingRetrySleep;
                            PipelineStep::Yield(StoreEffect::Sleep { duration })
                        } else {
                            tracing::error!(
                                max_retries = self.policy.max_retries.into_inner(),
                                streams = ?stream_ids.as_slice()
                            );
                            PipelineStep::Done(PipelineOutcome::Error(
                                CommandError::ConcurrencyError(self.policy.max_retries.into()),
                            ))
                        }
                    }
                    Err(other) => PipelineStep::Done(PipelineOutcome::Error(
                        CommandError::EventStoreError(other),
                    )),
                }
            }

            Phase::AwaitingRetrySleep => {
                match result {
                    StoreEffectResult::Slept => {}
                    _ => panic!("expected Slept result"),
                }
                // Restart from Init for the next attempt
                self.phase = Phase::Init;
                self.step()
            }

            _ => panic!("resume() called in wrong phase"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_types::{
        CommandStreams, Event, EventStreamReader, NewEvents, StreamDeclarations,
    };
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
            StreamDeclarations::try_from_streams(vec![self.stream_id.clone()])
                .expect("single stream")
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

    fn empty_reader() -> EventStreamReader<TestEvent> {
        EventStreamReader::new(vec![])
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
        let step = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader())));

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
        let _append = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader())));
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
        let _append = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader())));
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
        let step = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader())));

        // Should be done with error (no append attempt)
        assert!(matches!(
            step,
            PipelineStep::Done(PipelineOutcome::Error(CommandError::BusinessRuleViolation(
                _
            )))
        ));
    }
}
