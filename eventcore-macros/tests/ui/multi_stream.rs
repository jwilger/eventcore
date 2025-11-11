use std::collections::HashSet;

use eventcore::{CommandStreams, StreamId};
use eventcore_macros::Command;

#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount_cents: u64,
}

fn main() {
    let from_account = StreamId::try_new("accounts::primary").expect("valid stream id");
    let to_account = StreamId::try_new("accounts::secondary").expect("valid stream id");

    let command = TransferMoney {
        from_account: from_account.clone(),
        to_account: to_account.clone(),
        amount_cents: 500,
    };

    let declarations = command.stream_declarations();
    assert_eq!(declarations.len(), 2);

    let streams: HashSet<StreamId> = declarations.iter().cloned().collect();
    assert_eq!(streams.len(), 2);
    assert!(streams.contains(&from_account));
    assert!(streams.contains(&to_account));
}
