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
