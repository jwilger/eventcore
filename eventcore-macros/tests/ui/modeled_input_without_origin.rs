extern crate eventcore;

use eventcore::ModelInput;

#[derive(ModelInput)]
struct Payload {
    amount: u64,
}

fn main() {
    let _ = Payload::model_builder().amount(42);
}
