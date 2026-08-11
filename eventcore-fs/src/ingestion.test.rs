use super::*;

#[test]
fn position_encoding_is_monotonic_in_sequence() {
    // Position UUIDs must sort by sequence so the cursor advances forward.
    let mut previous = position_from_seq(0);
    for seq in 1..1000u64 {
        let current = position_from_seq(seq);
        assert!(
            current > previous,
            "seq {seq} must produce a strictly larger position"
        );
        previous = current;
    }
}

#[test]
fn sequence_one_is_above_the_nil_position() {
    // The exclusive `after` cursor starts below any real event; seq 1 must
    // be strictly greater than the nil UUID so the first event is reachable.
    assert!(position_from_seq(1) > Uuid::nil());
}
