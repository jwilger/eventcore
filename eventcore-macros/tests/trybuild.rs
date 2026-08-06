use trybuild::TestCases;

#[test]
fn command_macro_single_stream_initial_red() {
    // Initial red test: the derive macro should compile successfully once implemented.
    let t = TestCases::new();
    t.pass("tests/ui/single_stream_pass.rs");
}

#[test]
fn command_macro_missing_stream_attribute_produces_error() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/missing_stream_attribute.rs");
}

#[test]
fn command_macro_rejects_tuple_struct_stream_field() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/tuple_struct.rs");
}

#[test]
fn command_macro_rejects_wrong_stream_field_type() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/wrong_stream_type.rs");
}

#[test]
fn command_macro_rejects_stream_attribute_args() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/stream_attr_args.rs");
}

#[test]
fn command_macro_single_stream_allows_account_id_field() {
    let t = TestCases::new();
    t.pass("tests/ui/single_stream_account.rs");
}

#[test]
fn command_macro_multi_stream_should_compile() {
    let t = TestCases::new();
    t.pass("tests/ui/multi_stream_pass.rs");
}

#[test]
fn modeled_input_requires_an_explicit_origin_for_raw_values() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_input_without_origin.rs");
}

#[test]
fn modeled_events_allow_unit_and_single_payload_variants() {
    let t = TestCases::new();
    t.pass("tests/ui/modeled_event_enum_pass.rs");
}

#[test]
fn modeled_events_reject_multi_field_tuple_variants() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_event_multi_payload.rs");
}

#[test]
fn modeled_semantic_streams_and_mappings_compile() {
    let t = TestCases::new();
    t.pass("tests/ui/modeled_semantic_streams_pass.rs");
}

#[test]
fn modeled_mapping_rejects_swapped_semantic_roles() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_swapped_semantic_roles.rs");
}

#[test]
fn modeled_mapping_rejects_wrong_target_occurrence() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_wrong_mapping_target.rs");
}

#[test]
fn modeled_builder_rejects_missing_fields() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_missing_builder_field.rs");
}

#[test]
fn modeled_named_origins_and_state_root_recipes_compile() {
    let t = TestCases::new();
    t.pass("tests/ui/modeled_root_recipes_pass.rs");
}

#[test]
fn private_modeled_components_do_not_leak_field_types() {
    let t = TestCases::new();
    t.pass("tests/ui/modeled_private_component_pass.rs");
}

#[test]
fn modeled_fallible_mapping_compiles() {
    let t = TestCases::new();
    t.pass("tests/ui/modeled_fallible_mapping_pass.rs");
}

#[test]
fn modeled_mapping_rejects_an_incompatible_transform_signature() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_wrong_transform_signature.rs");
}

#[test]
fn modeled_components_reject_generic_owners() {
    let t = TestCases::new();
    t.compile_fail("tests/ui/modeled_generic_component.rs");
}
