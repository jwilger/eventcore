use std::io;
use std::num::NonZeroU64;

use eventcore_types::{DeliveryPosition, DeliverySourceId, ProjectionSelectionId};
use futures::FutureExt;
use serde_json::Value;

use super::{
    ProjectionApplicationBehavior, ProjectionHookLogEntry, ProjectionProgressObservation,
    ProjectionRunMode, ProjectionRunOutcome, TransactionalProjectionFixture,
    transactional_projection_contract,
};

struct ContractFixture {
    source_id: DeliverySourceId,
    selection_id: ProjectionSelectionId,
    position: DeliveryPosition,
    effect_count: u64,
    runs: u64,
    duplicates_on_redelivery: bool,
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

    async fn append_malformed_input(&mut self, _input: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn select_application_behavior(&mut self, _behavior: ProjectionApplicationBehavior) {}

    async fn run_batch(&mut self) -> Result<ProjectionRunOutcome, Self::Error> {
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

    async fn run_continuous(&mut self) -> Result<ProjectionRunOutcome, Self::Error> {
        Ok(ProjectionRunOutcome::Cancelled {
            processed: 0,
            skipped: 0,
        })
    }

    async fn inject_progress_failure(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn inject_connection_loss(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn effect_count(&self) -> Result<u64, Self::Error> {
        Ok(self.effect_count)
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

    async fn start_leadership_attempt(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn lose_leadership(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn reset(&mut self) -> Result<(), Self::Error> {
        self.effect_count = 0;
        Ok(())
    }

    fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    fn selection_id(&self) -> &ProjectionSelectionId {
        &self.selection_id
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

// Break caught: deleting either public mode would make backend contract fixtures unable to state
// whether they are exercising finite catch-up or cancellation-driven continuous behavior.
#[test]
fn projection_run_modes_remain_public_behavior_controls() {
    assert_eq!(ProjectionRunMode::Batch, ProjectionRunMode::Batch);
    assert_eq!(ProjectionRunMode::Continuous, ProjectionRunMode::Continuous);
}
