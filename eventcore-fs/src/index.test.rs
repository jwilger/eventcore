use super::*;
use std::collections::BTreeMap;

fn header(id: Uuid, parents: Vec<Uuid>) -> TransactionHeader {
    TransactionHeader {
        format_version: crate::format::FORMAT_VERSION,
        content_hash: None,
        transaction_id: id,
        replica_id: Uuid::now_v7(),
        parent_transaction_ids: parents,
        created_at: "2026-06-12T00:00:00Z".to_string(),
        stream_bases: BTreeMap::new(),
    }
}

#[test]
fn linearize_orders_chain_by_parent_depth() {
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    let mut headers: HashMap<Uuid, TransactionHeader> = HashMap::new();
    // Insert out of order to prove ordering comes from parent links, not insertion.
    let _ = headers.insert(c, header(c, vec![b]));
    let _ = headers.insert(a, header(a, vec![]));
    let _ = headers.insert(b, header(b, vec![a]));

    assert_eq!(linearize(&headers), vec![a, b, c]);
    assert_eq!(compute_tips(&headers), vec![c]);
}

#[test]
fn transaction_depth_takes_max_of_known_parents_and_ignores_dangling() {
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    let d = Uuid::now_v7();
    let e = Uuid::now_v7();
    let missing = Uuid::now_v7();
    let mut headers: HashMap<Uuid, TransactionHeader> = HashMap::new();
    let _ = headers.insert(a, header(a, vec![]));
    let _ = headers.insert(b, header(b, vec![a]));
    let _ = headers.insert(c, header(c, vec![b]));
    // d has a shallow parent (a, depth 0) and a deep parent (c, depth 2),
    // plus a dangling parent that must be ignored: depth = max(1, 3) = 3.
    let _ = headers.insert(d, header(d, vec![a, c, missing]));
    // e's only parent is dangling, so it is treated as a root: depth 0.
    let _ = headers.insert(e, header(e, vec![missing]));

    let mut memo: HashMap<Uuid, usize> = HashMap::new();
    assert_eq!(transaction_depth(a, &headers, &mut memo), 0);
    assert_eq!(transaction_depth(b, &headers, &mut memo), 1);
    assert_eq!(transaction_depth(c, &headers, &mut memo), 2);
    assert_eq!(transaction_depth(d, &headers, &mut memo), 3);
    assert_eq!(transaction_depth(e, &headers, &mut memo), 0);
}
