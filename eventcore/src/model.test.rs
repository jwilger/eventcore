use super::*;

#[cfg(feature = "experimental-model-check")]
use std::collections::BTreeMap;

#[derive(Debug)]
struct InitialState;

impl ModelState for InitialState {
    fn initial() -> Modeled<Self> {
        Modeled::from_built(Self)
    }
}

#[test]
fn modeled_state_default_uses_model_initialization() {
    let state = Modeled::<InitialState>::default();

    assert!(matches!(state.into_inner(), InitialState));
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn temporal_recurrence_requires_a_non_temporal_seed() {
    static INPUT: Descriptor = Descriptor::field("input", "Input.amount", true);
    static BALANCE: Descriptor = Descriptor::field("read_model", "History.balance", false);
    static RECURRENCE: Descriptor = Descriptor::mapping(
        "CreditBalance",
        &["History.balance", "Input.amount"],
        "History.balance",
        &[true, false],
    );

    let fields = BTreeMap::from([("Input.amount", &INPUT), ("History.balance", &BALANCE)]);
    let mappings = BTreeMap::from([("History.balance", vec![&RECURRENCE])]);
    let assumptions = BTreeMap::new();
    let options = CheckOptions::default();
    let graph = CheckerGraph {
        fields: &fields,
        mappings: &mappings,
        assumptions: &assumptions,
        options: &options,
    };
    let mut evaluation = CheckerEvaluation::default();

    assert!(!is_complete("History.balance", &graph, &mut evaluation));
    assert!(evaluation.errors.iter().any(|error| error.code == "ECM007"));
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn temporal_recurrence_accepts_a_non_temporal_seed() {
    static INPUT: Descriptor = Descriptor::field("input", "Input.amount", true);
    static BALANCE: Descriptor = Descriptor::field("read_model", "History.balance", false);
    static RECURRENCE: Descriptor = Descriptor::mapping(
        "CreditBalance",
        &["History.balance", "Input.amount"],
        "History.balance",
        &[true, false],
    );
    static SEED: Descriptor = Descriptor::mapping(
        "InitialBalance",
        &["Input.amount"],
        "History.balance",
        &[false],
    );

    let fields = BTreeMap::from([("Input.amount", &INPUT), ("History.balance", &BALANCE)]);
    let mappings = BTreeMap::from([("History.balance", vec![&RECURRENCE, &SEED])]);
    let assumptions = BTreeMap::new();
    let options = CheckOptions::default();
    let graph = CheckerGraph {
        fields: &fields,
        mappings: &mappings,
        assumptions: &assumptions,
        options: &options,
    };
    let mut evaluation = CheckerEvaluation::default();

    assert!(is_complete("History.balance", &graph, &mut evaluation));
    assert!(evaluation.errors.is_empty());
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn explicit_descriptor_checks_retain_duplicate_registration_errors() {
    let descriptors = [
        Descriptor::field("input", "Input.amount", true),
        Descriptor::field("input", "Input.amount", true),
    ];

    let error = check_descriptors(&descriptors, CheckOptions::default())
        .expect_err("duplicate descriptors must not be discarded during evaluation");
    assert!(error.diagnostics.iter().any(|error| error.code == "ECM002"));
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn multi_input_mapping_requires_every_source_and_every_alternative() {
    let complete = [
        Descriptor::field("input", "Input.left", true),
        Descriptor::field("input", "Input.right", true),
        Descriptor::field("output", "Output.sum", false),
        Descriptor::mapping(
            "Sum",
            &["Input.left", "Input.right"],
            "Output.sum",
            &[false, false],
        ),
    ];
    assert_eq!(
        check_descriptors(&complete, CheckOptions::default())
            .expect("both sources make the AND-edge complete")
            .status,
        CheckStatus::Verified
    );

    let incomplete_alternative = [
        Descriptor::field("input", "Input.left", true),
        Descriptor::field("output", "Output.sum", false),
        Descriptor::mapping("Good", &["Input.left"], "Output.sum", &[false]),
        Descriptor::mapping("Broken", &["Missing.right"], "Output.sum", &[false]),
    ];
    let error = check_descriptors(&incomplete_alternative, CheckOptions::default())
        .expect_err("all registered producer alternatives must be complete");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM004")
    );
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_reports_empty_registry_and_unused_boundaries() {
    let empty = check_descriptors(&[], CheckOptions::default())
        .expect_err("an empty explicit descriptor set is not a model");
    assert_eq!(empty.status(), CheckFailureStatus::Incomplete);
    assert_eq!(empty.diagnostics[0].code, "ECM001");

    let descriptors = [
        Descriptor::field("input", "Input.used", true),
        Descriptor::field("input", "Input.unused", true),
        Descriptor::field("event", "Event.value", false),
        Descriptor::field("output", "Output.value", false),
        Descriptor::mapping("EventFromInput", &["Input.used"], "Event.value", &[false]),
        Descriptor::mapping("OutputFromInput", &["Input.used"], "Output.value", &[false]),
    ];
    let report = check_descriptors(&descriptors, CheckOptions::default())
        .expect("the output remains complete even with unused boundaries");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.code == "ECM102")
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.code == "ECM103")
    );
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_rejects_registrations_for_unknown_targets() {
    let descriptors = [
        Descriptor::field("input", "Input.value", true),
        Descriptor::mapping(
            "UnknownMappingTarget",
            &["Input.value"],
            "Missing.mapped",
            &[false],
        ),
        Descriptor::assumption("unknown-sink", "Missing.assumed"),
    ];

    let error = check_descriptors(&descriptors, CheckOptions::default())
        .expect_err("every mapping and assumption target must be registered");

    assert_eq!(error.status(), CheckFailureStatus::Incomplete);
    assert_eq!(
        error
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "ECM005")
            .map(|diagnostic| diagnostic.subject.as_str())
            .collect::<Vec<_>>(),
        vec!["UnknownMappingTarget", "unknown-sink"],
    );
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_warns_only_for_actual_unused_boundaries() {
    let complete = [
        Descriptor::field("input", "Input.value", true),
        Descriptor::field("command", "Command.unused", false),
        Descriptor::field("event", "Event.consumed", false),
        Descriptor::field("output", "Output.value", false),
        Descriptor::mapping(
            "CommandFromInput",
            &["Input.value"],
            "Command.unused",
            &[false],
        ),
        Descriptor::mapping(
            "EventFromInput",
            &["Input.value"],
            "Event.consumed",
            &[false],
        ),
        Descriptor::mapping(
            "OutputFromEvent",
            &["Event.consumed"],
            "Output.value",
            &[false],
        ),
    ];
    let report = check_descriptors(&complete, CheckOptions::default())
        .expect("complete, consumed boundaries should not warn");
    assert!(report.warnings.is_empty());
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_accepts_allow_all_assumptions_and_marks_the_result_assumed() {
    let descriptors = [
        Descriptor::field("output", "Output.value", false),
        Descriptor::assumption("native-sink", "Output.value"),
    ];
    let report = check_descriptors(&descriptors, CheckOptions::default().allow_assumptions())
        .expect("an explicitly allowed assumption is a valid boundary");
    assert_eq!(report.status, CheckStatus::Assumed);
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_treats_explicit_roots_as_complete_but_not_option_or_collections() {
    let rooted = [
        Descriptor::field("input", "Request.actor", true),
        Descriptor::field("state", "History.default_balance", true),
        Descriptor::field("state", "History.absent_note", true),
        Descriptor::field("output", "View.balance", false),
        Descriptor::mapping(
            "RenderBalance",
            &[
                "Request.actor",
                "History.default_balance",
                "History.absent_note",
            ],
            "View.balance",
            &[false, false, false],
        ),
    ];
    assert_eq!(
        check_descriptors(&rooted, CheckOptions::default())
            .expect("explicit origins and state recipes are roots")
            .status,
        CheckStatus::Verified
    );

    let implicit_container_values = [
        Descriptor::field("read_model", "History.optional_note", false),
        Descriptor::field("read_model", "History.items", false),
    ];
    let error = check_descriptors(&implicit_container_values, CheckOptions::default())
        .expect_err("Option and collections are not implicit roots");
    assert_eq!(
        error
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "ECM003")
            .count(),
        2
    );
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_rejects_ordinary_cycles_and_sorts_unresolved_diagnostics() {
    let cycle = [
        Descriptor::field("read_model", "History.left", false),
        Descriptor::field("read_model", "History.right", false),
        Descriptor::mapping(
            "LeftFromRight",
            &["History.right"],
            "History.left",
            &[false],
        ),
        Descriptor::mapping(
            "RightFromLeft",
            &["History.left"],
            "History.right",
            &[false],
        ),
    ];
    let cycle_error = check_descriptors(&cycle, CheckOptions::default())
        .expect_err("non-temporal cycles have no seed");
    assert!(
        cycle_error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ECM006")
    );

    let unresolved = [
        Descriptor::field("output", "View.a", false),
        Descriptor::field("output", "View.b", false),
        Descriptor::mapping("Zed", &["Missing.z"], "View.a", &[false]),
        Descriptor::mapping("Alpha", &["Missing.a"], "View.b", &[false]),
    ];
    let forward = check_descriptors(&unresolved, CheckOptions::default())
        .expect_err("unknown sources are rejected");
    let reverse_descriptors = [
        Descriptor::mapping("Alpha", &["Missing.a"], "View.b", &[false]),
        Descriptor::mapping("Zed", &["Missing.z"], "View.a", &[false]),
        Descriptor::field("output", "View.b", false),
        Descriptor::field("output", "View.a", false),
    ];
    let reverse = check_descriptors(&reverse_descriptors, CheckOptions::default())
        .expect_err("registration order cannot affect diagnostics");
    assert_eq!(forward.diagnostics, reverse.diagnostics);
    assert_eq!(
        forward
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.subject.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Zed"]
    );
}
