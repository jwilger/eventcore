//! Source alias spelling must not supply a different modeled root.

#![cfg(feature = "experimental-model-check")]

use eventcore::{ModelInput, ModelOutput, mapping};

#[derive(ModelInput)]
pub struct Root {
    #[model(origin)]
    pub value: u64,
}
mod aliases {
    pub(crate) use super::Left as Root;
}

#[derive(ModelOutput)]
pub struct Left {
    pub value: u64,
}
#[derive(ModelOutput)]
pub struct Right {
    pub value: u64,
}

mapping! { LeftToRight: aliases::Root.value => Right.value using copy; }
mapping! { RightToLeft: Right.value => Left.value using copy; }

#[test]
fn source_alias_does_not_root_an_ordinary_cycle() {
    // Given an unrelated root whose declaration name matches an alias for Left.
    let unrelated = Root::model_builder().value(7).build();
    assert_eq!(unrelated.as_ref().value, 7);

    // When the checker follows executable Left -> Right -> Left mappings.
    let error = eventcore::model::check().expect_err("the cycle has no root");

    // Then it rejects the actual cycle, rather than verifying or losing the alias.
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM006")
    );
    assert!(
        !error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM004")
    );
}
