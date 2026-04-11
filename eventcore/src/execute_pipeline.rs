use std::collections::{HashMap, HashSet, VecDeque};

use eventcore_types::{
    CommandError, CommandLogic, EventStoreError, HandleDecision, StreamId, StreamVersion,
    StreamWrites,
};

use crate::effects::{StoreEffect, StoreEffectResult};
use crate::{ExecutionResponse, RetryPolicy};

/// A step yielded by the `ExecutePipeline` state machine.
pub(crate) enum PipelineStep<C: CommandLogic> {
    /// The pipeline needs an I/O effect dispatched before it can continue.
    Yield(StoreEffect),
    /// The pipeline has completed with a final outcome.
    Done(PipelineOutcome<C>),
}

impl<C: CommandLogic> std::fmt::Debug for PipelineStep<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yield(effect) => f.debug_tuple("Yield").field(effect).finish(),
            Self::Done(outcome) => f.debug_tuple("Done").field(outcome).finish(),
        }
    }
}

/// The final outcome of the pipeline.
pub(crate) enum PipelineOutcome<C: CommandLogic> {
    /// Command completed successfully.
    Success(ExecutionResponse),
    /// Command failed with an error.
    Error(CommandError),
    /// Command requested a caller-driven effect.
    Effect { effect: C::Effect, state: C::State },
}

impl<C: CommandLogic> std::fmt::Debug for PipelineOutcome<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success(r) => f.debug_tuple("Success").field(r).finish(),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
            Self::Effect { effect, .. } => f
                .debug_struct("Effect")
                .field("effect", effect)
                .finish_non_exhaustive(),
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
    /// When Some, the Handling phase calls resume() instead of handle().
    pending_resume: Option<(C::State, C::EffectResult)>,
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
    /// All streams read; call handle() (or resume()) and build writes.
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
            pending_resume: None,
        }
    }

    /// Create a pipeline that resumes from an effect.
    ///
    /// Instead of calling `handle()`, this pipeline will call `resume()`
    /// with the provided state and effect result after re-reading streams.
    pub(crate) fn new_resume(
        command: C,
        policy: RetryPolicy,
        state: C::State,
        effect_result: C::EffectResult,
    ) -> Self {
        Self {
            command,
            policy,
            phase: Phase::Init,
            attempt: 0,
            pending_resume: Some((state, effect_result)),
        }
    }

    /// Advance the pipeline by one step.
    ///
    /// Call this in a loop. When it returns `PipelineStep::Yield(effect)`,
    /// dispatch the effect and call `resume(result)`. When it returns
    /// `PipelineStep::Done(outcome)`, the pipeline is finished.
    pub(crate) fn step(&mut self) -> PipelineStep<C> {
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

            Phase::Handling {
                stream_ids,
                expected_versions,
                state,
            } => {
                // If we have a pending resume, call resume() instead of handle()
                let decision =
                    if let Some((resume_state, effect_result)) = self.pending_resume.take() {
                        self.command.resume(resume_state, effect_result)
                    } else {
                        self.command.handle(state.clone())
                    };
                match decision {
                    HandleDecision::Done(Ok(events)) => {
                        match build_stream_writes::<C>(Vec::from(events), expected_versions) {
                            Ok(writes) => {
                                self.phase = Phase::AwaitingAppend { stream_ids };
                                PipelineStep::Yield(StoreEffect::AppendEvents { writes })
                            }
                            Err(e) => PipelineStep::Done(PipelineOutcome::Error(e)),
                        }
                    }
                    HandleDecision::Done(Err(e)) => PipelineStep::Done(PipelineOutcome::Error(e)),
                    HandleDecision::Effect(effect) => {
                        PipelineStep::Done(PipelineOutcome::Effect { effect, state })
                    }
                }
            }

            Phase::AwaitingStreamRead { .. }
            | Phase::AwaitingAppend { .. }
            | Phase::AwaitingRetrySleep => {
                panic!("step() called while awaiting a result — call resume() instead")
            }

            Phase::Done => {
                panic!("step() called on a completed pipeline")
            }
        }
    }

    /// Feed the result of a dispatched effect back into the pipeline.
    pub(crate) fn resume(&mut self, result: StoreEffectResult<C::Event>) -> PipelineStep<C> {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::AwaitingStreamRead {
                current_stream,
                queue,
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
                            // Need to add back to queue — but queue was moved.
                            // We need a mutable queue here.
                            let mut queue = queue;
                            queue.push_back(related_stream);
                            self.phase = Phase::ReadingStreams {
                                queue,
                                visited,
                                scheduled,
                                stream_ids,
                                expected_versions,
                                state,
                            };
                            return self.step();
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
                        let attempts = std::num::NonZeroU32::new(self.attempt + 1)
                            .expect("attempts are 1-based");
                        PipelineStep::Done(PipelineOutcome::Success(ExecutionResponse::new(
                            attempts,
                        )))
                    }
                    Err(EventStoreError::VersionConflict)
                        if self.attempt < self.policy.max_retries.into() =>
                    {
                        let delay = crate::compute_retry_delay_ms(
                            &self.policy.backoff_strategy,
                            self.attempt,
                        );
                        let duration = std::time::Duration::from_millis(delay.into());

                        // Log and invoke metrics hook
                        let attempt_number = self.attempt + 1;
                        tracing::warn!(
                            attempt = attempt_number,
                            delay_ms = delay.into_inner(),
                            streams = ?stream_ids.as_slice(),
                            "retrying command after concurrency conflict"
                        );

                        if let Some(hook) = &self.policy.metrics_hook {
                            let attempt_number_domain = eventcore_types::AttemptNumber::new(
                                std::num::NonZeroU32::new(attempt_number)
                                    .expect("attempt_number is always > 0"),
                            );
                            let ctx = crate::RetryContext {
                                streams: stream_ids.to_vec(),
                                attempt: attempt_number_domain,
                                delay_ms: delay,
                            };
                            hook.on_retry_attempt(&ctx);
                        }

                        self.attempt += 1;
                        self.phase = Phase::AwaitingRetrySleep;
                        PipelineStep::Yield(StoreEffect::Sleep { duration })
                    }
                    Err(EventStoreError::VersionConflict) => {
                        tracing::error!(
                            max_retries = self.policy.max_retries.into_inner(),
                            streams = ?stream_ids.as_slice()
                        );
                        PipelineStep::Done(PipelineOutcome::Error(CommandError::ConcurrencyError(
                            self.policy.max_retries.into(),
                        )))
                    }
                    Err(other) => PipelineStep::Done(PipelineOutcome::Error(
                        CommandError::EventStoreError(other),
                    )),
                }
            }

            Phase::AwaitingRetrySleep => match result {
                StoreEffectResult::Slept => {
                    // Restart from Init for the next attempt
                    self.phase = Phase::Init;
                    self.step()
                }
                _ => panic!("expected Slept result"),
            },

            _ => panic!("resume() called in wrong phase"),
        }
    }

    /// Consume the pipeline and return the command and policy for effect handling.
    pub(crate) fn into_parts(self) -> (C, RetryPolicy) {
        (self.command, self.policy)
    }
}

fn build_stream_writes<C: CommandLogic>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_types::{CommandStreams, Event, EventStreamReader, StreamDeclarations};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestEvent {
        stream_id: StreamId,
        value: i32,
    }

    impl Event for TestEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
        fn event_type_name() -> &'static str {
            "TestEvent"
        }
    }

    #[derive(Default, Clone)]
    struct TestState {
        total: i32,
    }

    struct TestCommand {
        stream_id: StreamId,
        value: i32,
    }

    impl CommandStreams for TestCommand {
        fn stream_declarations(&self) -> StreamDeclarations {
            StreamDeclarations::single(self.stream_id.clone())
        }
    }

    impl CommandLogic for TestCommand {
        type Event = TestEvent;
        type State = TestState;
        type Effect = ();
        type EffectResult = ();

        fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State {
            state.total += event.value;
            state
        }

        fn handle(&self, _state: Self::State) -> HandleDecision<Self> {
            HandleDecision::Done(Ok(vec![TestEvent {
                stream_id: self.stream_id.clone(),
                value: self.value,
            }]
            .into()))
        }
    }

    fn test_stream_id() -> StreamId {
        StreamId::try_new("test-stream").expect("valid")
    }

    #[test]
    fn pipeline_yields_read_stream_then_append_then_success() {
        // Given: a pipeline for a single-stream command
        let stream_id = test_stream_id();
        let command = TestCommand {
            stream_id: stream_id.clone(),
            value: 42,
        };
        let mut pipeline = ExecutePipeline::new(command, RetryPolicy::new());

        // When: first step
        let step = pipeline.step();

        // Then: it yields ReadStream for the declared stream
        let read_stream_id = match &step {
            PipelineStep::Yield(StoreEffect::ReadStream { stream_id }) => stream_id.clone(),
            other => panic!("expected ReadStream, got {other:?}"),
        };
        assert_eq!(read_stream_id, stream_id);

        // When: feed empty stream read result
        let empty_reader = EventStreamReader::new(vec![]);
        let step = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader)));

        // Then: it yields AppendEvents
        match &step {
            PipelineStep::Yield(StoreEffect::AppendEvents { .. }) => {}
            other => panic!("expected AppendEvents, got {other:?}"),
        }

        // When: feed successful append result
        let step = pipeline.resume(StoreEffectResult::EventsAppended(Ok(
            eventcore_types::EventStreamSlice,
        )));

        // Then: pipeline completes with success
        match step {
            PipelineStep::Done(PipelineOutcome::Success(response)) => {
                assert_eq!(response.attempts(), 1);
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn pipeline_retries_on_version_conflict() {
        // Given: a pipeline with retries enabled
        let stream_id = test_stream_id();
        let command = TestCommand {
            stream_id: stream_id.clone(),
            value: 10,
        };
        let mut pipeline = ExecutePipeline::new(command, RetryPolicy::new());

        // First attempt: read -> handle -> append -> conflict
        let _read = pipeline.step(); // ReadStream
        let empty_reader = EventStreamReader::new(vec![]);
        let _append = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader)));
        let step = pipeline.resume(StoreEffectResult::EventsAppended(Err(
            EventStoreError::VersionConflict,
        )));

        // Then: yields Sleep for retry backoff
        match &step {
            PipelineStep::Yield(StoreEffect::Sleep { .. }) => {}
            other => panic!("expected Sleep, got {other:?}"),
        }

        // When: sleep completes, pipeline restarts
        let step = pipeline.resume(StoreEffectResult::Slept);

        // Then: yields ReadStream again (second attempt)
        match &step {
            PipelineStep::Yield(StoreEffect::ReadStream { .. }) => {}
            other => panic!("expected ReadStream on retry, got {other:?}"),
        }

        // Complete second attempt successfully
        let empty_reader = EventStreamReader::new(vec![]);
        let _append = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader)));
        let step = pipeline.resume(StoreEffectResult::EventsAppended(Ok(
            eventcore_types::EventStreamSlice,
        )));

        match step {
            PipelineStep::Done(PipelineOutcome::Success(response)) => {
                assert_eq!(response.attempts(), 2);
            }
            other => panic!("expected Success on retry, got {other:?}"),
        }
    }

    #[test]
    fn pipeline_returns_error_on_business_rule_violation() {
        // Given: a command that always fails
        struct FailingCommand {
            stream_id: StreamId,
        }

        impl CommandStreams for FailingCommand {
            fn stream_declarations(&self) -> StreamDeclarations {
                StreamDeclarations::single(self.stream_id.clone())
            }
        }

        impl CommandLogic for FailingCommand {
            type Event = TestEvent;
            type State = ();
            type Effect = ();
            type EffectResult = ();

            fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
                state
            }

            fn handle(&self, _state: Self::State) -> HandleDecision<Self> {
                HandleDecision::Done(Err(CommandError::BusinessRuleViolation(
                    "always fails".into(),
                )))
            }
        }

        let mut pipeline = ExecutePipeline::new(
            FailingCommand {
                stream_id: test_stream_id(),
            },
            RetryPolicy::new(),
        );

        // Read stream
        let _read = pipeline.step();
        let empty_reader = EventStreamReader::new(vec![]);
        let step = pipeline.resume(StoreEffectResult::StreamRead(Ok(empty_reader)));

        // Then: pipeline returns error
        match step {
            PipelineStep::Done(PipelineOutcome::Error(CommandError::BusinessRuleViolation(
                msg,
            ))) => {
                assert_eq!(msg, "always fails");
            }
            other => panic!("expected BusinessRuleViolation, got {other:?}"),
        }
    }
}
