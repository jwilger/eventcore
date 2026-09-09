//! Backend-neutral behavioral contracts for transactional projections.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use eventcore_types::{BatchSize, DeliveryPosition, DeliverySourceId, ProjectionSelectionId};
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
    /// Fail the first application attempt with retry, then apply successfully.
    RetryThenApply,
    /// Request retry after another runner has durably advanced the same progress.
    RetryWithExternallyCommittedProgress,
    /// Ask the runner to skip an application failure.
    Skip,
    /// Ask the runner to stop at an application failure.
    Stop,
    /// Return an application failure that must be reported as fatal.
    Fatal,
    /// Commit the effect and progress, then fail the after-commit action.
    AfterCommitFail,
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
    /// Another invocation currently owns the projector's leadership grant.
    LeadershipBusy,
    /// The session authorizing all destination writes was lost.
    LeadershipLost,
    /// Application code rejected the selected event before its effect committed.
    ApplicationFatal {
        /// Position left pending by the failed application mutation.
        position: DeliveryPosition,
    },
    /// The application explicitly classified its failure as fatal.
    Application {
        /// Position left pending by the fatal application failure.
        position: DeliveryPosition,
    },
    /// Retry attempts were exhausted at a known pending position.
    RetryExhausted {
        /// Position left pending after all attempts rolled back.
        position: DeliveryPosition,
        /// Exact number of application invocations, including the initial attempt.
        attempts: u32,
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
    /// A confirmed commit succeeded but its after-commit action failed.
    AfterCommitFailed {
        /// Position already committed before the action ran.
        committed_position: DeliveryPosition,
        /// Stable public evidence forwarded from the failed hook.
        source: String,
    },
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

    /// Selects per-invocation application behavior in order.
    fn select_application_script(&mut self, behaviors: &[ProjectionApplicationBehavior]) {
        for &behavior in behaviors {
            self.select_application_behavior(behavior);
        }
    }

    /// Configures the bounded retry policy used by subsequent finite runs.
    fn configure_retry_policy(
        &mut self,
        max_retries: u32,
        initial_delay: Duration,
        multiplier: f64,
        maximum_delay: Duration,
    );

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

    /// Reads opaque top-level transaction identities observed through each apply transaction.
    fn application_attempt_transaction_tokens(
        &self,
    ) -> impl Future<Output = Result<Vec<String>, Self::Error>> + Send;

    /// Reads retry durations requested through the fixture's injected sleeper.
    fn retry_sleep_requests(
        &self,
    ) -> impl Future<Output = Result<Vec<Duration>, Self::Error>> + Send;

    /// Reads durable attempt rows written through the runner-supplied transaction.
    fn transaction_attempt_row_count(
        &self,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Reads public durable projection progress.
    fn progress(
        &self,
    ) -> impl Future<Output = Result<Option<ProjectionProgressObservation>, Self::Error>> + Send;

    /// Reads the ordered after-commit hook log.
    fn hook_log(
        &self,
    ) -> impl Future<Output = Result<Vec<ProjectionHookLogEntry>, Self::Error>> + Send;

    /// Reads the number of after-commit actions invoked, including failures.
    fn hook_attempt_count(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

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

/// Observable result of overlapping attempts to own one projector's leadership grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionLeadershipObservation {
    /// Public failure returned by the competing invocation while the first invocation is active.
    pub competing_failure: ProjectionFailureObservation,
    /// Completed result of the original leader after the fixture releases its application gate.
    pub leader_outcome: ProjectionRunOutcome,
}

/// Durable facts observed after terminating the exact PostgreSQL session owning leadership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionLeadershipLossObservation {
    /// Public terminal failure returned by the stale runner.
    pub failure: ProjectionFailureObservation,
    /// Read-model count observed independently after the stale runner exits.
    pub effect_count: u64,
    /// Durable progress observed independently after the stale runner exits.
    pub progress: Option<ProjectionProgressObservation>,
}

/// Results around an event appended after a finite run captured its frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionFixedHighWaterObservation {
    /// Event that existed when the run captured its finite frontier.
    pub initial_position: DeliveryPosition,
    /// Event appended while the first event was gated inside application code.
    pub appended_position: DeliveryPosition,
    /// First finite run result.
    pub first_outcome: ProjectionRunOutcome,
    /// Durable progress immediately after the first run.
    pub progress_after_first: Option<ProjectionProgressObservation>,
    /// Second finite run result.
    pub second_outcome: ProjectionRunOutcome,
}

/// Backend-neutral controls for finite execution and fencing behavior.
pub trait TransactionalProjectionExecutionFixture: TransactionalProjectionFixture {
    /// Changes the page size used by later finite runs.
    fn configure_batch_size(&mut self, batch_size: BatchSize);

    /// Appends an event outside the configured selection and returns its global position.
    fn append_unselected(
        &mut self,
    ) -> impl Future<Output = Result<DeliveryPosition, Self::Error>> + Send;

    /// Coordinates two overlapping invocations for the same projector identity.
    fn observe_overlapping_leadership(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionLeadershipObservation, Self::Error>> + Send;

    /// Terminates the exact active leader session while application work is gated.
    fn observe_leadership_loss(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionLeadershipLossObservation, Self::Error>> + Send;

    /// Appends after the first run has captured its frontier and before it can commit its event.
    fn observe_fixed_high_watermark(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionFixedHighWaterObservation, Self::Error>> + Send;
}

/// Continuous-mode facts observed through public runner and destination boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionContinuousObservation {
    /// Normal continuous-run completion result.
    pub outcome: ProjectionRunOutcome,
    /// Last position present before a multi-page continuous run starts.
    pub initial_through: Option<DeliveryPosition>,
    /// Positions appended only after the runner first became caught up.
    pub appended_positions: Vec<DeliveryPosition>,
    /// Independently observed read-model count.
    pub effect_count: u64,
    /// Independently observed durable progress.
    pub progress: Option<ProjectionProgressObservation>,
    /// Idle durations actually awaited by the runner.
    pub poll_sleep_requests: Vec<Duration>,
    /// Durable state observed at each awaited idle boundary.
    pub idle_boundaries: Vec<ProjectionIdleBoundaryObservation>,
}

/// Durable state observed when a continuous runner begins awaiting its poll delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionIdleBoundaryObservation {
    /// Independently observed read-model count at the idle boundary.
    pub effect_count: u64,
    /// Independently observed durable progress at the idle boundary.
    pub progress: Option<ProjectionProgressObservation>,
}

/// Backend-neutral controls for continuous polling and cancellation behavior.
pub trait TransactionalProjectionContinuousFixture {
    /// Fixture setup or projection-run error.
    type Error: Error + Send + Sync + 'static;

    /// Catches up, appends after the first idle wait begins, delivers it, then cancels.
    fn observe_delivery_after_initial_catch_up(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionContinuousObservation, Self::Error>> + Send;

    /// Reaches an empty frontier, proves one pending positive idle wait, then cancels it.
    fn observe_idle_cancellation(
        &mut self,
    ) -> impl Future<Output = Result<ProjectionContinuousObservation, Self::Error>> + Send;
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

/// Stable source evidence emitted by the contract's deliberately failing hook.
pub const AFTER_COMMIT_FAILURE_SENTINEL: &str = "fixture after-commit sentinel";

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

/// Verifies bounded retry exhaustion and rollback of every failed attempt transaction.
pub async fn retry_exhaustion_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Retry);
    fixture.configure_retry_policy(2, Duration::from_secs(60 * 60), 3.0, Duration::ZERO);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::RetryExhausted {
            position,
            attempts: 3,
        }),
        "retry exhaustion must report the exact pending position and initial-plus-retry count",
    );
    assert_eq!(fixture.application_attempt_count().await?, 3);
    let transaction_tokens = fixture.application_attempt_transaction_tokens().await?;
    assert_eq!(transaction_tokens.len(), 3);
    assert!(
        transaction_tokens
            .iter()
            .enumerate()
            .all(|(index, token)| transaction_tokens[..index].iter().all(|seen| seen != token)),
        "every failed retry must use a distinct top-level transaction",
    );
    assert_eq!(fixture.transaction_attempt_row_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(fixture.progress().await?, None);
    assert!(fixture.hook_log().await?.is_empty());
    assert_eq!(fixture.hook_attempt_count().await?, 0);
    Ok(())
}

/// Verifies a transient retry rolls back its failed transaction and commits only once.
pub async fn transient_retry_success_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::RetryThenApply);
    fixture.configure_retry_policy(
        2,
        Duration::from_secs(60 * 60),
        2.0,
        Duration::from_millis(50),
    );
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(position),
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 2);
    let transaction_tokens = fixture.application_attempt_transaction_tokens().await?;
    assert_eq!(transaction_tokens.len(), 2);
    assert_ne!(
        transaction_tokens[0], transaction_tokens[1],
        "a retry must begin a fresh top-level transaction, not a savepoint",
    );
    assert_eq!(
        fixture.retry_sleep_requests().await?,
        vec![Duration::from_millis(50)],
        "retry must request the exact capped nonzero delay through the configured sleeper",
    );
    assert_eq!(
        fixture.transaction_attempt_row_count().await?,
        1,
        "only the successful fresh transaction may retain its attempt row",
    );
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(position),
    );
    assert_eq!(
        fixture.hook_log().await?,
        vec![ProjectionHookLogEntry::Committed(position)],
    );
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    Ok(())
}

/// Verifies exponential retry delays grow from the initial delay and cap at the maximum.
pub async fn exponential_retry_backoff_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_script(&[
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Apply,
    ]);
    fixture.configure_retry_policy(3, Duration::from_millis(10), 2.0, Duration::from_millis(25));

    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(position),
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 4);
    assert_eq!(
        fixture.retry_sleep_requests().await?,
        vec![
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(25),
        ],
    );
    let transaction_tokens = fixture.application_attempt_transaction_tokens().await?;
    assert_eq!(transaction_tokens.len(), 4);
    assert!(
        transaction_tokens
            .iter()
            .enumerate()
            .all(|(index, token)| transaction_tokens[..index].iter().all(|seen| seen != token)),
        "every retry and successful application must use a distinct top-level transaction",
    );
    assert_eq!(fixture.transaction_attempt_row_count().await?, 1);
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?,
        Some(ProjectionProgressObservation {
            source_id: fixture.source_id().clone(),
            selection_id: fixture.selection_id().clone(),
            position,
        }),
    );
    assert_eq!(
        fixture.hook_log().await?,
        vec![ProjectionHookLogEntry::Committed(position)],
    );
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    Ok(())
}

/// Verifies finite multiplier overflow saturates every affected retry at the maximum delay.
pub async fn finite_overflow_retry_backoff_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_script(&[
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Retry,
        ProjectionApplicationBehavior::Apply,
    ]);
    fixture.configure_retry_policy(
        3,
        Duration::from_millis(10),
        f64::MAX,
        Duration::from_millis(25),
    );

    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(position),
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 4);
    assert_eq!(
        fixture.retry_sleep_requests().await?,
        vec![
            Duration::from_millis(10),
            Duration::from_millis(25),
            Duration::from_millis(25),
        ],
        "finite exponential overflow must saturate at the configured maximum",
    );
    let transaction_tokens = fixture.application_attempt_transaction_tokens().await?;
    assert_eq!(transaction_tokens.len(), 4);
    assert!(
        transaction_tokens
            .iter()
            .enumerate()
            .all(|(index, token)| transaction_tokens[..index].iter().all(|seen| seen != token)),
    );
    assert_eq!(fixture.transaction_attempt_row_count().await?, 1);
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(position),
    );
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    Ok(())
}

/// Verifies a retry reloads durable progress before invoking application code again.
pub async fn retry_reloads_progress_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(
        ProjectionApplicationBehavior::RetryWithExternallyCommittedProgress,
    );
    fixture.configure_retry_policy(2, Duration::ZERO, 1.0, Duration::ZERO);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: Some(position),
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 1);
    assert_eq!(fixture.transaction_attempt_row_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(position),
    );
    assert_eq!(fixture.hook_attempt_count().await?, 0);
    Ok(())
}

/// Verifies explicit skip rolls back failed application work before advancing only progress.
pub async fn explicit_skip_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Skip);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 1,
            through: Some(position),
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 1);
    assert_eq!(fixture.transaction_attempt_row_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(position),
    );
    assert!(fixture.hook_log().await?.is_empty());
    assert_eq!(fixture.hook_attempt_count().await?, 0);
    Ok(())
}

/// Verifies stop preserves prior counts while leaving its exact position pending.
pub async fn stop_leaves_position_pending_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let positions = fixture
        .append_values(&[
            Value::Object(Default::default()),
            Value::Object(Default::default()),
            Value::Object(Default::default()),
        ])
        .await?;
    fixture.select_application_script(&[
        ProjectionApplicationBehavior::Apply,
        ProjectionApplicationBehavior::Skip,
        ProjectionApplicationBehavior::Stop,
    ]);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Completed(ProjectionRunOutcome::Stopped {
            position: positions[2],
            processed: 1,
            skipped: 1,
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 3);
    assert_eq!(fixture.transaction_attempt_row_count().await?, 1);
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(positions[1]),
    );
    assert_eq!(
        fixture.hook_log().await?,
        vec![ProjectionHookLogEntry::Committed(positions[0])],
    );
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    Ok(())
}

/// Verifies fatal application failure leaves its exact position pending without a hook.
pub async fn fatal_leaves_position_pending_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Fatal);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::Application {
            position,
        }),
    );
    assert_eq!(fixture.application_attempt_count().await?, 1);
    assert_eq!(fixture.transaction_attempt_row_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(fixture.progress().await?, None);
    assert!(fixture.hook_log().await?.is_empty());
    assert_eq!(fixture.hook_attempt_count().await?, 0);
    Ok(())
}

/// Verifies after-commit observes durable state and never runs for rolled-back work.
pub async fn after_commit_ordering_and_rollback_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let first = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    let _ = fixture.run_batch().await?;
    assert_eq!(
        fixture.hook_log().await?,
        vec![ProjectionHookLogEntry::Committed(first)],
        "the hook may log success only after observing committed effect and progress",
    );

    let _second = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::ApplyThenFail);
    let _ = fixture.run_batch_attempt().await?;
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    assert_eq!(fixture.hook_log().await?.len(), 1);
    Ok(())
}

/// Verifies hook failure reports committed progress and is never replayed on recovery.
pub async fn after_commit_failure_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionFixture,
{
    let position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    fixture.select_application_behavior(ProjectionApplicationBehavior::AfterCommitFail);
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::AfterCommitFailed {
            committed_position: position,
            source: AFTER_COMMIT_FAILURE_SENTINEL.to_owned(),
        }),
    );
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(position),
    );
    assert_eq!(fixture.application_attempt_count().await?, 1);
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    assert!(fixture.hook_log().await?.is_empty());

    fixture.select_application_behavior(ProjectionApplicationBehavior::Apply);
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: Some(position),
        },
    );
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(fixture.application_attempt_count().await?, 1);
    assert_eq!(fixture.hook_attempt_count().await?, 1);
    assert!(fixture.hook_log().await?.is_empty());
    Ok(())
}

/// Verifies that one finite run drains every selected page through its captured frontier.
pub async fn multi_page_batch_drain_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    fixture.configure_batch_size(BatchSize::new(2));
    let positions = fixture
        .append_values(&[
            Value::Object(Default::default()),
            Value::Object(Default::default()),
            Value::Object(Default::default()),
            Value::Object(Default::default()),
            Value::Object(Default::default()),
        ])
        .await?;
    let through = positions[4];
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 5,
            skipped: 0,
            through: Some(through),
        },
        "a finite run must drain all selected pages, not stop after the configured page size",
    );
    assert_eq!(fixture.effect_count().await?, 5);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(through),
    );
    Ok(())
}

/// Verifies finite completion for a source with no committed events.
pub async fn initially_empty_batch_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: None,
        },
    );
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(fixture.progress().await?, None);
    Ok(())
}

/// Verifies finite completion when the global frontier has no selected event.
pub async fn no_match_batch_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let through = fixture.append_unselected().await?;
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 0,
            skipped: 0,
            through: Some(through),
        },
    );
    assert_eq!(fixture.effect_count().await?, 0);
    assert_eq!(fixture.progress().await?, None);
    Ok(())
}

/// Verifies that a trailing unselected global event still bounds finite completion.
pub async fn trailing_unselected_frontier_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let selected_position = fixture
        .append_values(&[Value::Object(Default::default())])
        .await?[0];
    let through = fixture.append_unselected().await?;
    assert!(
        through > selected_position,
        "the unselected event must trail the selected event in the global sequence",
    );
    assert_eq!(
        fixture.run_batch().await?,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(through),
        },
        "an empty bounded page must certify catch-up beyond trailing unselected events",
    );
    assert_eq!(fixture.effect_count().await?, 1);
    assert_eq!(
        fixture.progress().await?.map(|progress| progress.position),
        Some(selected_position),
        "durable progress must remain at the last selected event, below the global frontier",
    );
    Ok(())
}

/// Verifies that a saved cursor is validated even when the source is initially empty.
pub async fn empty_source_identity_validation_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    fixture
        .seed_progress_identity(
            DeliverySourceId::try_new("incompatible-empty-source")
                .expect("contract source identity should be valid"),
            ProjectionSelectionId::try_new("incompatible-empty-selection")
                .expect("contract selection identity should be valid"),
            DeliveryPosition::new(
                std::num::NonZeroU64::new(1).expect("contract position should be positive"),
            ),
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::SourceIdentityMismatch),
        "source identity must be checked first even when no source frontier exists",
    );
    assert_eq!(fixture.application_attempt_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    Ok(())
}

/// Verifies selection-only identity validation when the source is initially empty.
pub async fn empty_source_selection_identity_validation_contract<F>(
    fixture: &mut F,
) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    fixture
        .seed_progress_identity(
            fixture.source_id().clone(),
            ProjectionSelectionId::try_new("incompatible-empty-selection-only")
                .expect("contract selection identity should be valid"),
            DeliveryPosition::new(
                std::num::NonZeroU64::new(1).expect("contract position should be positive"),
            ),
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(
            ProjectionFailureObservation::SelectionIdentityMismatch,
        ),
    );
    assert_eq!(fixture.application_attempt_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    Ok(())
}

/// Verifies identity validation on an empty selected page and deterministic mismatch precedence.
pub async fn no_match_identity_validation_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let position = fixture.append_unselected().await?;
    fixture
        .seed_progress_identity(
            DeliverySourceId::try_new("incompatible-no-match-source")
                .expect("contract source identity should be valid"),
            ProjectionSelectionId::try_new("incompatible-no-match-selection")
                .expect("contract selection identity should be valid"),
            position,
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(ProjectionFailureObservation::SourceIdentityMismatch),
        "source mismatch must win before selection mismatch on an empty selected page",
    );
    assert_eq!(fixture.application_attempt_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    Ok(())
}

/// Verifies selection-only identity validation when the bounded selected page is empty.
pub async fn no_match_selection_identity_validation_contract<F>(
    fixture: &mut F,
) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let position = fixture.append_unselected().await?;
    fixture
        .seed_progress_identity(
            fixture.source_id().clone(),
            ProjectionSelectionId::try_new("incompatible-no-match-selection-only")
                .expect("contract selection identity should be valid"),
            position,
        )
        .await?;
    assert_eq!(
        fixture.run_batch_attempt().await?,
        ProjectionAttemptObservation::Failed(
            ProjectionFailureObservation::SelectionIdentityMismatch,
        ),
    );
    assert_eq!(fixture.application_attempt_count().await?, 0);
    assert_eq!(fixture.effect_count().await?, 0);
    Ok(())
}

/// Verifies exclusion of a second writer while a leader is active.
pub async fn overlapping_leadership_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let observation = fixture.observe_overlapping_leadership().await?;
    assert_eq!(
        observation.competing_failure,
        ProjectionFailureObservation::LeadershipBusy,
    );
    assert!(matches!(
        observation.leader_outcome,
        ProjectionRunOutcome::CaughtUp { processed: 1, .. }
    ));
    Ok(())
}

/// Verifies session fencing after exact leader-backend termination.
pub async fn leadership_loss_fencing_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let observation = fixture.observe_leadership_loss().await?;
    assert_eq!(
        observation.failure,
        ProjectionFailureObservation::LeadershipLost,
    );
    assert_eq!(observation.effect_count, 0, "stale effect must not commit");
    assert_eq!(observation.progress, None, "stale progress must not commit");
    Ok(())
}

/// Verifies that one finite run uses exactly the frontier captured before concurrent append.
pub async fn fixed_high_watermark_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionExecutionFixture,
{
    let observation = fixture.observe_fixed_high_watermark().await?;
    assert_eq!(
        observation.first_outcome,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(observation.initial_position),
        },
    );
    assert_eq!(
        observation
            .progress_after_first
            .map(|progress| progress.position),
        Some(observation.initial_position),
        "the append beyond the captured frontier must remain pending after the first run",
    );
    assert_eq!(
        observation.second_outcome,
        ProjectionRunOutcome::CaughtUp {
            processed: 1,
            skipped: 0,
            through: Some(observation.appended_position),
        },
    );
    Ok(())
}

/// Verifies post-catch-up delivery and normal continuous cancellation.
pub async fn continuous_delivery_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionContinuousFixture,
{
    let observation = fixture.observe_delivery_after_initial_catch_up().await?;
    assert_eq!(
        observation.outcome,
        ProjectionRunOutcome::Cancelled {
            processed: 8,
            skipped: 0,
        },
        "cancellation is a normal outcome after delivering the post-catch-up event",
    );
    let initial_through = observation
        .initial_through
        .expect("continuous multi-page scenario must start with selected events");
    let appended_through = *observation
        .appended_positions
        .last()
        .expect("continuous scenario must append events after initial catch-up");
    assert_eq!(observation.appended_positions.len(), 3);
    assert_eq!(observation.effect_count, 8);
    assert_eq!(
        observation.progress.map(|progress| progress.position),
        Some(appended_through),
    );
    assert_eq!(
        observation.poll_sleep_requests,
        vec![Duration::from_millis(17), Duration::from_millis(17)],
        "continuous mode must await the configured delay exactly at both idle boundaries",
    );
    assert_eq!(observation.idle_boundaries.len(), 2);
    assert_eq!(observation.idle_boundaries[0].effect_count, 5);
    assert_eq!(
        observation.idle_boundaries[0]
            .progress
            .as_ref()
            .map(|progress| progress.position),
        Some(initial_through),
        "the first sleep must not begin between non-empty initial pages",
    );
    assert_eq!(observation.idle_boundaries[1].effect_count, 8);
    assert_eq!(
        observation.idle_boundaries[1]
            .progress
            .as_ref()
            .map(|progress| progress.position),
        Some(appended_through),
        "the second sleep must not begin between non-empty appended pages",
    );
    Ok(())
}

/// Verifies a single pending idle wait and responsive cancellation without a busy loop.
pub async fn continuous_idle_cancellation_contract<F>(fixture: &mut F) -> Result<(), F::Error>
where
    F: TransactionalProjectionContinuousFixture,
{
    let observation = fixture.observe_idle_cancellation().await?;
    assert_eq!(
        observation.outcome,
        ProjectionRunOutcome::Cancelled {
            processed: 0,
            skipped: 0,
        },
    );
    assert_eq!(observation.effect_count, 0);
    assert_eq!(observation.progress, None);
    assert_eq!(observation.initial_through, None);
    assert!(observation.appended_positions.is_empty());
    assert_eq!(
        observation.poll_sleep_requests,
        vec![Duration::from_millis(17)],
        "an idle runner must await exactly one configured poll delay instead of spinning",
    );
    assert_eq!(
        observation.idle_boundaries,
        vec![ProjectionIdleBoundaryObservation {
            effect_count: 0,
            progress: None,
        }],
    );
    Ok(())
}
