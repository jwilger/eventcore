extern crate eventcore;

use eventcore::ModelInput;

#[derive(ModelInput)]
struct Request {
    #[model(origin)]
    amount: u64,
    #[model(origin)]
    note: String,
}

fn main() {
    let _ = Request::model_builder().amount(42).build();
}
