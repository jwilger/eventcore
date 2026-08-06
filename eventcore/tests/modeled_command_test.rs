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
    source: TransferSource,
    target: TransferTarget,
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
        let _amount = self.amount;
        Ok(ModeledEvents::none("example does not emit events"))
    }
}

fn stream(value: &str) -> StreamId {
    StreamId::try_new(value.to_owned()).expect("valid stream id")
}

#[test]
fn modeled_command_builder_accepts_semantic_stream_ids_and_deduplicates_equal_ids() {
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
    assert!(CommandLogic::handle(&command, Default::default()).is_ok());
}

#[cfg(feature = "experimental-model-check")]
#[test]
fn checker_verifies_the_registered_runtime_mappings() {
    let report = check().expect("all modeled fields have executable provenance");

    assert_eq!(report.status, eventcore::model::CheckStatus::Verified);
}
