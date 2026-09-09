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
    /// Apply the non-idempotent effect, then return an application error in the same transaction.
    ApplyThenFail,
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

/// Backend-neutral classification of a failed projection attempt.
///
/// Fixtures translate their public runner error into this vocabulary so the
/// reusable contract can assert recovery semantics without depending on a
/// particular backend error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionFailureObservation {
    /// Application code rejected the selected event before its effect committed.
    ApplicationFatal {
        /// Position left pending by the failed application mutation.
        position: DeliveryPosition,
    },
    /// Progress persistence failed at a known pending delivery position.
    Progress {
        /// Position whose effect and progress transaction was rolled back.
        position: DeliveryPosition,
    },
    /// The backend exposed a progress failure without its pending position.
    ProgressWithoutPosition,
    /// A selected persisted payload could not decode into the application event.
    Decode {
        /// Position left pending by the malformed selected envelope.
        position: DeliveryPosition,
    },
    /// Saved progress was bound to a different source identity.
    SourceIdentityMismatch,
    /// Saved progress was bound to a different selection identity.
    SelectionIdentityMismatch,
    /// PostgreSQL did not acknowledge the commit, so recovery must inspect durable state.
    CommitIndeterminate {
        /// Position whose commit acknowledgement was lost.
        position: DeliveryPosition,
    },
    /// The backend exposed an indeterminate commit but omitted its pending position.
    CommitIndeterminateWithoutPosition,
    /// The backend exposed a generic identity mismatch without identifying the bad binding.
    UndifferentiatedIdentityMismatch,
    /// The backend returned a public failure outside this recovery contract.
    Other,
}

/// Public result of one finite projection attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionAttemptObservation {
    /// The projection reached its finite source frontier.
    Completed(ProjectionRunOutcome),
    /// The projection stopped with a classified public failure.
    Failed(ProjectionFailureObservation),
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
    ) -> impl Future<Output = Result<DeliveryPosition, Self::Error>> + Send;

    /// Selects the behavior used when the projection receives an event.
    fn select_application_behavior(&mut self, behavior: ProjectionApplicationBehavior);

    /// Runs one finite batch projection catch-up.
    fn run_batch(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionRunOutcome, Self::Error>> + Send;

    /// Runs one finite batch and retains the fixture's public failure classification.
    fn run_batch_attempt(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionAttemptObservation, Self::Error>> + Send;

    /// Runs a continuous projection until the fixture's cancellation condition is observed.
    fn run_continuous(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionRunOutcome, Self::Error>> + Send;

    /// Makes the next progress persistence attempt fail.
    fn inject_progress_failure(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Makes the next transaction lose its commit acknowledgement only after commit processing.
    fn inject_commit_acknowledgement_loss(
        &mut self,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Starts a fresh normal-destination invocation after an indeterminate commit.
    fn recover_after_commit_acknowledgement_loss(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionRunOutcome, Self::Error>> + Send;

    /// Loses the active destination connection while a run is in progress.
    fn inject_connection_loss(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Reads the externally observable non-idempotent effect count.
    fn effect_count(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Reads the number of in-memory applications attempted by the runner.
    fn application_attempt_count(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

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

    /// Persists a deliberately incompatible identity for a known progress position.
    fn seed_progress_identity(
        &mut self,
        source_id: DeliverySourceId,
        selection_id: ProjectionSelectionId,
        position: DeliveryPosition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
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

/// Verifies that a failed application mutation does not expose its effect or progress.
pub async fn mutation_failure_rolls_back_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::ApplyThenFail);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::ApplicationFatal {
            position,
        }),
        "an application mutation failure must be a typed terminal failure",
    );
    assert_eq!(
        fixture.effect_count().await?,
        0,
        "a mutation applied before the error must still roll back its effect"
    );
    assert_eq!(
        fixture.progress().await?,
        None,
        "failed mutation must not advance progress"
    );
    Ok(())
}

/// Verifies that a failed progress write rolls back a previously applied mutation.
pub async fn progress_failure_rolls_back_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    fixture.inject_progress_failure().await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::Progress { position }),
        "a progress write failure must identify its exact pending position",
    );
    assert_eq!(
        fixture.effect_count().await?,
        0,
        "failed progress must roll back the effect"
    );
    assert_eq!(
        fixture.progress().await?,
        None,
        "failed progress must remain absent"
    );
    Ok(())
}

/// Verifies that a new runner instance resumes after the last committed position.
pub async fn restart_resumes_from_committed_position_contract<F>(
    fixture: &mut F,
) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let first = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    let _ = fixture.run_batch().await?;
    let second = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(second),
        },
        "restart must apply only the event after committed progress",
    );
    assert_eq!(
        fixture.effect_count().await?,
        2,
        "restart must not duplicate the first effect"
    );
    assert_eq!(
        fixture.progress().await?,
        Some(ProjectionProgressObservation {
            source_id: fixture.source_id().clone(),
            selection_id: fixture.selection_id().clone(),
            position: second,
        }),
        "restart must durably advance from the first committed position",
    );
    assert_ne!(
        first, second,
        "separate persisted events must have distinct positions"
    );
    Ok(())
}

/// Verifies that malformed selected persisted input is terminal and leaves progress pending.
pub async fn malformed_selected_input_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture.append_malformed_input("{\"stream_id\": 7}").await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::Decode { position }),
        "selected malformed input must return its exact decode position",
    );
    assert_eq!(
        fixture.effect_count().await?,
        0,
        "malformed input must not apply an effect"
    );
    assert_eq!(
        fixture.application_attempt_count().await?,
        0,
        "malformed selected input must fail before application code runs"
    );
    assert_eq!(
        fixture.progress().await?,
        None,
        "malformed input must not advance progress"
    );
    Ok(())
}

/// Verifies that saved source identity is checked before application code runs.
pub async fn source_identity_mismatch_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture
        .seed_progress_identity(
            DeliverySourceId::try_new("different-source").expect("test source ID should be valid"),
            fixture.selection_id().clone(),
            position,
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::SourceIdentityMismatch),
        "source identity mismatch must be distinguished from selection mismatch",
    );
    assert_eq!(
        fixture.application_attempt_count().await?,
        0,
        "identity check must precede apply"
    );
    assert_eq!(
        fixture.effect_count().await?,
        0,
        "identity mismatch must not expose an effect"
    );
    Ok(())
}

/// Verifies that saved selection identity is checked before application code runs.
pub async fn selection_identity_mismatch_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture
        .seed_progress_identity(
            fixture.source_id().clone(),
            ProjectionSelectionId::try_new("different-selection")
                .expect("test selection ID should be valid"),
            position,
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(
            ProjectionFailureObservation::SelectionIdentityMismatch
        ),
        "selection identity mismatch must be distinguished from source mismatch",
    );
    assert_eq!(
        fixture.application_attempt_count().await?,
        0,
        "identity check must precede apply"
    );
    assert_eq!(
        fixture.effect_count().await?,
        0,
        "identity mismatch must not expose an effect"
    );
    Ok(())
}

/// Verifies truthful handling of a commit acknowledgement lost after commit processing began.
pub async fn commit_acknowledgement_loss_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    fixture.inject_commit_acknowledgement_loss().await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::CommitIndeterminate {
            position,
        }),
        "lost commit acknowledgement must never be reported as a proven rollback",
    );
    assert_eq!(
        fixture.application_attempt_count().await?,
        1,
        "an indeterminate commit must not retry the same in-memory delivery",
    );
    assert!(
        fixture.hook_log().await?.is_empty(),
        "after-commit must not run without acknowledgement"
    );
    assert_eq!(
        fixture.effect_count().await?,
        1,
        "the committed/lost-ack fixture must expose exactly one non-idempotent effect",
    );
    let expected_progress = Some(ProjectionProgressObservation {
        source_id: fixture.source_id().clone(),
        selection_id: fixture.selection_id().clone(),
        position,
    });
    assert_eq!(
        fixture.progress().await?,
        expected_progress,
        "the committed/lost-ack fixture must expose matching durable progress",
    );
    assert_eq!(
        fixture.recover_after_commit_acknowledgement_loss().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: Some(position),
        },
        "a later normal invocation must recover by rereading durable progress",
    );
    assert_eq!(
        fixture.effect_count().await?,
        1,
        "recovery must not duplicate the committed non-idempotent effect",
    );
    assert_eq!(
        fixture.application_attempt_count().await?,
        1,
        "recovery must suppress application code for the already committed position",
    );
    assert_eq!(
        fixture.progress().await?,
        expected_progress,
        "recovery must retain the exact committed progress binding",
    );
    assert!(
        fixture.hook_log().await?.is_empty(),
        "recovery must not replay the after-commit hook missed by the unknown acknowledgement"
    );
    Ok(())
}
