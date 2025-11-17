extern crate eventcore;

use eventcore::StreamId;
use eventcore_macros::Command;

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
