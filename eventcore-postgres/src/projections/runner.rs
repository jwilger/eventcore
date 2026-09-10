use eventcore_types::{
    AttemptNumber, DeliveryPosition, DeliveryUpperBound, PersistedEventEnvelope, ProjectionSource,
};
use std::future::Future;
use std::num::NonZeroU32;
use std::time::Duration;

use super::{
    AfterCommit, PostgresProjectionConfig, PostgresProjectionMode, PostgresProjectionStore,
    PostgresProjector, ProjectionFailureContext, ProjectionFailureDecision, ProjectionLeader,
    TransactionalProjectionError,
};

enum EnvelopeResult {
    Processed,
    Skipped,
    Suppressed,
    Stopped,
}

enum DrainCycleOutcome {
    CaughtUp(Option<DeliveryPosition>),
    Stopped(DeliveryPosition),
}

/// Observable result of a transactional projection run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionRunOutcome {
    /// Batch mode reached its captured source frontier.
    CaughtUp {
        /// Number of committed event effects.
        processed: u64,
        /// Number of explicitly skipped events.
        skipped: u64,
        /// Captured high-water mark, absent when the source was empty.
        through: Option<DeliveryPosition>,
    },
    /// The runner ended without committing the pending position.
    Stopped {
        /// Position left pending.
        position: DeliveryPosition,
        /// Number of committed event effects before stopping.
        processed: u64,
        /// Number of explicitly skipped events before stopping.
        skipped: u64,
    },
    /// Continuous execution observed cancellation.
    Cancelled {
        /// Number of committed event effects before cancellation.
        processed: u64,
        /// Number of explicitly skipped events before cancellation.
        skipped: u64,
    },
}

/// Starts a PostgreSQL transactional projection.
#[expect(
    clippy::manual_async_fn,
    reason = "public future must be Send for backend-neutral fixture traits"
)]
pub fn run_transactional_projection<P, S>(
    projector: P,
    source: &S,
    store: &PostgresProjectionStore,
    config: PostgresProjectionConfig,
) -> impl Future<Output = Result<ProjectionRunOutcome, TransactionalProjectionError>> + Send
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    async move {
        let projector_name = projector.name().clone();
        let leader = store.acquire_leader(&projector_name).await?;
        run_transactional_projection_with_leader(projector, source, config, leader).await
    }
}

pub(crate) async fn run_transactional_projection_with_leader<P, S>(
    mut projector: P,
    source: &S,
    config: PostgresProjectionConfig,
    mut leader: ProjectionLeader,
) -> Result<ProjectionRunOutcome, TransactionalProjectionError>
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    let projector_name = projector.name().clone();
    let mut after = load_validated_progress(
        &mut leader,
        &projector_name,
        source.source_id(),
        config.selection().id(),
    )
    .await?;
    let mut processed = 0;
    let mut skipped = 0;

    loop {
        let cycle = drain_cycle(
            &mut projector,
            &mut leader,
            &projector_name,
            source,
            &config,
            &mut after,
            &mut processed,
            &mut skipped,
        )
        .await?;
        let through = match cycle {
            DrainCycleOutcome::CaughtUp(through) => through,
            DrainCycleOutcome::Stopped(position) => {
                leader.release().await?;
                return Ok(ProjectionRunOutcome::Stopped {
                    position,
                    processed,
                    skipped,
                });
            }
        };

        match config.mode() {
            PostgresProjectionMode::Batch => {
                leader.release().await?;
                return Ok(ProjectionRunOutcome::CaughtUp {
                    processed,
                    skipped,
                    through,
                });
            }
            PostgresProjectionMode::Continuous(cancellation) => {
                tokio::select! {
                    () = config
                        .poll_sleeper()
                        .sleep(config.continuous_poll_interval()) => {}
                    () = cancellation.cancelled() => {
                        leader.release().await?;
                        return Ok(ProjectionRunOutcome::Cancelled { processed, skipped });
                    }
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the drain cycle makes runner ownership and cumulative state explicit"
)]
async fn drain_cycle<P, S>(
    projector: &mut P,
    leader: &mut ProjectionLeader,
    projector_name: &eventcore_types::ProjectorName,
    source: &S,
    config: &PostgresProjectionConfig,
    after: &mut Option<DeliveryPosition>,
    processed: &mut u64,
    skipped: &mut u64,
) -> Result<DrainCycleOutcome, TransactionalProjectionError>
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    let through = source.high_watermark().await.map_err(source_error)?;
    let Some(through) = through else {
        return Ok(DrainCycleOutcome::CaughtUp(None));
    };

    loop {
        let envelopes = source
            .read_envelopes(
                config.selection(),
                *after,
                DeliveryUpperBound::Inclusive(through),
                config.batch_size(),
            )
            .await
            .map_err(source_error)?;
        if envelopes.is_empty() {
            return Ok(DrainCycleOutcome::CaughtUp(Some(through)));
        }

        for envelope in envelopes {
            let position = envelope.position();
            *after = Some(position);
            match process_envelope(projector, leader, projector_name, source, config, envelope)
                .await?
            {
                EnvelopeResult::Processed => *processed += 1,
                EnvelopeResult::Skipped => *skipped += 1,
                EnvelopeResult::Suppressed => {}
                EnvelopeResult::Stopped => return Ok(DrainCycleOutcome::Stopped(position)),
            }
        }
    }
}

async fn load_validated_progress(
    leader: &mut ProjectionLeader,
    projector_name: &eventcore_types::ProjectorName,
    source_id: &eventcore_types::DeliverySourceId,
    selection_id: &eventcore_types::ProjectionSelectionId,
) -> Result<Option<DeliveryPosition>, TransactionalProjectionError> {
    let mut transaction = leader.begin().await?;
    let progress = match ProjectionLeader::load_progress(&mut transaction, projector_name).await {
        Ok(progress) => progress,
        Err(error) => return Err(rollback_preserving_error(transaction, error).await),
    };
    let validation = progress.as_ref().map_or(Ok(()), |progress| {
        if progress.source_id() != source_id {
            return Err(TransactionalProjectionError::SourceIdentityMismatch {
                projector: projector_name.clone(),
                persisted: progress.source_id().clone(),
                configured: source_id.clone(),
            });
        }
        if progress.selection_id() != selection_id {
            return Err(TransactionalProjectionError::SelectionIdentityMismatch {
                projector: projector_name.clone(),
                persisted: progress.selection_id().clone(),
                configured: selection_id.clone(),
            });
        }
        Ok(())
    });
    if let Err(error) = validation {
        return Err(rollback_preserving_error(transaction, error).await);
    }
    transaction.rollback().await.map_err(leadership_lost)?;
    Ok(progress.map(|progress| progress.position()))
}

async fn process_envelope<P, S>(
    projector: &mut P,
    leader: &mut ProjectionLeader,
    projector_name: &eventcore_types::ProjectorName,
    source: &S,
    config: &PostgresProjectionConfig,
    envelope: PersistedEventEnvelope,
) -> Result<EnvelopeResult, TransactionalProjectionError>
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    let position = envelope.position();
    let mut transaction = leader
        .begin()
        .await
        .map_err(|error| progress_at_position(position, error))?;
    let pending =
        match position_is_pending(&mut transaction, projector_name, source, config, position).await
        {
            Ok(pending) => pending,
            Err(error) => return Err(rollback_preserving_error(transaction, error).await),
        };
    if !pending {
        transaction.rollback().await.map_err(leadership_lost)?;
        return Ok(EnvelopeResult::Suppressed);
    }
    let event = match serde_json::from_str(envelope.payload().get()) {
        Ok(event) => event,
        Err(source) => {
            let error = TransactionalProjectionError::Decode {
                position,
                source: Box::new(source),
            };
            return Err(rollback_preserving_error(transaction, error).await);
        }
    };
    let mut attempt = 1_u32;

    loop {
        match projector.apply(&event, position, &mut transaction).await {
            Ok(after_commit) => {
                if let Err(error) = ProjectionLeader::advance_progress(
                    &mut transaction,
                    projector_name,
                    source.source_id(),
                    config.selection().id(),
                    position,
                )
                .await
                .map_err(|error| progress_at_position(position, error))
                {
                    return Err(rollback_preserving_error(transaction, error).await);
                }
                transaction.commit().await.map_err(|source| {
                    TransactionalProjectionError::CommitIndeterminate {
                        position,
                        source: Box::new(source),
                    }
                })?;
                after_commit.execute().await.map_err(|source| {
                    TransactionalProjectionError::AfterCommitFailed {
                        committed_position: position,
                        source: Box::new(source),
                    }
                })?;
                return Ok(EnvelopeResult::Processed);
            }
            Err(error) => {
                let decision = projector.on_error(ProjectionFailureContext::new(
                    position,
                    &error,
                    AttemptNumber::new(
                        NonZeroU32::new(attempt).expect("application attempt starts at one"),
                    ),
                ));
                transaction.rollback().await.map_err(leadership_lost)?;

                match decision {
                    ProjectionFailureDecision::Retry => {
                        if attempt > config.retry_policy().max_retries() {
                            return Err(TransactionalProjectionError::RetryExhausted {
                                position,
                                attempts: attempt,
                                source: Box::new(error),
                            });
                        }
                        let delay = retry_delay(config, attempt);
                        config.retry_sleeper().sleep(delay).await;
                        attempt = attempt.checked_add(1).ok_or_else(|| {
                            TransactionalProjectionError::Configuration {
                                source: super::ProjectionConfigurationError::TooManyRetries,
                            }
                        })?;
                        transaction = leader
                            .begin()
                            .await
                            .map_err(|error| progress_at_position(position, error))?;
                        let pending = match position_is_pending(
                            &mut transaction,
                            projector_name,
                            source,
                            config,
                            position,
                        )
                        .await
                        {
                            Ok(pending) => pending,
                            Err(error) => {
                                return Err(rollback_preserving_error(transaction, error).await);
                            }
                        };
                        if !pending {
                            transaction.rollback().await.map_err(leadership_lost)?;
                            return Ok(EnvelopeResult::Suppressed);
                        }
                    }
                    ProjectionFailureDecision::Skip => {
                        return skip_position(leader, projector_name, source, config, position)
                            .await;
                    }
                    ProjectionFailureDecision::Stop => return Ok(EnvelopeResult::Stopped),
                    ProjectionFailureDecision::Fatal => {
                        return Err(TransactionalProjectionError::Application {
                            position,
                            source: Box::new(error),
                        });
                    }
                }
            }
        }
    }
}

async fn skip_position<S>(
    leader: &mut ProjectionLeader,
    projector_name: &eventcore_types::ProjectorName,
    source: &S,
    config: &PostgresProjectionConfig,
    position: DeliveryPosition,
) -> Result<EnvelopeResult, TransactionalProjectionError>
where
    S: ProjectionSource,
{
    let mut transaction = leader
        .begin()
        .await
        .map_err(|error| progress_at_position(position, error))?;
    let pending =
        match position_is_pending(&mut transaction, projector_name, source, config, position).await
        {
            Ok(pending) => pending,
            Err(error) => return Err(rollback_preserving_error(transaction, error).await),
        };
    if !pending {
        transaction.rollback().await.map_err(leadership_lost)?;
        return Ok(EnvelopeResult::Suppressed);
    }
    if let Err(error) = ProjectionLeader::advance_progress(
        &mut transaction,
        projector_name,
        source.source_id(),
        config.selection().id(),
        position,
    )
    .await
    .map_err(|error| progress_at_position(position, error))
    {
        return Err(rollback_preserving_error(transaction, error).await);
    }
    transaction.commit().await.map_err(|source| {
        TransactionalProjectionError::CommitIndeterminate {
            position,
            source: Box::new(source),
        }
    })?;
    Ok(EnvelopeResult::Skipped)
}

async fn rollback_preserving_error(
    transaction: sqlx::Transaction<'_, sqlx::Postgres>,
    error: TransactionalProjectionError,
) -> TransactionalProjectionError {
    match transaction.rollback().await {
        Ok(()) => error,
        Err(source) => leadership_lost(source),
    }
}

fn leadership_lost(source: sqlx::Error) -> TransactionalProjectionError {
    TransactionalProjectionError::LeadershipLost {
        source: Box::new(source),
    }
}

async fn position_is_pending<S>(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    projector_name: &eventcore_types::ProjectorName,
    source: &S,
    config: &PostgresProjectionConfig,
    position: DeliveryPosition,
) -> Result<bool, TransactionalProjectionError>
where
    S: ProjectionSource,
{
    let progress = ProjectionLeader::load_progress(transaction, projector_name)
        .await
        .map_err(|error| progress_at_position(position, error))?;
    let Some(progress) = progress else {
        return Ok(true);
    };
    if progress.source_id() != source.source_id() {
        return Err(TransactionalProjectionError::SourceIdentityMismatch {
            projector: projector_name.clone(),
            persisted: progress.source_id().clone(),
            configured: source.source_id().clone(),
        });
    }
    if progress.selection_id() != config.selection().id() {
        return Err(TransactionalProjectionError::SelectionIdentityMismatch {
            projector: projector_name.clone(),
            persisted: progress.selection_id().clone(),
            configured: config.selection().id().clone(),
        });
    }
    Ok(position > progress.position())
}

fn retry_delay(config: &PostgresProjectionConfig, retry_number: u32) -> Duration {
    let policy = config.retry_policy();
    let initial = policy.initial_delay();
    let maximum = policy.maximum_delay();
    if initial.is_zero() {
        return initial;
    }

    let factor = policy
        .multiplier()
        .powf(f64::from(retry_number.saturating_sub(1)));
    let scaled_seconds = initial.as_secs_f64() * factor;
    if !scaled_seconds.is_finite() || scaled_seconds >= maximum.as_secs_f64() {
        maximum
    } else {
        Duration::from_secs_f64(scaled_seconds)
    }
}

fn progress_at_position(
    position: DeliveryPosition,
    error: TransactionalProjectionError,
) -> TransactionalProjectionError {
    match error {
        TransactionalProjectionError::ProgressStore { source } => {
            TransactionalProjectionError::Progress { position, source }
        }
        error => error,
    }
}

fn source_error(
    error: impl std::error::Error + Send + Sync + 'static,
) -> TransactionalProjectionError {
    TransactionalProjectionError::Source {
        source: Box::new(error),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use eventcore_types::{
        EventTypeName, ProjectionSelection, ProjectionSelectionId, ProjectionStreamFilter,
    };

    use super::{PostgresProjectionConfig, retry_delay};
    use crate::ProjectionRetryPolicy;

    // Break caught: removing the zero-delay guard lets an overflowing exponential factor turn
    // `0 * infinity` into NaN and incorrectly sleep for the configured maximum.
    #[test]
    fn zero_initial_retry_delay_remains_zero_when_the_multiplier_overflows() {
        let selection = ProjectionSelection::try_new(
            ProjectionSelectionId::try_new("retry-delay-test")
                .expect("test selection ID should be valid"),
            ProjectionStreamFilter::All,
            vec![EventTypeName::try_new("retry-event").expect("test event type should be valid")],
        )
        .expect("test selection should be valid");
        let policy = ProjectionRetryPolicy::new(
            u32::MAX - 1,
            Duration::ZERO,
            f64::MAX,
            Duration::from_secs(1),
        )
        .expect("test retry policy should be valid");
        let config = PostgresProjectionConfig::new(selection).with_retry_policy(policy);

        assert_eq!(retry_delay(&config, u32::MAX), Duration::ZERO);
    }
}
