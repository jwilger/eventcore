// trybuild pass fixture: proving #[derive(Command)] handles two #[stream] fields.
// Exercised via tests/trybuild.rs.
extern crate eventcore;

use eventcore::{Command, StreamId};

#[derive(Command)]
struct TransferFundsCommand {
    #[stream]
    from: StreamId,

    #[stream]
    to: StreamId,
}

fn main() {
    // Intentionally left empty; macro expansion failure is asserted via trybuild.
}
