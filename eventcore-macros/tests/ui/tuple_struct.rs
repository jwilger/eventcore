// trybuild compile-fail fixture: tuple structs are not supported by #[derive(Command)].
// Exercised via tests/trybuild.rs.
use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct TupleCommand(#[stream] StreamId);

fn main() {}
