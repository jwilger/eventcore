extern crate eventcore;

use eventcore::{ModelInput, ModelState};

#[derive(ModelInput)]
struct Session {
    #[model(origin = actor)]
    actor_id: String,
}

#[derive(ModelState)]
struct State {
    #[model(absence)]
    last_seen: Option<u64>,
    #[model(constant)]
    attempts: u64,
}

fn main() {
    let session = Session::model_builder().actor_id("a-1".to_owned()).build();
    assert_eq!(session.as_ref().actor_id, "a-1");
    let state = <eventcore::model::Modeled<State> as Default>::default();
    assert_eq!(state.as_ref().last_seen, None);
    assert_eq!(state.as_ref().attempts, 0);
}
