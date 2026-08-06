#![cfg(feature = "experimental-model-check")]

use eventcore::ModelReadModel;

/// A checker failure must be reported from the linked runtime model, not from
/// a separate specification language.
#[derive(ModelReadModel)]
struct UnproducedBalance {
    balance: u64,
}

#[test]
fn checker_reports_a_non_root_field_without_an_executable_producer() {
    let model = UnproducedBalance::model_builder()
        .balance(eventcore::model::FieldValue::from_value(1))
        .build();
    assert_eq!(model.as_ref().balance, 1);

    let error = eventcore::model::check().expect_err("the field has no mapping or root recipe");

    assert!(error
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ECM003" && diagnostic.subject.contains("balance")));
}
