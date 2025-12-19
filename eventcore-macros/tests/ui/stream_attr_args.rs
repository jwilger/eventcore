// trybuild compile-fail fixture: #[stream] rejects arguments.
// Exercised via tests/trybuild.rs.
use eventcore::{Command, StreamId};

#[derive(Command)]
struct StreamAttributeArgs {
    #[stream(invalid)]
    account_id: StreamId,
}

fn main() {}
