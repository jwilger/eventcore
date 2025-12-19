// trybuild pass fixture: ensures #[derive(Command)] supports arbitrary field names.
// Exercised via tests/trybuild.rs.
extern crate eventcore;

use eventcore::{Command, StreamId};

#[derive(Command)]
struct CreateAccountCommand {
    #[stream]
    account_id: StreamId,
}

fn main() {
    // Intentionally left empty; macro expansion failure is asserted via trybuild.
}
