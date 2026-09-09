use std::convert::Infallible;
use std::error::Error;
use std::future::Future;

use eventcore_types::{AttemptNumber, DeliveryPosition, ProjectorName};
use serde::de::DeserializeOwned;
use sqlx::{Postgres, Transaction};

/// Work that is safe to perform only after the read-model transaction commits.
pub trait AfterCommit: Send + 'static {
    /// Error returned by the after-commit action.
    type Error: Error + Send + Sync + 'static;

    /// Executes the action after the transaction's commit was acknowledged.
    fn execute(self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// An after-commit action that has no work to perform.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopAfterCommit;

impl AfterCommit for NoopAfterCommit {
    type Error = Infallible;

    async fn execute(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Decision made by an application after its event effect failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionFailureDecision {
    /// Roll back and retry the event, subject to the configured retry policy.
    Retry,
    /// Roll back the failed effect and explicitly advance past the event.
    Skip,
    /// Roll back and end this run without advancing progress.
    Stop,
    /// Roll back and return the application error.
    Fatal,
}

/// Public failure information passed to [`PostgresProjector::on_error`].
#[derive(Debug)]
pub struct ProjectionFailureContext<'a, E> {
    position: DeliveryPosition,
    error: &'a E,
    attempt: AttemptNumber,
}

impl<'a, E> ProjectionFailureContext<'a, E> {
    /// Creates failure context for a projector invocation.
    pub fn new(position: DeliveryPosition, error: &'a E, attempt: AttemptNumber) -> Self {
        Self {
            position,
            error,
            attempt,
        }
    }

    /// Returns the delivery position that failed.
    pub fn position(&self) -> DeliveryPosition {
        self.position
    }

    /// Returns the application error.
    pub fn error(&self) -> &'a E {
        self.error
    }

    /// Returns the one-based attempt number.
    pub fn attempt(&self) -> AttemptNumber {
        self.attempt
    }
}

/// Applies decoded events through the transaction owned by the PostgreSQL runner.
pub trait PostgresProjector: Send {
    /// Decoded event contract accepted by this projection.
    type Event: DeserializeOwned + Send + Sync;
    /// Application error returned while applying an event.
    type Error: Error + Send + Sync + 'static;
    /// Work deferred until the event transaction commits.
    type AfterCommit: AfterCommit;

    /// Returns this projection's stable, application-supplied name.
    fn name(&self) -> &ProjectorName;

    /// Applies one event through the runner-owned read-model transaction.
    fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        position: DeliveryPosition,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> impl Future<Output = Result<Self::AfterCommit, Self::Error>> + Send + 'a
    where
        'c: 'a;

    /// Chooses how the runner handles an application failure.
    fn on_error(
        &mut self,
        failure: ProjectionFailureContext<'_, Self::Error>,
    ) -> ProjectionFailureDecision {
        let _ = failure;
        ProjectionFailureDecision::Fatal
    }
}
