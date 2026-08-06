extern crate eventcore;

use eventcore::ModelEvent;

#[derive(ModelEvent)]
enum Event {
    Opened,
    Deposited(u64),
}

fn main() {}
