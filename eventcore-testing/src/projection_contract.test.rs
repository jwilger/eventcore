use std::collections::VecDeque;
use std::io;
use std::num::NonZeroU64;
use std::time::Duration;

use eventcore_types::{DeliveryPosition, DeliverySourceId, ProjectionSelectionId};
use futures::FutureExt;
use serde_json::Value;

use super::{
    ProjectionApplicationBehavior, ProjectionAttemptObservation, ProjectionHookLogEntry,
    ProjectionProgressObservation, ProjectionRunOutcome, TransactionalProjectionFixture,
    transactional_projection_contract,
};

struct ContractFixture {
    source_id: DeliverySourceId,
    selection_id: ProjectionSelectionId,
    position: DeliveryPosition,
    effect_count: u64,
    runs: u64,
    duplicates_on_redelivery: bool,
    behaviors: VecDeque<ProjectionApplicationBehavior>,
}

impl ContractFixture {
    fn new(duplicates_on_redelivery: bool) -> Self {
        Self {
            source_id: DeliverySourceId::try_new("fixture-source").expect("valid source ID"),
            selection_id: ProjectionSelectionId::try_new("fixture-selection")
                .expect("valid selection ID"),
            position: DeliveryPosition::new(NonZeroU64::new(7).expect("positive position")),
            effect_count: 0,
            runs: 0,
            duplicates_on_redelivery,
            behaviors: VecDeque::new(),
        }
    }
}

impl TransactionalProjectionFixture for ContractFixture {
    type Error = io::Error;

    async fn append_values(
        &mut self,
        values: &[Value],
    ) -> Result<Vec<DeliveryPosition>, Self::Error> {
        Ok(values.iter().map(|_| self.position).collect())
    }

    async fn append_malformed_input(
        &mut self,
        _input: &str,
    ) -> Result<DeliveryPosition, Self::Error> {
        Ok(self.position)
    }

    fn select_application_behavior(&mut self, behavior: ProjectionApplicationBehavior) {
        self.behaviors = VecDeque::from([behavior]);
    }

    fn select_application_script(&mut self, behaviors: &[ProjectionApplicationBehavior]) {
        self.behaviors = behaviors.iter().copied().collect();
    }

    fn configure_retry_policy(
        &mut self,
        _max_retries: u32,
        _initial_delay: Duration,
        _multiplier: f64,
        _maximum_delay: Duration,
    ) {
    }

    async fn run_batch(&mut self) -> Result<ProjectionRunOutcome, Self::Error> {
        assert_eq!(
            self.behaviors.front().copied(),
            Some(ProjectionApplicationBehavior::Apply),
            "the focused contract fixture only models successful application",
        );
        self.runs += 1;
        if self.runs == 1 || self.duplicates_on_redelivery {
            self.effect_count += 1;
        }
        Ok(ProjectionRunOutcome::CaughtUp {
            processed: u64::from(self.runs == 1),
            skipped: 0,
            through: Some(self.position),
        })
    }

    async fn run_batch_attempt(&mut self) -> Result<ProjectionAttemptObservation, Self::Error> {
        Ok(ProjectionAttemptObservation::Completed(
            self.run_batch().await?,
        ))
    }

    async fn inject_progress_failure(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn inject_commit_acknowledgement_loss(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn recover_after_commit_acknowledgement_loss(
        &mut self,
    ) -> Result<ProjectionRunOutcome, Self::Error> {
        self.run_batch().await
    }

    async fn effect_count(&self) -> Result<u64, Self::Error> {
        Ok(self.effect_count)
    }

    async fn application_attempt_count(&self) -> Result<u64, Self::Error> {
        Ok(self.runs)
    }

    async fn application_attempt_transaction_tokens(&self) -> Result<Vec<String>, Self::Error> {
        Ok(Vec::new())
    }

    async fn retry_sleep_requests(&self) -> Result<Vec<Duration>, Self::Error> {
        Ok(Vec::new())
    }

    async fn transaction_attempt_row_count(&self) -> Result<u64, Self::Error> {
        Ok(0)
    }

    async fn progress(&self) -> Result<Option<ProjectionProgressObservation>, Self::Error> {
        Ok(Some(ProjectionProgressObservation {
            source_id: self.source_id.clone(),
            selection_id: self.selection_id.clone(),
            position: self.position,
        }))
    }

    async fn hook_log(&self) -> Result<Vec<ProjectionHookLogEntry>, Self::Error> {
        Ok(Vec::new())
    }

    async fn hook_attempt_count(&self) -> Result<u64, Self::Error> {
        Ok(0)
    }

    fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    fn selection_id(&self) -> &ProjectionSelectionId {
        &self.selection_id
    }

    async fn seed_progress_identity(
        &mut self,
        _source_id: DeliverySourceId,
        _selection_id: ProjectionSelectionId,
        _position: DeliveryPosition,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

// Break caught: weakening the shared contract's redelivery assertion would permit a fixture to
// report a duplicate non-idempotent effect while appearing to satisfy the atomicity suite.
#[tokio::test]
async fn transactional_projection_contract_rejects_duplicate_effects_on_redelivery() {
    let mut conforming = ContractFixture::new(false);
    transactional_projection_contract(&mut conforming)
        .await
        .expect("a fixture that suppresses redelivery should satisfy the contract");

    let mut duplicate = ContractFixture::new(true);
    let result = std::panic::AssertUnwindSafe(transactional_projection_contract(&mut duplicate))
        .catch_unwind()
        .await;
    assert!(
        result.is_err(),
        "duplicate redelivery must fail the shared contract"
    );
}
