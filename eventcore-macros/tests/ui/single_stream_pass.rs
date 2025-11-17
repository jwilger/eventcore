extern crate eventcore;

use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct PingCommand {
    #[stream]
    stream: StreamId,
}

fn main() {
    // Intentionally left empty; macro expansion failure is asserted via trybuild.
}
