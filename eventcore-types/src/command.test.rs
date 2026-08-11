use super::*;

fn stream(id: &str) -> StreamId {
    StreamId::try_new(id.to_owned()).expect("valid stream id")
}

#[test]
fn try_from_streams_succeeds_with_unique_streams() {
    let result = StreamDeclarations::try_from_streams(vec![
        stream("accounts::primary"),
        stream("accounts::secondary"),
    ]);

    assert!(result.is_ok());
}

#[test]
fn try_from_streams_rejects_empty_collections() {
    let result = StreamDeclarations::try_from_streams(Vec::new());

    assert_eq!(Err(StreamDeclarationsError::Empty), result);
}

#[test]
fn try_from_streams_rejects_duplicate_streams() {
    let duplicate = stream("accounts::primary");
    let result = StreamDeclarations::try_from_streams(vec![duplicate.clone(), duplicate.clone()]);

    assert_eq!(
        Err(StreamDeclarationsError::DuplicateStream {
            duplicate: duplicate.clone(),
        }),
        result,
    );
}

#[test]
fn with_participant_rejects_duplicate_streams() {
    let existing = stream("accounts::primary");
    let streams = StreamDeclarations::single(existing.clone());
    let result = streams.with_participant(existing.clone());

    assert_eq!(
        Err(StreamDeclarationsError::DuplicateStream {
            duplicate: existing,
        }),
        result,
    );
}

#[test]
fn len_returns_number_of_declared_streams() {
    let streams = StreamDeclarations::try_from_streams(vec![
        stream("accounts::primary"),
        stream("audit::shadow"),
    ])
    .expect("multi-stream declaration should succeed");

    assert_eq!(2, streams.len());
}

#[test]
fn is_empty_returns_true_for_empty_construction() {
    let result = StreamDeclarations::try_from_streams(Vec::<StreamId>::new());

    assert!(matches!(result, Err(StreamDeclarationsError::Empty)));
}

#[test]
fn is_empty_returns_false_for_single_stream() {
    let streams = StreamDeclarations::single(stream("accounts::primary"));

    assert!(!streams.is_empty());
}

#[test]
fn is_empty_returns_false_for_multi_stream() {
    let streams = StreamDeclarations::try_from_streams(vec![
        stream("accounts::primary"),
        stream("audit::shadow"),
    ])
    .expect("multi-stream declaration should succeed");

    assert!(!streams.is_empty());
}

#[test]
fn stream_declarations_len_and_is_empty_consistency() {
    let primary = stream("accounts::primary");
    let secondary = stream("audit::shadow");

    let single = StreamDeclarations::single(primary.clone());
    let multi = StreamDeclarations::try_from_streams(vec![primary, secondary])
        .expect("multi-stream declaration should succeed");
    let empty_error = StreamDeclarations::try_from_streams(Vec::<StreamId>::new())
        .expect_err("empty set rejected");

    let observed = (
        single.len(),
        single.is_empty(),
        multi.len(),
        multi.is_empty(),
        matches!(empty_error, StreamDeclarationsError::Empty),
    );

    assert_eq!(observed, (1, false, 2, false, true));
}

#[test]
fn iter_yields_declared_streams() {
    let primary = stream("accounts::primary");
    let secondary = stream("audit::shadow");
    let declarations =
        StreamDeclarations::try_from_streams(vec![primary.clone(), secondary.clone()])
            .expect("multi-stream declaration should succeed");

    let collected: Vec<&StreamId> = declarations.iter().collect();

    assert_eq!(collected, vec![&primary, &secondary]);
}
