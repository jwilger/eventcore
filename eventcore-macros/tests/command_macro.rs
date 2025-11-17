use eventcore::{CommandStreams, StreamId};
use eventcore_macros::Command;
use trybuild::TestCases;

#[derive(Command)]
struct TransferFundsCommand {
    #[stream]
    from: StreamId,

    #[stream]
    to: StreamId,
}

#[test]
fn command_macro_generates_stream_declarations() {
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
fn command_macro_accepts_custom_stream_field_name() {
    let t = TestCases::new();
    t.pass("tests/ui/single_stream_account.rs");
}

#[test]
fn command_macro_accepts_multiple_stream_fields() {
    let t = TestCases::new();
    t.pass("tests/ui/multi_stream_pass.rs");
}

#[test]
fn command_macro_multi_stream_declares_both_streams() {
    let command = TransferFundsCommand {
        from: StreamId::try_new("from".to_owned()).expect("valid stream id"),
        to: StreamId::try_new("to".to_owned()).expect("valid stream id"),
    };

    assert_eq!(2, command.stream_declarations().len());
}
