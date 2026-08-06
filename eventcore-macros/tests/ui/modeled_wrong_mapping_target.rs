extern crate eventcore;

use eventcore::{ModelCommand, ModelInput, StreamId, mapping};

#[derive(ModelInput)]
struct Request {
    #[model(origin)]
    amount: u64,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    stream: StreamId,
}

mapping! { WrongTarget: Request.amount => Transfer.stream using clone; }

fn main() {}
