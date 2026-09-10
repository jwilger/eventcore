//! Transactional projection delivery support for PostgreSQL.

pub use eventcore_types::{
    DeliveryIdentityError, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, EventTypeName,
    PersistedEventEnvelope, PersistedEventId, ProjectionSelection, ProjectionSelectionError,
    ProjectionSelectionId, ProjectionSource, ProjectionStreamFilter, ProjectorName,
};
pub use sqlx::{Postgres, Transaction};

mod config;
mod error;
mod migration;
mod projector;
mod reset;
mod runner;
mod source;
mod store;

pub use config::{
    PostgresProjectionConfig, PostgresProjectionMode, ProjectionConfigurationError,
    ProjectionPollSleeper, ProjectionRetryPolicy, ProjectionRetrySleeper,
    TokioProjectionPollSleeper, TokioProjectionRetrySleeper,
};
pub use error::{
    BoxedProjectionError, ProjectionResetAndReplayError, ProjectionResetError,
    TransactionalProjectionError,
};
pub use projector::{
    AfterCommit, NoopAfterCommit, PostgresProjectionReset, PostgresProjector,
    ProjectionFailureContext, ProjectionFailureDecision,
};
pub use reset::{reset_and_replay_transactional_projection, reset_transactional_projection};
pub use runner::{ProjectionRunOutcome, run_transactional_projection};
pub use source::{PostgresProjectionSource, PostgresProjectionSourceError};
pub(crate) use store::ProjectionLeader;
pub use store::{PostgresProjectionStore, ProjectionProgress};
