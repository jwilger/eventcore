use eventcore_types::{DeliverySourceId, ProjectionSelectionId, ProjectionSource, ProjectorName};

use super::{
    PostgresProjectionConfig, PostgresProjectionReset, PostgresProjectionStore, PostgresProjector,
    ProjectionLeader, ProjectionResetAndReplayError, ProjectionResetError, ProjectionRunOutcome,
    TransactionalProjectionError, runner::run_transactional_projection_with_leader,
};

/// Reinitializes a projection and deletes matching progress under named leadership.
#[expect(
    clippy::manual_async_fn,
    reason = "public future must be Send for backend-neutral fixture traits"
)]
pub fn reset_transactional_projection<'a, R>(
    reset: &'a mut R,
    projector_name: &'a ProjectorName,
    source_id: &'a DeliverySourceId,
    selection_id: &'a ProjectionSelectionId,
    store: &'a PostgresProjectionStore,
) -> impl Future<Output = Result<(), ProjectionResetError>> + Send + 'a
where
    R: PostgresProjectionReset + 'a,
{
    async move {
        let mut leader = store
            .acquire_leader(projector_name)
            .await
            .map_err(map_leadership_error)?;
        if let Err(error) =
            reset_with_leader(reset, projector_name, source_id, selection_id, &mut leader).await
        {
            drop(leader);
            return Err(error);
        }
        leader.release().await.map_err(map_leadership_error)
    }
}

/// Reinitializes a projection and replays it while retaining named leadership.
#[expect(
    clippy::manual_async_fn,
    reason = "public future must be Send for backend-neutral fixture traits and spawned recovery"
)]
pub fn reset_and_replay_transactional_projection<'a, P, R, S>(
    projector: P,
    reset: &'a mut R,
    source: &'a S,
    store: &'a PostgresProjectionStore,
    config: PostgresProjectionConfig,
) -> impl Future<Output = Result<ProjectionRunOutcome, ProjectionResetAndReplayError>> + Send + 'a
where
    P: PostgresProjector + 'a,
    R: PostgresProjectionReset + 'a,
    S: ProjectionSource + 'a,
{
    async move {
        let projector_name = projector.name().clone();
        let mut leader = store
            .acquire_leader(&projector_name)
            .await
            .map_err(map_leadership_error)
            .map_err(ProjectionResetAndReplayError::Reset)?;
        if let Err(error) = reset_with_leader(
            reset,
            &projector_name,
            source.source_id(),
            config.selection().id(),
            &mut leader,
        )
        .await
        {
            drop(leader);
            return Err(ProjectionResetAndReplayError::Reset(error));
        }

        run_transactional_projection_with_leader(projector, source, config, leader)
            .await
            .map_err(ProjectionResetAndReplayError::Replay)
    }
}

async fn reset_with_leader<R>(
    reset: &mut R,
    projector_name: &ProjectorName,
    source_id: &DeliverySourceId,
    selection_id: &ProjectionSelectionId,
    leader: &mut ProjectionLeader,
) -> Result<(), ProjectionResetError>
where
    R: PostgresProjectionReset,
{
    let mut transaction = leader.begin().await.map_err(map_leadership_error)?;
    let progress = match ProjectionLeader::load_progress(&mut transaction, projector_name).await {
        Ok(progress) => progress,
        Err(error) => {
            return Err(
                rollback_preserving_reset_error(transaction, map_progress_error(error)).await,
            );
        }
    };

    if let Some(progress) = &progress {
        if progress.source_id() != source_id {
            let error = ProjectionResetError::SourceIdentityMismatch {
                projector: projector_name.clone(),
                persisted: progress.source_id().clone(),
                configured: source_id.clone(),
            };
            return Err(rollback_preserving_reset_error(transaction, error).await);
        }
        if progress.selection_id() != selection_id {
            let error = ProjectionResetError::SelectionIdentityMismatch {
                projector: projector_name.clone(),
                persisted: progress.selection_id().clone(),
                configured: selection_id.clone(),
            };
            return Err(rollback_preserving_reset_error(transaction, error).await);
        }
    }

    if let Err(source) = reset.reset(&mut transaction).await {
        return Err(rollback_preserving_reset_error(
            transaction,
            ProjectionResetError::Callback {
                source: Box::new(source),
            },
        )
        .await);
    }
    if let Err(error) = ProjectionLeader::delete_progress(&mut transaction, projector_name).await {
        return Err(rollback_preserving_reset_error(transaction, map_progress_error(error)).await);
    }
    transaction
        .commit()
        .await
        .map_err(|source| ProjectionResetError::CommitIndeterminate {
            source: Box::new(source),
        })
}

async fn rollback_preserving_reset_error(
    transaction: sqlx::Transaction<'_, sqlx::Postgres>,
    error: ProjectionResetError,
) -> ProjectionResetError {
    match transaction.rollback().await {
        Ok(()) => error,
        Err(source) => ProjectionResetError::LeadershipLost {
            source: Box::new(source),
        },
    }
}

fn map_progress_error(error: TransactionalProjectionError) -> ProjectionResetError {
    match error {
        TransactionalProjectionError::ProgressStore { source }
        | TransactionalProjectionError::Progress { source, .. } => {
            ProjectionResetError::Progress { source }
        }
        TransactionalProjectionError::LeadershipLost { source } => {
            ProjectionResetError::LeadershipLost { source }
        }
        error => ProjectionResetError::Progress {
            source: Box::new(error),
        },
    }
}

fn map_leadership_error(error: TransactionalProjectionError) -> ProjectionResetError {
    match error {
        TransactionalProjectionError::LeadershipBusy => ProjectionResetError::Busy,
        TransactionalProjectionError::LeadershipLost { source } => {
            ProjectionResetError::LeadershipLost { source }
        }
        error => ProjectionResetError::LeadershipLost {
            source: Box::new(error),
        },
    }
}
