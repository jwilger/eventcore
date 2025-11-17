use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct TupleCommand(#[stream] StreamId);

fn main() {}
