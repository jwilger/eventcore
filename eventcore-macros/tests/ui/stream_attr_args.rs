use eventcore::StreamId;
use eventcore_macros::Command;

#[derive(Command)]
struct StreamAttributeArgs {
    #[stream(invalid)]
    account_id: StreamId,
}

fn main() {}
