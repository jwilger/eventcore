//! Backend-neutral behavioral contracts for transactional projections.

use std::error::Error;
use std::future::Future;

use eventcore_types::{DeliveryPosition, DeliverySourceId, ProjectionSelectionId};
use serde_json::Value;

/// Application behavior selected by a transactional projection fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionApplicationBehavior {
    /// Apply the event's configured read-model effect.
    Apply,
    /// Return an application failure before its effect can commit.
    Fail,
    /// Ask the runner to retry an application failure.
    Retry,
    /// Ask the runner to skip an application failure.
    Skip,
    /// Ask the runner to stop at an application failure.
    Stop,
}

/// Execution mode selected through a fixture's public behavior boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionRunMode {
    /// Run through the source's current finite high-water mark.
    Batch,
    /// Run until the fixture's cancellation control is triggered.
    Continuous,
}

/// An observable after-commit hook entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionHookLogEntry {
    /// A hook ran after the specified position committed.
    Committed(DeliveryPosition),
}

/// Public effects and progress observed after a projection run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionContractObservation {
    /// Count of externally observable read-model effects.
    pub effect_count: u64,
    /// Durable progress, if any event transaction committed.
    pub progress: Option<ProjectionProgressObservation>,
    /// Ordered after-commit hook observations.
    pub hook_log: Vec<ProjectionHookLogEntry>,
}

/// Public identity and position of durable projection progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionProgressObservation {
    /// Source identity bound to the durable progress record.
    pub source_id: DeliverySourceId,
    /// Selection identity bound to the durable progress record.
    pub selection_id: ProjectionSelectionId,
    /// Last transactionally committed delivery position.
    pub position: DeliveryPosition,
}

/// Backend-neutral controls and observations required by transactional projection contracts.
///
/// Implementations expose product behavior rather than a backend pool, SQL transaction, runner
/// internals, or adapter-specific fault proxy. Backend fixtures may use those mechanisms
/// internally to establish deterministic failures.
pub trait TransactionalProjectionFixture {
    /// Fixture setup or projection-run error.
    type Error: Error + Send + Sync + 'static;

    /// Appends valid selected values to the fixture's event source.
    fn append_values(
        &mut self,
        values: &[Value],
    ) -> impl Future<Output = Result<Vec<DeliveryPosition>, Self::Error>> + Send;

    /// Appends selected malformed persisted input that must block progress.
    fn append_malformed_input(
        &mut self,
        input: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Selects the behavior used when the projection receives an event.
    fn select_application_behavior(&mut self, behavior: ProjectionApplicationBehavior);

    /// Runs one finite batch projection catch-up.
    fn run_batch(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionRunOutcome, Self::Error>> + Send;

    /// Runs a continuous projection until the fixture's cancellation condition is observed.
    fn run_continuous(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionRunOutcome, Self::Error>> + Send;

    /// Makes the next progress persistence attempt fail.
    fn inject_progress_failure(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Loses the active destination connection while a run is in progress.
    fn inject_connection_loss(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Reads the externally observable non-idempotent effect count.
    fn effect_count(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Reads public durable projection progress.
    fn progress(
        &self,
    ) -> impl Future<Output = Result<Option<ProjectionProgressObservation>, Self::Error>> + Send;

    /// Reads the ordered after-commit hook log.
    fn hook_log(
        &self,
    ) -> impl Future<Output = Result<Vec<ProjectionHookLogEntry>, Self::Error>> + Send;

    /// Starts a competing leadership attempt.
    fn start_leadership_attempt(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Forces the active leader's destination session to be lost.
    fn lose_leadership(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Resets application effects and progress through the public reset behavior.
    fn reset(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns the source identity expected in successful progress observations.
    fn source_id(&self) -> &DeliverySourceId;

    /// Returns the selection identity expected in successful progress observations.
    fn selection_id(&self) -> &ProjectionSelectionId;
}

/// Backend-neutral outcome shape asserted by transactional projection contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionRunOutcome {
    /// A finite run reached its captured frontier.
    CaughtUp {
        /// Number of committed effects.
        processed: u64,
        /// Number of explicit skips.
        skipped: u64,
        /// Captured source frontier.
        through: Option<DeliveryPosition>,
    },
    /// A run stopped with one pending position.
    Stopped {
        /// Pending position.
        position: DeliveryPosition,
        /// Number of committed effects.
        processed: u64,
        /// Number of explicit skips.
        skipped: u64,
    },
    /// A continuous run observed cancellation.
    Cancelled {
        /// Number of committed effects.
        processed: u64,
        /// Number of explicit skips.
        skipped: u64,
    },
}

/// Runs the reusable atomic effect-and-progress behavior contract.
pub async fn transactional_projection_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let positions = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?;
    assert_eq!(
        positions.len(),
        1,
        "one appended value must have one delivery position"
    );
    let position = positions[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    let first_outcome = fixture.run_batch().await?;
    let after_first_run = ProjectionContractObservation {
        effect_count: fixture.effect_count().await?,
        progress: fixture.progress().await?,
        hook_log: fixture.hook_log().await?,
    };
    assert_eq!(
        after_first_run.effect_count, 1,
        "first delivery must commit the non-idempotent effect",
    );
    assert_eq!(
        after_first_run.progress,
        Some(ProjectionProgressObservation {
            source_id: fixture.source_id().clone(),
            selection_id: fixture.selection_id().clone(),
            position,
        }),
        "first delivery must commit progress with its source and selection identity",
    );
    assert_eq!(
        first_outcome,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(position),
        },
        "first batch must report exactly one committed event through its frontier",
    );

    let second_outcome = fixture.run_batch().await?;
    let after_redelivery = ProjectionContractObservation {
        effect_count: fixture.effect_count().await?,
        progress: fixture.progress().await?,
        hook_log: fixture.hook_log().await?,
    };
    assert_eq!(
        after_redelivery.effect_count, 1,
        "durable progress must suppress redelivery of the non-idempotent effect",
    );
    assert_eq!(
        after_redelivery.progress, after_first_run.progress,
        "redelivery must leave durable progress unchanged",
    );
    assert_eq!(
        second_outcome,
        ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: Some(position),
        },
        "redelivery batch must be caught up without processing the committed event",
    );
    Ok(())
}
