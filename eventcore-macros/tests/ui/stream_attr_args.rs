// trybuild compile-fail fixture: #[stream] rejects arguments.
// Exercised via tests/trybuild.rs.
use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct StreamAttributeArgs {
    #[stream(invalid)]
    account_id: StreamId,
}

fn main() {}
