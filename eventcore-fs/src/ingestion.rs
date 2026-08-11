//! Per-replica local-ingestion order (ADR-0043(c)).
//!
//! The `EventReader` cursor on the file store must be monotonic in the order
//! *this* replica first became aware of each event — not in canonical
//! linearized order. A `git merge` can union in a transaction whose canonical
//! position is *earlier* than a cursor a projection already passed; keyed on
//! canonical position that event would be silently skipped. Tracking
//! local-ingestion order instead gives every newly-arrived event a fresh,
//! larger position, so a projection's cursor advances forward to cover it and
//! never rewinds.
//!
//! Each event is assigned a strictly increasing local sequence number the first
//! time this replica sees it. The sequence is encoded into the high bytes of a
//! [`Uuid`] so the resulting [`StreamPosition`](eventcore_types::StreamPosition)
//! sorts by sequence. The mapping is persisted under the gitignored, rebuildable
//! `index/` directory (ADR-0046): it is machine-local and never committed.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Roots;
use crate::error::FsEventStoreError;

/// One persisted local-ingestion assignment: an event and the local sequence
/// number this replica gave it.
#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    event_id: Uuid,
    seq: u64,
}

/// The local-ingestion order: which local sequence number this replica assigned
/// each event, and the next sequence to hand out.
#[derive(Debug, Default)]
pub(crate) struct IngestionLog {
    seq_by_event: HashMap<Uuid, u64>,
    next_seq: u64,
}

/// Encode a local-ingestion sequence number into a position UUID whose natural
/// `Ord` matches the sequence: the sequence occupies the high 8 bytes, so a
/// larger sequence is a strictly larger UUID. Sequences start at 1 so no event
/// ever maps to the nil UUID (which would collide with "before any event").
pub(crate) fn position_from_seq(seq: u64) -> Uuid {
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&seq.to_be_bytes());
    Uuid::from_bytes(bytes)
}

impl IngestionLog {
    /// Load the persisted local-ingestion order. A missing log (fresh clone, or
    /// an index that was never built) is an empty log: every event present is
    /// treated as newly ingested.
    pub(crate) fn load(roots: &Roots) -> Result<Self, FsEventStoreError> {
        let path = roots.ingestion_log_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(FsEventStoreError::InitFailed { path, source });
            }
        };
        let mut seq_by_event: HashMap<Uuid, u64> = HashMap::new();
        let mut max_seq: u64 = 0;
        for line in contents.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: LogEntry =
                serde_json::from_str(line).map_err(|error| FsEventStoreError::Corrupted {
                    path: path.clone(),
                    detail: format!("invalid ingestion-log line: {error}"),
                })?;
            max_seq = max_seq.max(entry.seq);
            let _ = seq_by_event.insert(entry.event_id, entry.seq);
        }
        Ok(Self {
            seq_by_event,
            next_seq: max_seq + 1,
        })
    }

    /// The local sequence already assigned to `event_id`, if any.
    pub(crate) fn seq_of(&self, event_id: &Uuid) -> Option<u64> {
        self.seq_by_event.get(event_id).copied()
    }

    /// Allocate the next local sequence for `event_id`, recording it in memory.
    /// The caller is responsible for persisting via [`Self::persist`].
    pub(crate) fn allocate(&mut self, event_id: Uuid) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        let _ = self.seq_by_event.insert(event_id, seq);
        seq
    }

    /// Append the given assignments to the persisted log. The log is
    /// append-only; entries are never rewritten.
    pub(crate) fn persist(
        &self,
        roots: &Roots,
        entries: &[(Uuid, u64)],
    ) -> Result<(), FsEventStoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let path = roots.ingestion_log_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| FsEventStoreError::InitFailed {
                path: path.clone(),
                source,
            })?;
        let mut body = String::new();
        for (event_id, seq) in entries {
            let line = serde_json::to_string(&LogEntry {
                event_id: *event_id,
                seq: *seq,
            })
            .map_err(|error| FsEventStoreError::Corrupted {
                path: path.clone(),
                detail: format!("failed to serialize ingestion-log entry: {error}"),
            })?;
            body.push_str(&line);
            body.push('\n');
        }
        file.write_all(body.as_bytes())
            .map_err(|source| FsEventStoreError::InitFailed { path, source })
    }
}

#[cfg(test)]
#[path = "ingestion.test.rs"]
mod tests;
