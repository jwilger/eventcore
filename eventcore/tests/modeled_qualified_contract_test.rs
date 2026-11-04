//! Qualified public contracts retain runtime behavior and strict provenance.

#![cfg(feature = "experimental-model-check")]

use eventcore::{ModelInput, ModelOutput, mapping, model::CheckStatus};
use eventcore_model_contract_fixture as contracts;

#[derive(ModelInput)]
struct Request {
    #[model(origin)]
    amount: u64,
}

#[derive(ModelOutput)]
struct Response {
    amount: u64,
}

mod aliases {
    pub(crate) use eventcore_model_contract_fixture::RecordedAmount as AcceptedAmount;
}

#[derive(Debug, PartialEq, thiserror::Error)]
#[error("amount must be nonzero")]
struct ZeroAmount;

fn require_nonzero(amount: &u64) -> Result<u64, ZeroAmount> {
    if *amount == 0 {
        return Err(ZeroAmount);
    }
    Ok(*amount)
}

mapping! {
    AcceptAmount: Request.amount => contracts::RecordedAmount.amount
        using try require_nonzero, error = ZeroAmount;
}
mapping! { Respond: aliases::AcceptedAmount.amount => Response.amount using copy; }
mapping! { Audit: contracts::RecordedAmount.amount => contracts::AuditEvent.Recorded using copy; }

#[test]
fn qualified_contracts_preserve_values_errors_and_strict_provenance() {
    // Given an input and public event contracts defined in another crate.
    let request = Request::model_builder().amount(7).build();

    // When qualified mappings construct the event, response, and enum payload.
    let recorded = contracts::RecordedAmount::model_builder()
        .amount(AcceptAmount::apply(request.as_ref()).expect("positive input"))
        .build();
    let response = Response::model_builder()
        .amount(Respond::apply(recorded.as_ref()))
        .build();
    let audit = contracts::AuditEvent::model_variant_recorded(Audit::apply(recorded.as_ref()));

    // Then executable values and typed rejection match the strictly verified model.
    assert_eq!(response.as_ref().amount, 7);
    assert!(matches!(audit.as_ref(), contracts::AuditEvent::Recorded(7)));
    let invalid = Request::model_builder().amount(0).build();
    assert!(matches!(
        AcceptAmount::apply(invalid.as_ref()),
        Err(ZeroAmount)
    ));
    assert_eq!(
        eventcore::model::check().expect("complete model").status,
        CheckStatus::Verified
    );
}
