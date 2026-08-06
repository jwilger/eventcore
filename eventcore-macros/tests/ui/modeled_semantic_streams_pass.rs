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
    #[model(origin)]
    amount: u64,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    source: Source,
    #[stream]
    target: Target,
    amount: u64,
}

mapping! { SourceMapping: Request.source => Transfer.source using clone; }
mapping! { TargetMapping: Request.target => Transfer.target using clone; }
mapping! { AmountMapping: Request.amount => Transfer.amount using copy; }

fn main() {
    let request = Request::model_builder()
        .source(Source(StreamId::try_new("accounts::source".to_owned()).unwrap()))
        .target(Target(StreamId::try_new("accounts::target".to_owned()).unwrap()))
        .amount(42)
        .build();
    let _command = Transfer::model_builder()
        .source(SourceMapping::apply(request.as_ref()))
        .target(TargetMapping::apply(request.as_ref()))
        .amount(AmountMapping::apply(request.as_ref()))
        .build();
}
