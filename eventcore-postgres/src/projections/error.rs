use std::error::Error;

use eventcore_types::{DeliveryPosition, DeliverySourceId, ProjectionSelectionId, ProjectorName};
use thiserror::Error;

use super::ProjectionConfigurationError;

/// Type-erased failure source retained by the stable runner error boundary.
pub type BoxedProjectionError = Box<dyn Error + Send + Sync>;

/// Terminal failures from a transactional projection run.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TransactionalProjectionError {
    /// Another process owns this projector's read-model leadership lock.
    #[error("transactional projection leadership is busy")]
    LeadershipBusy,
    /// The leader session was lost and can no longer authorize writes.
    #[error("transactional projection leadership was lost")]
    LeadershipLost {
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// The event source could not provide a page or high-water mark.
    #[error("transactional projection source failed")]
    Source {
        /// Underlying source error.
        #[source]
        source: BoxedProjectionError,
    },
    /// A selected persisted payload could not decode into the application's event contract.
    #[error("transactional projection could not decode event at position {position:?}")]
    Decode {
        /// Position of the malformed selected envelope.
        position: DeliveryPosition,
        /// Underlying deserialization error.
        #[source]
        source: BoxedProjectionError,
    },
    /// The projector chose the `Fatal` decision for an application error.
    #[error("transactional projection application failed at position {position:?}")]
    Application {
        /// Failed position.
        position: DeliveryPosition,
        /// Underlying application error.
        #[source]
        source: BoxedProjectionError,
    },
    /// Retry attempts were exhausted without committing the event effect.
    #[error("transactional projection retries exhausted at position {position:?}")]
    RetryExhausted {
        /// Failed position.
        position: DeliveryPosition,
        /// Total application invocations, including the initial attempt.
        attempts: u32,
        /// Last application error.
        #[source]
        source: BoxedProjectionError,
    },
    /// Progress could not be loaded or advanced for the pending delivery position.
    #[error("transactional projection progress operation failed at position {position:?}")]
    Progress {
        /// Delivery position whose transaction remains pending.
        position: DeliveryPosition,
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// A progress storage operation without a pending delivery position failed.
    #[error("transactional projection progress storage operation failed")]
    ProgressStore {
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// PostgreSQL did not acknowledge the commit, so durable state must be inspected on recovery.
    #[error("transactional projection commit acknowledgement is indeterminate")]
    CommitIndeterminate {
        /// Position whose transaction may or may not have committed.
        position: DeliveryPosition,
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// Existing progress belongs to a different delivery source.
    #[error("transactional projection source identity does not match {projector}")]
    SourceIdentityMismatch {
        /// Projector whose durable identity was incompatible.
        projector: ProjectorName,
        /// Source identity persisted with durable progress.
        persisted: DeliverySourceId,
        /// Source identity configured for this invocation.
        configured: DeliverySourceId,
    },
    /// Existing progress belongs to a different projection selection.
    #[error("transactional projection selection identity does not match {projector}")]
    SelectionIdentityMismatch {
        /// Projector whose durable identity was incompatible.
        projector: ProjectorName,
        /// Selection identity persisted with durable progress.
        persisted: ProjectionSelectionId,
        /// Selection identity configured for this invocation.
        configured: ProjectionSelectionId,
    },
    /// Configuration cannot safely start a projection.
    #[error("transactional projection configuration is invalid")]
    Configuration {
        /// Invalid configuration detail.
        #[source]
        source: ProjectionConfigurationError,
    },
    /// A confirmed commit succeeded but its after-commit action failed.
    #[error(
        "transactional projection after-commit action failed at position {committed_position:?}"
    )]
    AfterCommitFailed {
        /// Already committed position.
        committed_position: DeliveryPosition,
        /// Underlying hook error.
        #[source]
        source: BoxedProjectionError,
    },
}

/// Failures from a coordinated basic reset.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProjectionResetError {
    /// Another invocation owns this projector's leadership grant.
    #[error("transactional projection reset leadership is busy")]
    Busy,
    /// The leader session was lost while starting, rolling back, or releasing the reset.
    ///
    /// A release failure can occur after the reset commit was acknowledged. Callers must not
    /// infer from this variant that the reset was rolled back or did not commit.
    #[error("transactional projection reset leadership was lost")]
    LeadershipLost {
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// Existing progress belongs to a different delivery source.
    #[error("transactional projection reset source identity does not match {projector}")]
    SourceIdentityMismatch {
        /// Projector whose durable identity was incompatible.
        projector: ProjectorName,
        /// Source identity persisted with durable progress.
        persisted: DeliverySourceId,
        /// Source identity configured for this reset.
        configured: DeliverySourceId,
    },
    /// Existing progress belongs to a different projection selection.
    #[error("transactional projection reset selection identity does not match {projector}")]
    SelectionIdentityMismatch {
        /// Projector whose durable identity was incompatible.
        projector: ProjectorName,
        /// Selection identity persisted with durable progress.
        persisted: ProjectionSelectionId,
        /// Selection identity configured for this reset.
        configured: ProjectionSelectionId,
    },
    /// Application reset code failed and its transaction was rolled back.
    #[error("transactional projection reset callback failed")]
    Callback {
        /// Underlying application error.
        #[source]
        source: BoxedProjectionError,
    },
    /// Progress validation or deletion failed before commit.
    #[error("transactional projection reset progress operation failed")]
    Progress {
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
    /// PostgreSQL did not acknowledge the reset commit, so durable state is indeterminate.
    #[error("transactional projection reset commit acknowledgement is indeterminate")]
    CommitIndeterminate {
        /// Underlying database error.
        #[source]
        source: BoxedProjectionError,
    },
}

/// Failures from the reset-and-replay convenience operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProjectionResetAndReplayError {
    /// The coordinated reset phase failed.
    #[error("transactional projection reset phase failed")]
    Reset(#[source] ProjectionResetError),
    /// The replay phase failed after reset committed.
    #[error("transactional projection replay phase failed")]
    Replay(#[source] TransactionalProjectionError),
}
