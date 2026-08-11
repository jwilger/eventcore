//! In-memory linearized read model (ADR-0039) and the linearization engine
//! that computes canonical order and stream versions at read time.

use std::collections::{HashMap, HashSet};
use std::fs;

use uuid::Uuid;

use crate::config::Roots;
use crate::error::FsEventStoreError;
use crate::format::{EventEnvelope, LoadedTransaction, TransactionHeader, load_transaction};
use crate::ingestion::{IngestionLog, position_from_seq};

/// An event as held in the in-memory linearized index.
#[derive(Debug, Clone)]
pub(crate) struct IndexedEvent {
    pub(crate) event_id: Uuid,
    pub(crate) stream_id: String,
    pub(crate) event_type: String,
    pub(crate) event_data: serde_json::Value,
}

/// The linearized read model, rebuilt from `events/` on open.
#[derive(Debug, Default)]
pub(crate) struct Index {
    pub(crate) headers: HashMap<Uuid, TransactionHeader>,
    /// Events in this replica's local-ingestion order, paired with the
    /// position UUID derived from each event's local sequence (ADR-0043(c)).
    /// This is the order the `EventReader` cursor advances through, so it is
    /// sorted ascending by position and a merge-introduced event always sorts
    /// after everything previously ingested.
    pub(crate) by_ingestion: Vec<(Uuid, IndexedEvent)>,
    /// Per-stream events in canonical order.
    pub(crate) streams: HashMap<String, Vec<IndexedEvent>>,
    /// Transactions with no children — the current head(s).
    pub(crate) tips: Vec<Uuid>,
    /// This replica's local-ingestion order (sequence-per-event), held so the
    /// append path can hand out the next position.
    pub(crate) log: IngestionLog,
    /// Transaction files rejected by the read-time fsck because their payload
    /// did not match the recorded content-hash anchor (ADR-0046). They are
    /// excluded from the linearized history and surfaced via `status()`.
    pub(crate) integrity_failures: Vec<(Uuid, String)>,
}

impl Index {
    pub(crate) fn stream_head(&self, stream_id: &str) -> usize {
        self.streams.get(stream_id).map(Vec::len).unwrap_or(0)
    }

    /// Record a newly-appended event in local-ingestion order: allocate the
    /// next local sequence, append it to the ingestion view at the largest
    /// position (so the view stays sorted), and return both the position UUID
    /// to use as the cursor key and the `(event_id, seq)` entry the caller must
    /// persist to the ingestion log.
    pub(crate) fn ingest_appended(&mut self, indexed: IndexedEvent) -> (Uuid, (Uuid, u64)) {
        let event_id = indexed.event_id;
        let seq = self.log.allocate(event_id);
        let position = position_from_seq(seq);
        self.by_ingestion.push((position, indexed));
        (position, (event_id, seq))
    }
}

/// Compute the number of ancestor transactions reachable via parent pointers.
/// In single-writer mode (a chain) this is a strictly increasing total order.
fn transaction_depth(
    id: Uuid,
    headers: &HashMap<Uuid, TransactionHeader>,
    memo: &mut HashMap<Uuid, usize>,
) -> usize {
    if let Some(depth) = memo.get(&id) {
        return *depth;
    }
    let depth = match headers.get(&id) {
        None => 0,
        Some(header) => {
            let mut best = 0;
            for parent in &header.parent_transaction_ids {
                if headers.contains_key(parent) {
                    best = best.max(transaction_depth(*parent, headers, memo) + 1);
                }
            }
            best
        }
    };
    let _ = memo.insert(id, depth);
    depth
}

/// Order transactions into the canonical linear sequence.
///
/// Transactions are sorted by ancestor depth (so every parent precedes its
/// children — `depth(child) >= depth(parent) + 1` makes depth-order a valid
/// topological order), with concurrent transactions (equal depth) broken by
/// the deterministic tuple `(created_at, replica_id, transaction_id)`. Every
/// component is an immutable recorded value, so all replicas holding the same
/// file set compute the identical order (ADR-0039 convergence guarantee).
fn linearize(headers: &HashMap<Uuid, TransactionHeader>) -> Vec<Uuid> {
    let mut memo: HashMap<Uuid, usize> = HashMap::new();
    let mut keyed: Vec<(Uuid, (usize, String, Uuid, Uuid))> = headers
        .iter()
        .map(|(id, header)| {
            let depth = transaction_depth(*id, headers, &mut memo);
            (
                *id,
                (depth, header.created_at.clone(), header.replica_id, *id),
            )
        })
        .collect();
    keyed.sort_by(|(_, left), (_, right)| left.cmp(right));
    keyed.into_iter().map(|(id, _)| id).collect()
}

/// The tip transactions are those no other transaction lists as a parent.
pub(crate) fn compute_tips(headers: &HashMap<Uuid, TransactionHeader>) -> Vec<Uuid> {
    let mut referenced: HashSet<Uuid> = HashSet::new();
    for header in headers.values() {
        for parent in &header.parent_transaction_ids {
            let _ = referenced.insert(*parent);
        }
    }
    let mut tips: Vec<Uuid> = headers
        .keys()
        .copied()
        .filter(|id| !referenced.contains(id))
        .collect();
    tips.sort();
    tips
}

fn build_index(
    transactions: Vec<(TransactionHeader, Vec<EventEnvelope>)>,
    mut log: IngestionLog,
) -> (Index, Vec<(Uuid, u64)>) {
    let mut headers: HashMap<Uuid, TransactionHeader> = HashMap::new();
    let mut events_by_txn: HashMap<Uuid, Vec<EventEnvelope>> = HashMap::new();
    for (header, events) in transactions {
        let id = header.transaction_id;
        let _ = headers.insert(id, header);
        let _ = events_by_txn.insert(id, events);
    }

    let order = linearize(&headers);
    let tips = compute_tips(&headers);

    // Walk events in canonical order to build the per-stream canonical view and
    // to assign a local-ingestion sequence to every event not already in the
    // log. Events already known keep their persisted sequence (a projection
    // checkpoint stays valid across reopen); events new to this replica — newly
    // appended or arrived via a `git merge` — get fresh, larger sequences so a
    // cursor advances forward to cover them (ADR-0043(c)).
    let mut streams: HashMap<String, Vec<IndexedEvent>> = HashMap::new();
    let mut positioned: Vec<(Uuid, IndexedEvent)> = Vec::new();
    let mut new_entries: Vec<(Uuid, u64)> = Vec::new();
    for transaction_id in order {
        let events = events_by_txn.remove(&transaction_id).unwrap_or_default();
        for envelope in events {
            let indexed = IndexedEvent {
                event_id: envelope.event_id,
                stream_id: envelope.stream_id,
                event_type: envelope.event_type,
                event_data: envelope.event_data,
            };
            streams
                .entry(indexed.stream_id.clone())
                .or_default()
                .push(indexed.clone());
            let seq = match log.seq_of(&indexed.event_id) {
                Some(seq) => seq,
                None => {
                    let seq = log.allocate(indexed.event_id);
                    new_entries.push((indexed.event_id, seq));
                    seq
                }
            };
            positioned.push((position_from_seq(seq), indexed));
        }
    }
    // Local-ingestion order is the assigned sequence; sort so the cursor walks
    // events in the order this replica became aware of them.
    positioned.sort_by_key(|(position, _)| *position);

    (
        Index {
            headers,
            by_ingestion: positioned,
            streams,
            tips,
            log,
            integrity_failures: Vec::new(),
        },
        new_entries,
    )
}

/// Build event envelopes and their index entries for a transaction, assigning
/// each event a contiguous per-stream version above the stream's current head.
/// Shared by the normal append path and the merge path so the version
/// arithmetic is computed — and tested — in exactly one place.
pub(crate) fn build_envelopes(
    index: &Index,
    items: Vec<(String, &'static str, serde_json::Value)>,
) -> (Vec<EventEnvelope>, Vec<IndexedEvent>) {
    let mut added: HashMap<String, usize> = HashMap::new();
    let mut envelopes: Vec<EventEnvelope> = Vec::with_capacity(items.len());
    let mut new_events: Vec<IndexedEvent> = Vec::with_capacity(items.len());
    for (key, event_type, event_data) in items {
        let head = index.stream_head(&key);
        let offset = added.entry(key.clone()).or_insert(0);
        *offset += 1;
        let stream_version = head + *offset;
        let event_id = Uuid::now_v7();
        envelopes.push(EventEnvelope {
            event_id,
            stream_id: key.clone(),
            stream_version,
            event_type: event_type.to_string(),
            event_data: event_data.clone(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        });
        new_events.push(IndexedEvent {
            event_id,
            stream_id: key,
            event_type: event_type.to_string(),
            event_data,
        });
    }
    (envelopes, new_events)
}

pub(crate) fn scan_index(roots: &Roots) -> Result<Index, FsEventStoreError> {
    let read_dir = fs::read_dir(&roots.events).map_err(|source| FsEventStoreError::InitFailed {
        path: roots.events.clone(),
        source,
    })?;
    let mut transactions: Vec<(TransactionHeader, Vec<EventEnvelope>)> = Vec::new();
    let mut integrity_failures: Vec<(Uuid, String)> = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|source| FsEventStoreError::InitFailed {
            path: roots.events.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        // Read-time fsck (ADR-0046): a file whose payload no longer matches its
        // content-hash anchor is rejected and surfaced via status(), never
        // parsed into the trusted history.
        match load_transaction(&path)? {
            LoadedTransaction::Valid(header, events) => transactions.push((header, events)),
            LoadedTransaction::Integrity {
                transaction_id,
                detail,
            } => integrity_failures.push((transaction_id, detail)),
        }
    }
    let log = IngestionLog::load(roots)?;
    let (mut index, new_entries) = build_index(transactions, log);
    index.log.persist(roots, &new_entries)?;
    index.integrity_failures = integrity_failures;
    Ok(index)
}

#[cfg(test)]
#[path = "index.test.rs"]
mod tests;
