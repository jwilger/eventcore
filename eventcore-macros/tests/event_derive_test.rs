use eventcore::{Event, EventTypeName, StreamId};
use eventcore_macros::Event as DeriveEvent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Simple struct event type for testing derive macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveEvent)]
struct MoneyDeposited {
    #[stream]
    stream_id: StreamId,
    amount: u32,
}

fn new_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

#[test]
fn struct_event_type_returns_struct_name() {
    // Given: a struct event type decorated with #[derive(Event)]
    let account_id = new_stream_id();
    let event = MoneyDeposited {
        stream_id: account_id,
        amount: 100,
    };

    // When: event_type_name() is called
    let type_name = event.event_type_name();

    // Then: it returns the struct name as the type name
    let expected: EventTypeName = "MoneyDeposited".try_into().expect("valid event type name");
    assert_eq!(type_name, expected);
}

/// Enum event type for testing derive macro with multiple variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveEvent)]
enum AccountEvent {
    Deposited {
        #[stream]
        stream_id: StreamId,
        amount: u32,
    },
    Withdrawn {
        #[stream]
        stream_id: StreamId,
        amount: u32,
    },
}

#[test]
fn enum_event_type_returns_variant_names() {
    // Given: an enum event type with multiple variants
    let stream_id = StreamId::try_new("account-123").expect("valid stream id");
    let deposited = AccountEvent::Deposited {
        stream_id: stream_id.clone(),
        amount: 100,
    };

    // When: event_type_name() is called on the Deposited variant
    let deposited_type = deposited.event_type_name();

    // Then: it returns the variant name "Deposited"
    let expected: EventTypeName = "Deposited".try_into().expect("valid event type name");
    assert_eq!(deposited_type, expected);
}

#[test]
fn enum_event_type_returns_different_variant_name() {
    // Given: an enum event type with multiple variants
    let stream_id = StreamId::try_new("account-123").expect("valid stream id");
    let withdrawn = AccountEvent::Withdrawn {
        stream_id: stream_id.clone(),
        amount: 50,
    };

    // When: event_type_name() is called on the Withdrawn variant
    let withdrawn_type = withdrawn.event_type_name();

    // Then: it returns the variant name "Withdrawn"
    let expected: EventTypeName = "Withdrawn".try_into().expect("valid event type name");
    assert_eq!(withdrawn_type, expected);
}

#[test]
fn enum_all_type_names_returns_all_variants() {
    // Given: an enum event type with multiple variants
    // When: all_type_names() is called
    let all_names = AccountEvent::all_type_names();

    // Then: it returns both variant names
    let deposited: EventTypeName = "Deposited".try_into().expect("valid event type name");
    let withdrawn: EventTypeName = "Withdrawn".try_into().expect("valid event type name");
    assert!(all_names.contains(&deposited));
    assert!(all_names.contains(&withdrawn));
    assert_eq!(all_names.len(), 2);
}

#[test]
fn enum_stream_id_works_for_deposited_variant() {
    // Given: a Deposited variant with a stream_id
    let stream_id = StreamId::try_new("account-123").expect("valid stream id");
    let deposited = AccountEvent::Deposited {
        stream_id: stream_id.clone(),
        amount: 100,
    };

    // When: stream_id() is called
    let retrieved_stream_id = deposited.stream_id();

    // Then: it returns the correct stream_id
    assert_eq!(retrieved_stream_id, &stream_id);
}

#[test]
fn enum_stream_id_works_for_withdrawn_variant() {
    // Given: a Withdrawn variant with a stream_id
    let stream_id = StreamId::try_new("account-123").expect("valid stream id");
    let withdrawn = AccountEvent::Withdrawn {
        stream_id: stream_id.clone(),
        amount: 50,
    };

    // When: stream_id() is called
    let retrieved_stream_id = withdrawn.stream_id();

    // Then: it returns the correct stream_id
    assert_eq!(retrieved_stream_id, &stream_id);
}
