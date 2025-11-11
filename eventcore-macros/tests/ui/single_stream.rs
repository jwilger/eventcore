use eventcore::{CommandStreams, StreamId};
use eventcore_macros::Command;

#[derive(Command)]
struct OpenAccount {
    #[stream]
    account_id: StreamId,
    owner_name: String,
}

fn main() {
    let account_id = StreamId::try_new("accounts::primary").expect("valid stream id");
    let command = OpenAccount {
        account_id: account_id.clone(),
        owner_name: "Alice".to_owned(),
    };

    let declarations = command.stream_declarations();
    assert_eq!(declarations.len(), 1);

    let collected: Vec<StreamId> = declarations.iter().cloned().collect();
    assert_eq!(collected, vec![account_id]);
}
