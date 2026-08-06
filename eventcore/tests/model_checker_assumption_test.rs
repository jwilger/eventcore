#![cfg(feature = "experimental-model-check")]

use eventcore::{
    ModelReadModel,
    model::{CheckOptions, CheckStatus},
};

#[derive(ModelReadModel)]
struct NativeReadModel {
    #[model(assumption = "native-sql-account-history")]
    balance: u64,
}

#[test]
fn strict_check_rejects_a_named_assumption_boundary() {
    let model = NativeReadModel::model_builder()
        .balance(eventcore::model::FieldValue::from_value(1))
        .build();
    assert_eq!(model.as_ref().balance, 1);

    let error = eventcore::model::check().expect_err("strict checks reject assumptions");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM008")
    );
}

#[test]
fn opted_in_named_assumption_reports_assumed() {
    let report = eventcore::model::check_with(
        CheckOptions::default().allow_assumption("native-sql-account-history"),
    )
    .expect("the explicitly accepted boundary is allowed");

    assert_eq!(report.status, CheckStatus::Assumed);
}
