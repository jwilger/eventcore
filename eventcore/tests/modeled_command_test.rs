#![cfg(feature = "experimental-modeling")]

use eventcore::{
    CommandLogic, CommandStreams, Event, ModelCommand, ModelEvent, ModelInput, ModelState,
    StreamId, StreamIdentity, mapping,
    model::{ModelCommandLogic, Modeled, ModeledEvents},
};

#[cfg(feature = "experimental-model-check")]
use eventcore::model::check;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, StreamIdentity)]
pub struct TransferSource(StreamId);

#[derive(Debug, Clone, PartialEq, Eq, StreamIdentity)]
pub struct TransferTarget(StreamId);

#[derive(ModelInput)]
struct TransferRequest {
    #[model(origin)]
    source: TransferSource,
    #[model(origin)]
    target: TransferTarget,
    #[model(origin)]
    amount: u64,
}

#[derive(ModelCommand)]
struct Transfer {
    #[stream]
    source: TransferSource,
    #[stream]
    target: TransferTarget,
    amount: u64,
}

mapping! {
    RequestAmountToTransfer:
        TransferRequest.amount => Transfer.amount
        using clone;
}

mapping! {
    RequestSourceToTransfer:
        TransferRequest.source => Transfer.source
        using clone;
}

mapping! {
    RequestTargetToTransfer:
        TransferRequest.target => Transfer.target
        using clone;
}

#[derive(Debug, Clone, Serialize, Deserialize, ModelEvent)]
struct BankEvent {
    stream_id: StreamId,
}

impl Event for BankEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "bank-event"
    }
}

fn source_stream(source: &TransferSource) -> StreamId {
    eventcore::model::StreamIdentity::as_stream_id(source).clone()
}

mapping! {
    TransferSourceToEvent:
        Transfer.source => BankEvent.stream_id
        using source_stream;
}

#[derive(ModelState)]
struct TransferState {
    #[model(default)]
    processed: bool,
}

impl ModelCommandLogic for Transfer {
    type Event = BankEvent;
    type State = TransferState;

    fn evolve(&self, state: Modeled<Self::State>, _event: &Self::Event) -> Modeled<Self::State> {
        let state = state.into_inner();
        let _processed = state.processed;
        Modeled::from_built(state)
    }

    fn decide(
        &self,
        _state: Modeled<Self::State>,
    ) -> Result<ModeledEvents<Self::Event>, eventcore::CommandError> {
        if eventcore::model::StreamIdentity::as_stream_id(&self.source)
            == eventcore::model::StreamIdentity::as_stream_id(&self.target)
        {
            return Err("transfer source and target must differ".into());
        }
        let _event_stream = TransferSourceToEvent::apply(self);
        let _amount = self.amount;
        Ok(ModeledEvents::none("example does not emit events"))
    }
}

fn stream(value: &str) -> StreamId {
    StreamId::try_new(value.to_owned()).expect("valid stream id")
}

#[test]
fn equal_semantic_stream_ids_deduplicate_and_return_the_domain_error() {
    let request = TransferRequest::model_builder()
        .source(TransferSource(stream("accounts::same")))
        .target(TransferTarget(stream("accounts::same")))
        .amount(42)
        .build();
    assert_eq!(request.as_ref().amount, 42);

    let command = Transfer::model_builder()
        .source(RequestSourceToTransfer::apply(request.as_ref()))
        .target(RequestTargetToTransfer::apply(request.as_ref()))
        .amount(RequestAmountToTransfer::apply(request.as_ref()))
        .build();

    assert_eq!(command.stream_declarations().len(), 1);
    let error = match CommandLogic::handle(&command, Default::default()) {
        Ok(_) => panic!("equal stream identities must reach domain validation"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "transfer source and target must differ");
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_verifies_the_registered_runtime_mappings() {
    let report = check().expect("all modeled fields have executable provenance");

    assert_eq!(report.status, eventcore::model::CheckStatus::Verified);
}
