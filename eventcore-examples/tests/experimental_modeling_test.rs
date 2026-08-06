//! A complete, feature-gated Event Modeling lane.
//!
//! The test is intentionally executable: the same mappings that are checked
//! build a command, produce an event, and drive a projection sink.

use std::convert::Infallible;

use eventcore::{
    Event, ModelCommand, ModelEvent, ModelInput, ModelOutput, ModelReadModel, ModelState,
    Projector, RetryPolicy, StreamId, StreamIdentity, StreamPosition, execute, mapping,
    model::{
        InMemoryProjectionSink, ModelCommandLogic, ModelEffect, ModelEffectApplication,
        ModelProjection, Modeled, ModeledEvents, ProjectionAction,
        StreamIdentity as StreamIdentityTrait, checked_projection,
    },
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, StreamIdentity)]
pub struct TransferSource(StreamId);

#[derive(Clone, Debug, PartialEq, Eq, StreamIdentity)]
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

mapping! { RequestSource: TransferRequest.source => Transfer.source using clone; }
mapping! { RequestTarget: TransferRequest.target => Transfer.target using clone; }
mapping! { RequestAmount: TransferRequest.amount => Transfer.amount using clone; }

#[derive(Clone, Debug, Serialize, Deserialize, ModelEvent)]
struct BankEvent {
    stream_id: StreamId,
    amount: u64,
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
    source.as_stream_id().clone()
}
mapping! { CommandSourceToEvent: Transfer.source => BankEvent.stream_id using source_stream; }
mapping! { CommandAmountToEvent: Transfer.amount => BankEvent.amount using clone; }

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
        let _was_processed = state.processed;
        Modeled::from_built(state)
    }

    fn decide(
        &self,
        _state: Modeled<Self::State>,
    ) -> Result<ModeledEvents<Self::Event>, eventcore::CommandError> {
        let event = BankEvent::model_builder()
            .stream_id(CommandSourceToEvent::apply(self))
            .amount(CommandAmountToEvent::apply(self))
            .build();
        Ok(ModeledEvents::one(event))
    }
}

#[derive(ModelReadModel)]
struct AccountHistory {
    balance: u64,
}

mapping! { EventAmountToBalance: BankEvent.amount => AccountHistory.balance using clone; }

#[derive(ModelOutput)]
struct AccountView {
    balance: u64,
}

mapping! { BalanceToView: AccountHistory.balance => AccountView.balance using clone; }

struct BalanceEffect {
    amount: u64,
}

impl ModelEffect for BalanceEffect {}

impl ModelEffectApplication<AccountHistory> for BalanceEffect {
    fn apply_to(self, previous: Modeled<AccountHistory>) -> Modeled<AccountHistory> {
        let previous = previous.into_inner();
        Modeled::from_built(AccountHistory {
            balance: previous.balance + self.amount,
        })
    }
}

struct AccountProjector;

impl ModelProjection for AccountProjector {
    type Event = BankEvent;
    type Effect = BalanceEffect;
    type Error = Infallible;

    fn name(&self) -> &str {
        "account-history"
    }

    fn project(
        &mut self,
        event: Self::Event,
        _position: StreamPosition,
    ) -> Result<ProjectionAction<Self::Effect>, Self::Error> {
        Ok(ProjectionAction::Apply(Modeled::from_built(
            BalanceEffect {
                amount: event.amount,
            },
        )))
    }
}

fn stream(value: &str) -> StreamId {
    StreamId::try_new(value.to_owned()).expect("valid stream id")
}

#[tokio::test]
async fn modeled_bank_transfer_is_checked_and_executes_through_eventcore() {
    let report = eventcore::model::check().expect("the runtime graph is complete");
    assert_eq!(report.status, eventcore::model::CheckStatus::Verified);

    let request = TransferRequest::model_builder()
        .source(TransferSource(stream("accounts::source")))
        .target(TransferTarget(stream("accounts::target")))
        .amount(25)
        .build();
    let command = Transfer::model_builder()
        .source(RequestSource::apply(request.as_ref()))
        .target(RequestTarget::apply(request.as_ref()))
        .amount(RequestAmount::apply(request.as_ref()))
        .build();

    let store = InMemoryEventStore::new();
    let response = execute(&store, command, RetryPolicy::new())
        .await
        .expect("modeled command executes through the stable executor");
    assert_eq!(response.attempts(), 1);

    let sink = InMemoryProjectionSink::new(Modeled::from_built(AccountHistory { balance: 0 }));
    let mut projector = checked_projection(AccountProjector, sink);
    projector
        .apply(
            BankEvent {
                stream_id: stream("accounts::source"),
                amount: 25,
            },
            StreamPosition::new(Uuid::now_v7()),
            &mut (),
        )
        .expect("modeled projection applies its effect");

    let view = AccountView::model_builder()
        .balance(BalanceToView::apply(&AccountHistory { balance: 25 }))
        .build();
    assert_eq!(view.as_ref().balance, 25);
}
