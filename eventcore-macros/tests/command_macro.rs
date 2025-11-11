use trybuild::TestCases;

#[test]
fn command_macro_generates_stream_declarations() {
    let t = TestCases::new();
    t.pass("tests/ui/single_stream.rs");
    t.pass("tests/ui/multi_stream.rs");
    t.compile_fail("tests/ui/missing_stream_attribute.rs");
}
