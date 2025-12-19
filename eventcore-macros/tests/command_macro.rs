use eventcore::{Command, CommandStreams, StreamId};

// Compile-time coverage for #[derive(Command)] lives in tests/trybuild.rs via the
// trybuild harness (https://docs.rs/trybuild). Those UI fixtures deliberately
// include code that fails to compile, so we keep them centralized and leave this
// file focused on a runtime sanity check.
#[derive(Command)]
struct TransferFundsCommand {
    #[stream]
    from: StreamId,

    #[stream]
    to: StreamId,
}

#[test]
fn command_macro_multi_stream_declares_both_streams() {
    let command = TransferFundsCommand {
        from: StreamId::try_new("from".to_owned()).expect("valid stream id"),
        to: StreamId::try_new("to".to_owned()).expect("valid stream id"),
    };

    let declared: Vec<_> = command.stream_declarations().iter().cloned().collect();

    assert_eq!(declared, vec![command.from, command.to]);
}
