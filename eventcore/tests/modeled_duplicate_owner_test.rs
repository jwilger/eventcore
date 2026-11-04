//! Ambiguous declaration labels still fail closed after owner-based resolution.

#![cfg(feature = "experimental-model-check")]

use eventcore::mapping;

mod input {
    use eventcore::ModelInput;
    #[derive(ModelInput)]
    pub(crate) struct Value {
        #[model(origin)]
        pub data: u64,
    }
}
mod output {
    use eventcore::ModelOutput;
    #[derive(ModelOutput)]
    pub(crate) struct Value {
        pub data: u64,
    }
}
mapping! { CopyValue: input::Value.data => output::Value.data using copy; }

#[test]
fn duplicate_field_labels_are_rejected() {
    // Given two actual Rust owners with the same declaration field label.
    let input = input::Value::model_builder().data(7).build();
    let output = output::Value::model_builder()
        .data(CopyValue::apply(input.as_ref()))
        .build();
    assert_eq!(output.as_ref().data, 7);

    // When checking their otherwise executable mapping.
    let error = eventcore::model::check().expect_err("field identity is ambiguous");

    // Then duplicate detection remains authoritative.
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM002")
    );
}
