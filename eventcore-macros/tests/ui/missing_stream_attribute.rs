use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct MissingStreamAttribute {
    account_id: StreamId,
}

fn main() {}
