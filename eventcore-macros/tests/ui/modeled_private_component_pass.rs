extern crate eventcore;

use eventcore::{ModelCommand, ModelInput, StreamId, StreamIdentity, mapping};

#[derive(Clone, StreamIdentity)]
struct PrivateSource(StreamId);

#[derive(ModelInput)]
struct PrivateRequest {
    #[model(origin)]
    source: PrivateSource,
}

#[derive(ModelCommand)]
struct PrivateCommand {
    #[stream]
    source: PrivateSource,
}

mapping! { PrivateSourceMapping: PrivateRequest.source => PrivateCommand.source using clone; }

fn main() {
    let request = PrivateRequest::model_builder()
        .source(PrivateSource(StreamId::try_new("private::stream".to_owned()).unwrap()))
        .build();
    let _command = PrivateCommand::model_builder()
        .source(PrivateSourceMapping::apply(request.as_ref()))
        .build();
}
