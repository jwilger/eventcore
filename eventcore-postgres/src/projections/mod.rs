//! Transactional projection delivery support for PostgreSQL.

mod config;
mod error;
mod migration;
mod projector;
mod runner;
mod source;
mod store;

pub use config::{
    PostgresProjectionConfig, PostgresProjectionMode, ProjectionConfigurationError,
    ProjectionPollSleeper, ProjectionRetryPolicy, ProjectionRetrySleeper,
    TokioProjectionPollSleeper, TokioProjectionRetrySleeper,
};
pub use error::{BoxedProjectionError, TransactionalProjectionError};
pub use projector::{
    AfterCommit, NoopAfterCommit, PostgresProjector, ProjectionFailureContext,
    ProjectionFailureDecision,
};
pub use runner::{ProjectionRunOutcome, run_transactional_projection};
pub use source::{PostgresProjectionSource, PostgresProjectionSourceError};
pub(crate) use store::ProjectionLeader;
pub use store::{PostgresProjectionStore, ProjectionProgress};
