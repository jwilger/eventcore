use std::time::Duration;

use eventcore_types::{
    BatchSize, Event, EventFilter, EventPage, FailureContext, FailureStrategy, Projector,
    RetryCount, StreamPosition,
};

use crate::projection::{EventRetryConfig, PollConfig, ProjectionError};

/// Effects yielded by the projection pipeline state machine.
#[derive(Debug)]
pub(crate) enum ProjectionEffect {
    /// Read events from the store.
    ReadEvents {
        filter: EventFilter,
        page: EventPage,
    },
    /// Load the last checkpoint.
    LoadCheckpoint { name: String },
    /// Save a checkpoint at the given position.
    SaveCheckpoint {
        name: String,
        position: StreamPosition,
    },
    /// Sleep for a given duration.
    Sleep { duration: Duration },
}

/// Results from dispatching a `ProjectionEffect`.
pub(crate) enum ProjectionEffectResult<E: Event> {
    /// Events read from the store.
    EventsRead(Result<Vec<(E, StreamPosition)>, String>),
    /// Checkpoint loaded.
    CheckpointLoaded(Result<Option<StreamPosition>, String>),
    /// Checkpoint saved.
    CheckpointSaved(Result<(), String>),
    /// Sleep completed.
    Slept,
}

/// Step yielded by the projection pipeline.
#[derive(Debug)]
pub(crate) enum ProjectionStep {
    /// Pipeline needs an effect dispatched.
    Yield(ProjectionEffect),
    /// Pipeline has completed.
    Done(Result<(), ProjectionError>),
}

/// Pure state machine for projection execution.
///
/// Encapsulates the projection polling loop, event processing, retry logic,
/// and checkpoint management. Yields `ProjectionEffect` values for all I/O.
pub(crate) struct ProjectionPipeline<P: Projector> {
    projector: P,
    has_checkpoint_store: bool,
    poll_config: PollConfig,
    event_retry_config: EventRetryConfig,
    phase: ProjectionPhase<P>,
}

enum ProjectionPhase<P: Projector> {
    /// Load the initial checkpoint.
    LoadCheckpoint,
    /// Waiting for checkpoint load result.
    AwaitingCheckpoint,
    /// Poll for events.
    PollEvents {
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
        consecutive_failures: u32,
    },
    /// Waiting for read events result.
    AwaitingEvents {
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
        consecutive_failures: u32,
    },
    /// Processing events one at a time.
    ProcessingEvents {
        events: Vec<(P::Event, StreamPosition)>,
        event_index: usize,
        retry_count: u32,
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
    },
    /// Waiting for checkpoint save after successful event.
    AwaitingCheckpointSave {
        events: Vec<(P::Event, StreamPosition)>,
        event_index: usize,
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
    },
    /// Waiting for checkpoint save after skip.
    AwaitingSkipCheckpointSave {
        events: Vec<(P::Event, StreamPosition)>,
        event_index: usize,
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
    },
    /// Waiting for retry sleep.
    AwaitingEventRetrySleep {
        events: Vec<(P::Event, StreamPosition)>,
        event_index: usize,
        retry_count: u32,
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
    },
    /// Waiting for poll failure backoff sleep.
    AwaitingPollFailureSleep {
        last_checkpoint: Option<StreamPosition>,
        ctx: P::Context,
        consecutive_failures: u32,
    },
    /// Terminal state.
    Done,
}

impl<P: Projector> ProjectionPipeline<P>
where
    P::Event: Event + Clone,
    P::Context: Default,
    P::Error: std::fmt::Debug,
{
    pub(crate) fn new(
        projector: P,
        has_checkpoint_store: bool,
        poll_config: PollConfig,
        event_retry_config: EventRetryConfig,
    ) -> Self {
        let phase = if has_checkpoint_store {
            ProjectionPhase::LoadCheckpoint
        } else {
            ProjectionPhase::PollEvents {
                last_checkpoint: None,
                ctx: P::Context::default(),
                consecutive_failures: 0,
            }
        };

        Self {
            projector,
            has_checkpoint_store,
            poll_config,
            event_retry_config,
            phase,
        }
    }

    /// Advance the pipeline by one step.
    ///
    /// Call this in a loop. When it returns `ProjectionStep::Yield(effect)`,
    /// dispatch the effect and call `resume(result)`. When it returns
    /// `ProjectionStep::Done(outcome)`, the pipeline is finished.
    pub(crate) fn step(&mut self) -> ProjectionStep {
        match std::mem::replace(&mut self.phase, ProjectionPhase::Done) {
            ProjectionPhase::LoadCheckpoint => {
                self.phase = ProjectionPhase::AwaitingCheckpoint;
                ProjectionStep::Yield(ProjectionEffect::LoadCheckpoint {
                    name: self.projector.name().to_string(),
                })
            }

            ProjectionPhase::PollEvents {
                last_checkpoint,
                ctx,
                consecutive_failures,
            } => {
                let filter = EventFilter::all();
                let page = match last_checkpoint {
                    Some(position) => EventPage::after(position, BatchSize::new(1000)),
                    None => EventPage::first(BatchSize::new(1000)),
                };

                self.phase = ProjectionPhase::AwaitingEvents {
                    last_checkpoint,
                    ctx,
                    consecutive_failures,
                };
                ProjectionStep::Yield(ProjectionEffect::ReadEvents { filter, page })
            }

            ProjectionPhase::ProcessingEvents {
                events,
                event_index,
                retry_count,
                mut last_checkpoint,
                mut ctx,
            } => {
                if event_index >= events.len() {
                    // All events processed — batch mode always completes
                    return ProjectionStep::Done(Ok(()));
                }

                let (ref event, position) = events[event_index];

                match self.projector.apply(event.clone(), position, &mut ctx) {
                    Ok(()) => {
                        last_checkpoint = Some(position);
                        if self.has_checkpoint_store {
                            self.phase = ProjectionPhase::AwaitingCheckpointSave {
                                events,
                                event_index,
                                last_checkpoint,
                                ctx,
                            };
                            ProjectionStep::Yield(ProjectionEffect::SaveCheckpoint {
                                name: self.projector.name().to_string(),
                                position,
                            })
                        } else {
                            // No checkpoint store — move to next event
                            self.phase = ProjectionPhase::ProcessingEvents {
                                events,
                                event_index: event_index + 1,
                                retry_count: 0,
                                last_checkpoint,
                                ctx,
                            };
                            self.step()
                        }
                    }
                    Err(error) => {
                        let failure_ctx = FailureContext {
                            error: &error,
                            position,
                            retry_count: RetryCount::new(retry_count),
                        };
                        let strategy = self.projector.on_error(failure_ctx);

                        match strategy {
                            FailureStrategy::Fatal => ProjectionStep::Done(Err(
                                ProjectionError::Failed("projector apply failed".to_string()),
                            )),
                            FailureStrategy::Skip => {
                                tracing::warn!(
                                    projector = self.projector.name(),
                                    position = %position,
                                    error = ?error,
                                    "Skipping failed event"
                                );
                                last_checkpoint = Some(position);
                                if self.has_checkpoint_store {
                                    self.phase = ProjectionPhase::AwaitingSkipCheckpointSave {
                                        events,
                                        event_index,
                                        last_checkpoint,
                                        ctx,
                                    };
                                    ProjectionStep::Yield(ProjectionEffect::SaveCheckpoint {
                                        name: self.projector.name().to_string(),
                                        position,
                                    })
                                } else {
                                    self.phase = ProjectionPhase::ProcessingEvents {
                                        events,
                                        event_index: event_index + 1,
                                        retry_count: 0,
                                        last_checkpoint,
                                        ctx,
                                    };
                                    self.step()
                                }
                            }
                            FailureStrategy::Retry => {
                                if retry_count
                                    >= self.event_retry_config.max_retry_attempts.into_inner()
                                {
                                    return ProjectionStep::Done(Err(ProjectionError::Failed(
                                        "projector apply failed after max retries".to_string(),
                                    )));
                                }

                                let new_retry_count = retry_count + 1;
                                let base_delay_ms =
                                    self.event_retry_config.retry_delay.as_millis() as f64;
                                let multiplier = self
                                    .event_retry_config
                                    .retry_backoff_multiplier
                                    .into_inner();
                                let delay_ms =
                                    base_delay_ms * multiplier.powi(new_retry_count as i32 - 1);
                                let delay = Duration::from_millis(delay_ms as u64);
                                let capped_delay =
                                    delay.min(self.event_retry_config.max_retry_delay);

                                self.phase = ProjectionPhase::AwaitingEventRetrySleep {
                                    events,
                                    event_index,
                                    retry_count: new_retry_count,
                                    last_checkpoint,
                                    ctx,
                                };
                                ProjectionStep::Yield(ProjectionEffect::Sleep {
                                    duration: capped_delay,
                                })
                            }
                        }
                    }
                }
            }

            ProjectionPhase::AwaitingCheckpoint
            | ProjectionPhase::AwaitingEvents { .. }
            | ProjectionPhase::AwaitingCheckpointSave { .. }
            | ProjectionPhase::AwaitingSkipCheckpointSave { .. }
            | ProjectionPhase::AwaitingEventRetrySleep { .. }
            | ProjectionPhase::AwaitingPollFailureSleep { .. } => {
                panic!("step() called while awaiting a result — call resume() instead")
            }

            ProjectionPhase::Done => {
                panic!("step() called on completed pipeline")
            }
        }
    }

    /// Resume the pipeline after an effect has been dispatched.
    pub(crate) fn resume(&mut self, result: ProjectionEffectResult<P::Event>) -> ProjectionStep {
        match std::mem::replace(&mut self.phase, ProjectionPhase::Done) {
            ProjectionPhase::AwaitingCheckpoint => {
                let checkpoint = match result {
                    ProjectionEffectResult::CheckpointLoaded(Ok(cp)) => cp,
                    ProjectionEffectResult::CheckpointLoaded(Err(e)) => {
                        return ProjectionStep::Done(Err(ProjectionError::Failed(format!(
                            "failed to load checkpoint for projector '{}': {}",
                            self.projector.name(),
                            e
                        ))));
                    }
                    _ => panic!("expected CheckpointLoaded result"),
                };

                self.phase = ProjectionPhase::PollEvents {
                    last_checkpoint: checkpoint,
                    ctx: P::Context::default(),
                    consecutive_failures: 0,
                };
                self.step()
            }

            ProjectionPhase::AwaitingEvents {
                last_checkpoint,
                ctx,
                mut consecutive_failures,
            } => {
                let events = match result {
                    ProjectionEffectResult::EventsRead(Ok(events)) => events,
                    ProjectionEffectResult::EventsRead(Err(e)) => {
                        let max_failures: std::num::NonZeroU32 =
                            self.poll_config.max_consecutive_poll_failures.into();

                        tracing::warn!(
                            projector = self.projector.name(),
                            error = %e,
                            consecutive_failures = consecutive_failures + 1,
                            max_failures = max_failures.get(),
                            "Poll failure reading events"
                        );

                        if consecutive_failures >= max_failures.get() {
                            return ProjectionStep::Done(Err(ProjectionError::Failed(format!(
                                "failed to read events after max retries: {}",
                                e
                            ))));
                        }

                        consecutive_failures += 1;
                        self.phase = ProjectionPhase::AwaitingPollFailureSleep {
                            last_checkpoint,
                            ctx,
                            consecutive_failures,
                        };
                        return ProjectionStep::Yield(ProjectionEffect::Sleep {
                            duration: self.poll_config.poll_failure_backoff,
                        });
                    }
                    _ => panic!("expected EventsRead result"),
                };

                self.phase = ProjectionPhase::ProcessingEvents {
                    events,
                    event_index: 0,
                    retry_count: 0,
                    last_checkpoint,
                    ctx,
                };
                self.step()
            }

            ProjectionPhase::AwaitingCheckpointSave {
                events,
                event_index,
                last_checkpoint,
                ctx,
            } => {
                // Log checkpoint save errors but don't stop processing
                if let (ProjectionEffectResult::CheckpointSaved(Err(e)), Some(pos)) =
                    (&result, last_checkpoint)
                {
                    tracing::warn!(
                        projector = self.projector.name(),
                        position = %pos,
                        error = %e,
                        "Failed to save checkpoint after successful event processing"
                    );
                }

                // Move to next event
                self.phase = ProjectionPhase::ProcessingEvents {
                    events,
                    event_index: event_index + 1,
                    retry_count: 0,
                    last_checkpoint,
                    ctx,
                };
                self.step()
            }

            ProjectionPhase::AwaitingSkipCheckpointSave {
                events,
                event_index,
                last_checkpoint,
                ctx,
            } => {
                // Log checkpoint save errors but don't stop processing
                if let (ProjectionEffectResult::CheckpointSaved(Err(e)), Some(pos)) =
                    (&result, last_checkpoint)
                {
                    tracing::warn!(
                        projector = self.projector.name(),
                        position = %pos,
                        error = %e,
                        "Failed to save checkpoint after skipping event"
                    );
                }

                // Move to next event
                self.phase = ProjectionPhase::ProcessingEvents {
                    events,
                    event_index: event_index + 1,
                    retry_count: 0,
                    last_checkpoint,
                    ctx,
                };
                self.step()
            }

            ProjectionPhase::AwaitingEventRetrySleep {
                events,
                event_index,
                retry_count,
                last_checkpoint,
                ctx,
            } => {
                // Sleep done — retry the event
                self.phase = ProjectionPhase::ProcessingEvents {
                    events,
                    event_index,
                    retry_count,
                    last_checkpoint,
                    ctx,
                };
                self.step()
            }

            ProjectionPhase::AwaitingPollFailureSleep {
                last_checkpoint,
                ctx,
                consecutive_failures,
            } => {
                // Sleep done — retry poll
                self.phase = ProjectionPhase::PollEvents {
                    last_checkpoint,
                    ctx,
                    consecutive_failures,
                };
                self.step()
            }

            _ => panic!("resume() called in wrong phase"),
        }
    }
}
