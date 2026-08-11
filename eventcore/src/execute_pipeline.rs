use std::collections::{HashMap, HashSet, VecDeque};

use eventcore_types::{CommandError, CommandLogic, EventStoreError, StreamId, StreamVersion};

use crate::effects::{StoreEffect, StoreEffectResult};
use crate::{ExecutionResponse, RetryPolicy};

/// A step yielded by the `ExecutePipeline` state machine.
pub(crate) enum PipelineStep {
    /// The pipeline needs an I/O effect dispatched before it can continue.
    Yield(StoreEffect),
    /// The pipeline consumed a `resume()` result and is ready for the next one
    /// without dispatching a new effect. Used while a stream is being drained:
    /// after each `StreamEvent` is folded, the shell pulls the next item.
    WaitForResult,
    /// The pipeline has completed with a final outcome.
    Done(PipelineOutcome),
}

impl std::fmt::Debug for PipelineStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yield(effect) => f.debug_tuple("Yield").field(effect).finish(),
            Self::WaitForResult => f.write_str("WaitForResult"),
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
    /// Folding events from the current stream as they arrive.
    ///
    /// The shell pushes one `StreamEvent` at a time; each is folded into
    /// `state` and `event_count` is incremented. The phase is re-entered after
    /// every event and only advances when the stream terminates. `event_count`
    /// becomes the stream's expected version (equivalent to the old
    /// `reader.len()`).
    AwaitingStreamRead {
        current_stream: StreamId,
        queue: VecDeque<StreamId>,
        visited: HashSet<StreamId>,
        scheduled: HashSet<StreamId>,
        stream_ids: Vec<StreamId>,
        expected_versions: HashMap<StreamId, StreamVersion>,
        state: C::State,
        event_count: usize,
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
                            event_count: 0,
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
                mut event_count,
            } => {
                match result {
                    // Fold a single streamed event into state and wait for the
                    // next item. The whole stream is never buffered in the shell
                    // or the pipeline — only the in-progress `state` is kept.
                    StoreEffectResult::StreamEvent(event) => {
                        state = self.command.apply(state, &event);
                        event_count += 1;
                        self.phase = Phase::AwaitingStreamRead {
                            current_stream,
                            queue,
                            visited,
                            scheduled,
                            stream_ids,
                            expected_versions,
                            state,
                            event_count,
                        };
                        PipelineStep::WaitForResult
                    }

                    // Stream fully consumed: the number of folded events is the
                    // stream's expected version (the old `reader.len()`).
                    StoreEffectResult::StreamEnded => {
                        let expected_version = StreamVersion::new(event_count);
                        let _ = expected_versions.insert(current_stream.clone(), expected_version);
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

                    StoreEffectResult::StreamReadError(e) => {
                        PipelineStep::Done(PipelineOutcome::Error(CommandError::EventStoreError(e)))
                    }

                    _ => panic!("expected a stream read result"),
                }
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
#[path = "execute_pipeline.test.rs"]
mod tests;
