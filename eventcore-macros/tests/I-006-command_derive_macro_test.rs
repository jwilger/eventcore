use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, EventStore, InMemoryEventStore, NewEvents,
    RetryPolicy, StreamDeclarations, StreamId, execute,
};
use eventcore_macros::Command;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Domain events expressed in the test to keep both implementations honest.
/// They mimic the real aggregate events a developer would emit when moving
/// money between two accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TransferEvent {
    Debited { account_id: StreamId, cents: u64 },
    Credited { account_id: StreamId, cents: u64 },
}

impl Event for TransferEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            TransferEvent::Debited { account_id, .. }
            | TransferEvent::Credited { account_id, .. } => account_id,
        }
    }

    fn event_type_name(&self) -> &'static str {
        match self {
            TransferEvent::Debited { .. } => "Debited",
            TransferEvent::Credited { .. } => "Credited",
        }
    }
}

/// Minimal state object rebuilt from prior events to validate the command.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct TransferLedger {
    debits_recorded: u32,
    credits_recorded: u32,
}

impl TransferLedger {
    fn record(mut self, event: &TransferEvent) -> Self {
        match event {
            TransferEvent::Debited { .. } => self.debits_recorded += 1,
            TransferEvent::Credited { .. } => self.credits_recorded += 1,
        }
        self
    }

    fn already_completed(&self) -> bool {
        self.debits_recorded > 0 && self.credits_recorded > 0
    }
}

/// Pre-existing manual implementation developers want to migrate away from.
struct ManualTransfer {
    source: StreamId,
    destination: StreamId,
    cents: u64,
}

impl CommandStreams for ManualTransfer {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.source.clone(), self.destination.clone()])
            .expect("manual transfer declares unique streams")
    }
}

impl CommandLogic for ManualTransfer {
    type Event = TransferEvent;
    type State = TransferLedger;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.record(event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if state.already_completed() {
            return Err(CommandError::BusinessRuleViolation(
                "transfer already applied to both streams".to_string(),
            ));
        }

        Ok(vec![
            TransferEvent::Debited {
                account_id: self.source.clone(),
                cents: self.cents,
            },
            TransferEvent::Credited {
                account_id: self.destination.clone(),
                cents: self.cents,
            },
        ]
        .into())
    }
}

/// Version developers wish for: a derive-backed command that removes
/// the hand-written `CommandStreams` boilerplate.
#[derive(Command)]
struct DerivedTransfer {
    #[stream]
    source: StreamId,
    #[stream]
    destination: StreamId,
    cents: u64,
}

impl CommandLogic for DerivedTransfer {
    type Event = TransferEvent;
    type State = TransferLedger;

    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State {
        state.record(event)
    }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if state.already_completed() {
            return Err(CommandError::BusinessRuleViolation(
                "transfer already applied to both streams".to_string(),
            ));
        }

        Ok(vec![
            TransferEvent::Debited {
                account_id: self.source.clone(),
                cents: self.cents,
            },
            TransferEvent::Credited {
                account_id: self.destination.clone(),
                cents: self.cents,
            },
        ]
        .into())
    }
}

/// Convenience helper to generate unique stream identifiers per test run.
fn new_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

#[tokio::test]
async fn migrating_from_manual_impl_preserves_behavior() {
    // Given: developer prepares manual and derived transfer commands targeting identical streams
    let source_account = new_stream_id();
    let destination_account = new_stream_id();
    let cents = 6_500u64;

    let manual_command = ManualTransfer {
        source: source_account.clone(),
        destination: destination_account.clone(),
        cents,
    };
    let derived_command = DerivedTransfer {
        source: source_account.clone(),
        destination: destination_account.clone(),
        cents,
    };

    // Then: both approaches expose identical stream declarations in the same order
    let manual_streams: Vec<StreamId> = manual_command
        .stream_declarations()
        .iter()
        .cloned()
        .collect();
    let derived_streams: Vec<StreamId> = derived_command
        .stream_declarations()
        .iter()
        .cloned()
        .collect();

    let manual_store = InMemoryEventStore::new();
    let derived_store = InMemoryEventStore::new();

    // When: developer executes both variants to compare behavior before and after migration
    execute(&manual_store, manual_command, RetryPolicy::new())
        .await
        .expect("manual command to succeed");
    execute(&derived_store, derived_command, RetryPolicy::new())
        .await
        .expect("derived command to succeed");

    assert_eq!(
        manual_streams, derived_streams,
        "#[derive(Command)] should declare the same streams as the manual implementation",
    );

    // And Then: derived macro writes the same externally-observable events as the manual command
    let manual_source_events = manual_store
        .read_stream::<TransferEvent>(source_account.clone())
        .await
        .expect("reading manual source stream should succeed")
        .into_iter()
        .collect::<Vec<_>>();
    let derived_source_events = derived_store
        .read_stream::<TransferEvent>(source_account.clone())
        .await
        .expect("reading derived source stream should succeed")
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        manual_source_events, derived_source_events,
        "source stream should observe identical debits after migration",
    );

    let manual_destination_events = manual_store
        .read_stream::<TransferEvent>(destination_account.clone())
        .await
        .expect("reading manual destination stream should succeed")
        .into_iter()
        .collect::<Vec<_>>();
    let derived_destination_events = derived_store
        .read_stream::<TransferEvent>(destination_account.clone())
        .await
        .expect("reading derived destination stream should succeed")
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        manual_destination_events, derived_destination_events,
        "destination stream should observe identical credits after migration",
    );
}
