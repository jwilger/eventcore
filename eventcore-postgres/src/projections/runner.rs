use eventcore_types::{
    DeliveryPosition, DeliveryUpperBound, PersistedEventEnvelope, ProjectionSource,
};
use std::future::Future;

use super::{
    AfterCommit, PostgresProjectionConfig, PostgresProjectionStore, PostgresProjector,
    ProjectionLeader, TransactionalProjectionError,
};

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
                after = Some(envelope.position());
                if process_envelope(
                    &mut projector,
                    &mut leader,
                    &projector_name,
                    source,
                    &config,
                    envelope,
                )
                .await?
                {
                    processed += 1;
                }
            }
        }

        leader.release().await?;
        Ok(ProjectionRunOutcome::CaughtUp {
            processed,
            skipped: 0,
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
) -> Result<bool, TransactionalProjectionError>
where
    P: PostgresProjector,
    S: ProjectionSource,
{
    let position = envelope.position();
    let mut transaction = leader
        .begin()
        .await
        .map_err(|error| progress_at_position(position, error))?;
    let progress = ProjectionLeader::load_progress(&mut transaction, projector_name)
        .await
        .map_err(|error| progress_at_position(position, error))?;
    if let Some(progress) = progress {
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
        if position <= progress.position() {
            return Ok(false);
        }
    }

    let event = serde_json::from_str(envelope.payload().get()).map_err(|source| {
        TransactionalProjectionError::Decode {
            position,
            source: Box::new(source),
        }
    })?;
    let after_commit = projector
        .apply(&event, position, &mut transaction)
        .await
        .map_err(|source| TransactionalProjectionError::ApplicationFatal {
            position,
            source: Box::new(source),
        })?;
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
    after_commit
        .execute()
        .await
        .map_err(|source| TransactionalProjectionError::AfterCommit {
            position,
            source: Box::new(source),
        })?;
    Ok(true)
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
