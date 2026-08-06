extern crate eventcore;

use eventcore::{ModelCommand, ModelInput, StreamId, StreamIdentity, mapping};

#[derive(Clone, StreamIdentity)]
pub struct Source(StreamId);

#[derive(Clone, StreamIdentity)]
pub struct Target(StreamId);

#[derive(ModelInput)]
struct Request {
    #[model(origin)]
    source: Source,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    target: Target,
}

mapping! { WrongRole: Request.source => Transfer.target using clone; }

fn main() {}
