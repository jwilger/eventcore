extern crate eventcore;

use eventcore::ModelEvent;

#[derive(ModelEvent)]
enum Event {
    Transfer(u64, u64),
}

fn main() {}
