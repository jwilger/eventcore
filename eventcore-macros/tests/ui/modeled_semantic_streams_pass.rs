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
    #[model(origin)]
    target: Target,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    source: Source,
    #[stream]
    target: Target,
}

mapping! { SourceMapping: Request.source => Transfer.source using clone; }
mapping! { TargetMapping: Request.target => Transfer.target using clone; }

fn main() {
    let request = Request::model_builder()
        .source(Source(StreamId::try_new("accounts::source".to_owned()).unwrap()))
        .target(Target(StreamId::try_new("accounts::target".to_owned()).unwrap()))
        .build();
    let _command = Transfer::model_builder()
        .source(SourceMapping::apply(request.as_ref()))
        .target(TargetMapping::apply(request.as_ref()))
        .build();
}
