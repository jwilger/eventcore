use eventcore_types::{
    AttemptNumber, DeliveryPosition, DeliveryUpperBound, PersistedEventEnvelope, ProjectionSource,
};
use std::future::Future;
use std::num::NonZeroU32;
use std::time::Duration;

use super::{
    AfterCommit, PostgresProjectionConfig, PostgresProjectionStore, PostgresProjector,
    ProjectionFailureContext, ProjectionFailureDecision, ProjectionLeader,
    TransactionalProjectionError,
};

enum EnvelopeResult {
    Processed,
    Skipped,
    Suppressed,
    Stopped,
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
    mut projector: P,
    source: &S,
    store: &PostgresProjectionStore,
    config: PostgresProjectionConfig,
) -> impl Future<Output = Result<ProjectionRunOutcome, TransactionalProjectionError>> + Send
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    async move {
        let through = source.high_watermark().await.map_err(source_error)?;
        let Some(through) = through else {
            return Ok(ProjectionRunOutcome::CaughtUp {
                processed: 0,
                skipped: 0,
                through: None,
            });
        };

        let projector_name = projector.name().clone();
        let mut leader = store.acquire_leader(&projector_name).await?;
        let mut after = None;
        let mut processed = 0;
        let mut skipped = 0;

        loop {
            let envelopes = source
                .read_envelopes(
                    config.selection(),
                    after,
                    DeliveryUpperBound::Inclusive(through),
                    config.batch_size(),
                )
                .await
                .map_err(source_error)?;
            if envelopes.is_empty() {
                break;
            }

            for envelope in envelopes {
                let position = envelope.position();
                after = Some(position);
                match process_envelope(
                    &mut projector,
                    &mut leader,
                    &projector_name,
                    source,
                    &config,
                    envelope,
                )
                .await?
                {
                    EnvelopeResult::Processed => processed += 1,
                    EnvelopeResult::Skipped => skipped += 1,
                    EnvelopeResult::Suppressed => {}
                    EnvelopeResult::Stopped => {
                        leader.release().await?;
                        return Ok(ProjectionRunOutcome::Stopped {
                            position,
                            processed,
                            skipped,
                        });
                    }
                }
            }
        }

        leader.release().await?;
        Ok(ProjectionRunOutcome::CaughtUp {
            processed,
            skipped,
            through: Some(through),
        })
    }
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
    if !position_is_pending(&mut transaction, projector_name, source, config, position).await? {
        return Ok(EnvelopeResult::Suppressed);
    }
    let event = serde_json::from_str(envelope.payload().get()).map_err(|source| {
        TransactionalProjectionError::Decode {
            position,
            source: Box::new(source),
        }
    })?;
    let mut attempt = 1_u32;

    loop {
        match projector.apply(&event, position, &mut transaction).await {
            Ok(after_commit) => {
                ProjectionLeader::advance_progress(
                    &mut transaction,
                    projector_name,
                    source.source_id(),
                    config.selection().id(),
                    position,
                )
                .await
                .map_err(|error| progress_at_position(position, error))?;
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
                transaction.rollback().await.map_err(|source| {
                    TransactionalProjectionError::Progress {
                        position,
                        source: Box::new(source),
                    }
                })?;

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
                        if !position_is_pending(
                            &mut transaction,
                            projector_name,
                            source,
                            config,
                            position,
                        )
                        .await?
                        {
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
    if !position_is_pending(&mut transaction, projector_name, source, config, position).await? {
        return Ok(EnvelopeResult::Suppressed);
    }
    ProjectionLeader::advance_progress(
        &mut transaction,
        projector_name,
        source.source_id(),
        config.selection().id(),
        position,
    )
    .await
    .map_err(|error| progress_at_position(position, error))?;
    transaction.commit().await.map_err(|source| {
        TransactionalProjectionError::CommitIndeterminate {
            position,
            source: Box::new(source),
        }
    })?;
    Ok(EnvelopeResult::Skipped)
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
    if initial.is_zero() || maximum.is_zero() || initial >= maximum {
        return initial.min(maximum);
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
