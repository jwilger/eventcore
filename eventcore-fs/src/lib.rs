//! File-based, git-mergeable event store backend for EventCore.
//!
//! `eventcore-fs` persists each `append_events` transaction as one immutable
//! JSONL file under `<root>/events/`, named by a transaction UUID7. Because
//! every transaction is a uniquely named file that is never edited, a
//! `git merge` of two clones' `events/` directories is a pure additive union
//! with no textual conflicts — the foundation for the offline-collaboration
//! merge mode (Layer 2).
//!
//! Layer 1 is a single-writer backend that satisfies the shared EventCore
//! contract suite. `StreamVersion` and global order are computed at read time
//! by linearizing a transaction DAG (ADR-0039); in single-writer mode the DAG
//! is a linear chain, so the computed order is the append order and the
//! computed versions are contiguous.
//!
//! ## Module map
//!
//! - `error` — the crate's error types
//! - `config` — store configuration and the on-disk directory layout
//! - `format` — the immutable JSONL transaction-file format (ADR-0038)
//! - `index` — the in-memory read model and linearization engine (ADR-0039)
//! - `ingestion` — per-replica local-ingestion order backing the EventReader
//!   cursor (ADR-0043(c))
//! - `merge` — fork detection and reconciliation types (ADR-0041/0042)
//! - `replica` — fingerprint-bound, machine-local replica identity (ADR-0044)
//! - `coordination` — locking, checkpoints, and projector coordination
//!   (ADR-0040)
//!
//! See ADRs 0038–0047 and the `fs-merge-mode` blueprint for the full design.

mod config;
mod coordination;
mod error;
mod format;
mod index;
mod ingestion;
mod merge;
mod replica;

use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, RwLock};

use eventcore_types::{
    CommandStateSnapshot, CommandStateSnapshotId, Event, EventFilter, EventPage, EventReader,
    EventStore, EventStoreError, EventStream, EventStreamSlice, Operation, StreamId,
    StreamPosition, StreamVersion, StreamWriteEntry, StreamWrites,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub use config::{FsConfig, FsyncPolicy};
pub use coordination::{FileCheckpointStore, FileLeadershipGuard, FileProjectorCoordinator};
pub use error::{FsCheckpointError, FsCoordinationError, FsEventStoreError};
pub use merge::{
    BranchView, DanglingTransaction, Fork, ForkContext, IntegrityFailure, ReconcileReport,
    ResolutionOutcome, StoreStatus, TransactionId,
};

use config::Roots;
use coordination::StoreLockGuard;
use format::{
    FORMAT_VERSION, TransactionHeader, parse_transaction, serialize_transaction,
    write_transaction_file,
};
use index::{Index, build_envelopes, compute_tips, scan_index};
use merge::{detect_dangling_in, detect_forks_in};

#[derive(Debug)]
struct Shared {
    roots: Roots,
    replica_id: Uuid,
    config: FsConfig,
    index: RwLock<Index>,
    append_lock: tokio::sync::Mutex<()>,
    _store_lock: StoreLockGuard,
}

/// A file-based [`EventStore`] / [`EventReader`].
#[derive(Clone, Debug)]
pub struct FileEventStore {
    shared: Arc<Shared>,
}

impl FileEventStore {
    /// Open (or create) a file event store rooted at `root` with full fsync.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, FsEventStoreError> {
        Self::open_with_config(FsConfig::new(root.as_ref()))
    }

    /// Open (or create) a file event store with explicit configuration.
    pub fn open_with_config(config: FsConfig) -> Result<Self, FsEventStoreError> {
        let roots = Roots::new(&config.root);
        roots.create_dirs()?;
        write_git_metadata(&roots)?;
        let store_lock = StoreLockGuard::acquire(&roots.store_lock_path())?;
        let replica_id = replica::load_or_create_replica_id(&roots, &config)?;
        let index = scan_index(&roots)?;
        Ok(Self {
            shared: Arc::new(Shared {
                roots,
                replica_id,
                config,
                index: RwLock::new(index),
                append_lock: tokio::sync::Mutex::new(()),
                _store_lock: store_lock,
            }),
        })
    }
}

/// Only `events/` is the committed source of truth; everything else is
/// derived or machine-local (ADR-0046). This includes command-state snapshots
/// under `.eventcore/snapshots/`. Critically, `.eventcore/replica_id` must
/// never be committed, or a `git clone` would duplicate a writer's identity
/// (the copy trap, ADR-0044).
const GITIGNORE: &str = "\
# eventcore-fs: only events/ is the committed source of truth.
/tmp/
/checkpoints/
/locks/
/index/
/.eventcore/
/.lock
";

/// Transaction files are immutable and uniquely named, so a `git merge` of
/// `events/` is a pure additive union. `merge=union` is a defensive backstop
/// that keeps git from ever emitting conflict markers there (ADR-0046).
const GITATTRIBUTES: &str = "events/** merge=union\n";

fn write_git_metadata(roots: &Roots) -> Result<(), FsEventStoreError> {
    write_if_absent(&roots.root.join(".gitignore"), GITIGNORE)?;
    write_if_absent(&roots.root.join(".gitattributes"), GITATTRIBUTES)?;
    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> Result<(), FsEventStoreError> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents).map_err(|source| FsEventStoreError::InitFailed {
        path: path.to_path_buf(),
        source,
    })
}

impl EventStore for FileEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStream<E>, EventStoreError> {
        // Events live in an in-memory index behind an `RwLock`. We deserialize
        // each indexed event into the requested type while holding the read
        // guard (preserving read-time linearization order), then release the
        // lock and yield the per-event results one at a time. A per-event
        // decode failure surfaces as an `Err` item. The executor consumes the
        // stream incrementally; only this local index is read up front.
        let items: Vec<Result<E, EventStoreError>> = {
            let index = self
                .shared
                .index
                .read()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                })?;
            match index.streams.get(stream_id.as_ref()) {
                None => Vec::new(),
                Some(stream) => stream
                    .iter()
                    .map(|indexed| {
                        serde_json::from_value::<E>(indexed.event_data.clone()).map_err(|error| {
                            EventStoreError::DeserializationFailed {
                                stream_id: stream_id.clone(),
                                detail: error.to_string(),
                            }
                        })
                    })
                    .collect(),
            }
        };

        Ok(EventStream::new(futures::stream::iter(items)))
    }

    async fn read_stream_after<E: Event>(
        &self,
        stream_id: StreamId,
        exclusive_version: StreamVersion,
    ) -> Result<EventStream<E>, EventStoreError> {
        let items: Vec<Result<E, EventStoreError>> = {
            let index = self
                .shared
                .index
                .read()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                })?;
            let count: usize = exclusive_version.into();
            match index.streams.get(stream_id.as_ref()) {
                None => Vec::new(),
                Some(stream) => stream
                    .iter()
                    .skip(count)
                    .map(|indexed| {
                        serde_json::from_value::<E>(indexed.event_data.clone()).map_err(|error| {
                            EventStoreError::DeserializationFailed {
                                stream_id: stream_id.clone(),
                                detail: error.to_string(),
                            }
                        })
                    })
                    .collect(),
            }
        };

        Ok(EventStream::new(futures::stream::iter(items)))
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let _append = self.shared.append_lock.lock().await;

        let expected = writes.expected_versions().clone();
        let entries = writes.into_entries();
        if entries.is_empty() {
            return Ok(EventStreamSlice);
        }

        let transaction_id = Uuid::now_v7();
        let (header, envelopes, new_events) = {
            let index = self
                .shared
                .index
                .read()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::AppendEvents,
                })?;

            for (stream_id, expected_version) in &expected {
                let current = StreamVersion::new(index.stream_head(stream_id.as_ref()));
                if current != *expected_version {
                    return Err(EventStoreError::VersionConflict {
                        stream_id: stream_id.clone(),
                        expected: *expected_version,
                        actual: current,
                    });
                }
            }

            let parent_transaction_ids = index.tips.clone();
            let stream_bases: BTreeMap<String, usize> = expected
                .iter()
                .map(|(stream_id, version)| (stream_id.as_ref().to_string(), usize::from(*version)))
                .collect();

            let items: Vec<(String, &'static str, serde_json::Value)> = entries
                .into_iter()
                .map(|entry| {
                    let StreamWriteEntry {
                        stream_id,
                        event_type,
                        event_data,
                        ..
                    } = entry;
                    // event_data is pre-serialized JSON. Parse it back into a
                    // Value so the on-disk envelope (and its integrity anchor)
                    // stays byte-identical to the historical representation.
                    let event_data = serde_json::from_str::<serde_json::Value>(event_data.get())
                        .map_err(|_| EventStoreError::StoreFailure {
                            operation: Operation::AppendEvents,
                        })?;
                    Ok((stream_id.as_ref().to_string(), event_type, event_data))
                })
                .collect::<Result<Vec<_>, EventStoreError>>()?;
            let (envelopes, new_events) = build_envelopes(&index, items);

            let header = TransactionHeader {
                format_version: FORMAT_VERSION,
                transaction_id,
                replica_id: self.shared.replica_id,
                parent_transaction_ids,
                created_at: chrono::Utc::now().to_rfc3339(),
                stream_bases,
                content_hash: None,
            };
            (header, envelopes, new_events)
        };

        let body = serialize_transaction(&header, &envelopes)?;

        let tmp_path = self.shared.roots.tmp_path(transaction_id);
        let final_path = self.shared.roots.event_path(transaction_id);
        let events_dir = self.shared.roots.events.clone();
        let fsync = self.shared.config.fsync;
        let write_result = tokio::task::spawn_blocking(move || {
            write_transaction_file(&tmp_path, &final_path, &events_dir, &body, fsync)
        })
        .await;
        match write_result {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => {
                return Err(EventStoreError::StoreFailure {
                    operation: Operation::AppendEvents,
                });
            }
        }

        {
            let mut index =
                self.shared
                    .index
                    .write()
                    .map_err(|_| EventStoreError::StoreFailure {
                        operation: Operation::AppendEvents,
                    })?;
            let _ = index.headers.insert(transaction_id, header);
            let mut new_entries: Vec<(Uuid, u64)> = Vec::with_capacity(new_events.len());
            for indexed in new_events {
                index
                    .streams
                    .entry(indexed.stream_id.clone())
                    .or_default()
                    .push(indexed.clone());
                let (_position, entry) = index.ingest_appended(indexed);
                new_entries.push(entry);
            }
            index.tips = vec![transaction_id];
            index
                .log
                .persist(&self.shared.roots, &new_entries)
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: Operation::AppendEvents,
                })?;
        }

        Ok(EventStreamSlice)
    }

    async fn load_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
    ) -> Result<Option<CommandStateSnapshot>, EventStoreError> {
        let path = self
            .shared
            .roots
            .snapshot_path(&snapshot_filename(&snapshot_id));
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                });
            }
        };
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })
    }

    async fn save_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
        snapshot: CommandStateSnapshot,
    ) -> Result<(), EventStoreError> {
        let _append = self.shared.append_lock.lock().await;
        let filename = snapshot_filename(&snapshot_id);
        let path = self.shared.roots.snapshot_path(&filename);

        match fs::read_to_string(&path) {
            Ok(contents) => {
                let stored: CommandStateSnapshot =
                    serde_json::from_str(&contents).map_err(|_| EventStoreError::StoreFailure {
                        operation: Operation::AppendEvents,
                    })?;
                if !snapshot.covers(&stored) {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(EventStoreError::StoreFailure {
                    operation: Operation::AppendEvents,
                });
            }
        }

        let body = serde_json::to_vec(&snapshot).map_err(|_| EventStoreError::StoreFailure {
            operation: Operation::AppendEvents,
        })?;
        let tmp_path = self.shared.roots.snapshot_tmp_path(&filename);
        let snapshots_dir = self.shared.roots.snapshots.clone();
        let fsync = self.shared.config.fsync;
        tokio::task::spawn_blocking(move || {
            write_snapshot_file(&tmp_path, &path, &snapshots_dir, &body, fsync)
        })
        .await
        .map_err(|_| EventStoreError::StoreFailure {
            operation: Operation::AppendEvents,
        })?
        .map_err(|_| EventStoreError::StoreFailure {
            operation: Operation::AppendEvents,
        })
    }
}

fn snapshot_filename(snapshot_id: &CommandStateSnapshotId) -> String {
    format!("{:x}", Sha256::digest(snapshot_id.as_ref().as_bytes()))
}

fn write_snapshot_file(
    tmp_path: &Path,
    final_path: &Path,
    snapshots_dir: &Path,
    body: &[u8],
    fsync: FsyncPolicy,
) -> std::io::Result<()> {
    {
        let mut file = File::create(tmp_path)?;
        file.write_all(body)?;
        if matches!(fsync, FsyncPolicy::Full) {
            file.sync_all()?;
        }
    }
    fs::rename(tmp_path, final_path)?;
    if matches!(fsync, FsyncPolicy::Full) {
        let dir = File::open(snapshots_dir)?;
        dir.sync_all()?;
    }
    Ok(())
}

impl EventReader for FileEventStore {
    type Error = EventStoreError;

    async fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        let index = self
            .shared
            .index
            .read()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })?;

        let after = page.after_position().map(|position| position.into_inner());
        let limit = page.limit().into_inner();
        let prefix = filter.stream_prefix();
        let pattern = filter.stream_pattern();
        let explicit_type = filter.event_type();

        // Pagination walks local-ingestion order (ADR-0043(c)): events are
        // delivered in the order this replica became aware of them, and the
        // cursor is the local-ingestion position — not the canonical event id.
        // A merge-introduced event sorts after everything previously ingested,
        // so a live projection never rewinds and never skips it.
        let events: Vec<(E, StreamPosition)> = index
            .by_ingestion
            .iter()
            .filter(|(position, _)| match after {
                None => true,
                Some(after_position) => *position > after_position,
            })
            .filter(|(_, indexed)| match prefix {
                None => true,
                Some(prefix) => indexed.stream_id.starts_with(prefix.as_ref()),
            })
            .filter(|(_, indexed)| match pattern {
                // Glob pattern filtering (ADR-0047) is applied at the query
                // level, before take(), so non-matching streams don't consume
                // batch slots.
                None => true,
                Some(pattern) => pattern.matches(&indexed.stream_id),
            })
            .filter(|(_, indexed)| {
                let wanted = explicit_type.unwrap_or_else(|| E::event_type_name());
                indexed.event_type == wanted
            })
            .take(limit)
            .filter_map(|(position, indexed)| {
                serde_json::from_value::<E>(indexed.event_data.clone())
                    .ok()
                    .map(|event| (event, StreamPosition::new(*position)))
            })
            .collect();

        Ok(events)
    }
}

impl FileEventStore {
    /// This working copy's current `replica_id` — the identity stamped on the
    /// transactions it writes next (ADR-0044). It is machine-local and
    /// gitignored; a fresh clone or a `cp -r` to a new path mints its own.
    pub fn replica_id(&self) -> Uuid {
        self.shared.replica_id
    }

    /// Detect divergences (forks) in the current — possibly git-merged —
    /// history. A fork is two or more concurrent transactions that each
    /// extended a stream from the same base version. Merge mode is
    /// file-store-specific and lives outside the cross-backend traits
    /// (ADR-0045).
    ///
    /// Returns [`FsEventStoreError::ReplicaIdentityConflict`] if two concurrent
    /// transactions carry the same `replica_id` — the copy trap manifested
    /// (ADR-0044) — rather than silently merging a corrupt history.
    pub fn detect_forks(&self) -> Result<Vec<Fork>, FsEventStoreError> {
        let index = self.read_index()?;
        detect_forks_in(&index.headers)
    }

    /// A snapshot of the store's reconciliation state: unresolved forks,
    /// dangling transactions (absent parents from a partial git merge), and
    /// transaction files rejected by the read-time fsck (ADR-0045/0046).
    pub fn status(&self) -> Result<StoreStatus, FsEventStoreError> {
        let index = self.read_index()?;
        let forks = detect_forks_in(&index.headers)?;
        let dangling = detect_dangling_in(&index.headers);
        let integrity_failures = index
            .integrity_failures
            .iter()
            .map(|(transaction_id, detail)| IntegrityFailure {
                transaction_id: TransactionId::new(*transaction_id),
                detail: detail.clone(),
            })
            .collect();
        Ok(StoreStatus {
            forks,
            dangling,
            integrity_failures,
        })
    }

    /// The number of structural merges (N-parent merge transactions) recorded.
    ///
    /// This counter increases whenever a `git merge` brings in — or a
    /// [`reconcile`](Self::reconcile) records — a merge transaction, i.e.
    /// whenever the canonical history may have had events inserted behind a
    /// projection's cursor. A projection that cannot guarantee its resolution
    /// events are idempotent can compare this value between polls and rebuild
    /// from zero when it changes (ADR-0043).
    pub fn topology_generation(&self) -> Result<usize, FsEventStoreError> {
        let index = self.read_index()?;
        Ok(index
            .headers
            .values()
            .filter(|header| header.parent_transaction_ids.len() > 1)
            .count())
    }

    /// Reconcile every unresolved fork using a domain-supplied resolver.
    ///
    /// For each fork the resolver receives the divergent branches (typed as
    /// `E`) and returns either resolution events — recorded as an N-parent
    /// merge transaction that collapses the fork — or `Unresolvable`. The
    /// resolver is the application's domain policy; the library owns only the
    /// mechanism (ADR-0042). The merge transaction lists the fork heads as its
    /// parents, so the fork is thereafter reported as resolved, and the merge
    /// file replicates through `git` like any other transaction.
    pub async fn reconcile<E, R>(&self, resolver: R) -> Result<ReconcileReport, FsEventStoreError>
    where
        E: Event,
        R: Fn(&ForkContext<E>) -> ResolutionOutcome<E>,
    {
        let _append = self.shared.append_lock.lock().await;
        let forks = {
            let index = self.read_index()?;
            detect_forks_in(&index.headers)?
        };

        let mut resolved = 0usize;
        let mut unresolved: Vec<StreamId> = Vec::new();
        for fork in forks {
            let context = self.build_fork_context::<E>(&fork)?;
            match resolver(&context) {
                ResolutionOutcome::Unresolvable(_reason) => {
                    unresolved.push(fork.stream_id().clone());
                }
                ResolutionOutcome::Resolve(events) => {
                    self.write_merge_transaction(&fork, &events)?;
                    resolved += 1;
                }
            }
        }
        Ok(ReconcileReport {
            resolved,
            unresolved,
        })
    }

    fn read_index(&self) -> Result<std::sync::RwLockReadGuard<'_, Index>, FsEventStoreError> {
        self.shared
            .index
            .read()
            .map_err(|_| FsEventStoreError::Corrupted {
                path: self.shared.roots.root.clone(),
                detail: "index lock poisoned".to_string(),
            })
    }

    fn build_fork_context<E: Event>(
        &self,
        fork: &Fork,
    ) -> Result<ForkContext<E>, FsEventStoreError> {
        let mut branches: Vec<BranchView<E>> = Vec::new();
        for transaction_id in fork.transactions() {
            let path = self.shared.roots.event_path((*transaction_id).into_inner());
            let (_header, envelopes) = parse_transaction(&path)?;
            let mut events: Vec<E> = Vec::new();
            for envelope in envelopes {
                if envelope.stream_id == fork.stream_id().as_ref() {
                    let event =
                        serde_json::from_value::<E>(envelope.event_data).map_err(|error| {
                            FsEventStoreError::Corrupted {
                                path: path.clone(),
                                detail: format!(
                                    "branch event does not match resolver type: {error}"
                                ),
                            }
                        })?;
                    events.push(event);
                }
            }
            branches.push(BranchView {
                transaction_id: *transaction_id,
                events,
            });
        }
        Ok(ForkContext {
            stream_id: fork.stream_id().clone(),
            base_version: fork.base_version(),
            branches,
        })
    }

    fn write_merge_transaction<E: Event>(
        &self,
        fork: &Fork,
        events: &[E],
    ) -> Result<(), FsEventStoreError> {
        let transaction_id = Uuid::now_v7();
        let parents: Vec<Uuid> = fork
            .transactions()
            .iter()
            .map(|transaction_id| (*transaction_id).into_inner())
            .collect();

        let mut items: Vec<(String, &'static str, serde_json::Value)> =
            Vec::with_capacity(events.len());
        for event in events {
            let event_data =
                serde_json::to_value(event).map_err(|error| FsEventStoreError::Corrupted {
                    path: self.shared.roots.root.clone(),
                    detail: format!("failed to serialize resolution event: {error}"),
                })?;
            items.push((
                event.stream_id().as_ref().to_string(),
                E::event_type_name(),
                event_data,
            ));
        }

        let (header, envelopes, new_events) = {
            let index = self.read_index()?;
            let mut stream_bases: BTreeMap<String, usize> = BTreeMap::new();
            for (key, _, _) in &items {
                let _ = stream_bases
                    .entry(key.clone())
                    .or_insert_with(|| index.stream_head(key));
            }
            let (envelopes, new_events) = build_envelopes(&index, items);
            let header = TransactionHeader {
                format_version: FORMAT_VERSION,
                transaction_id,
                replica_id: self.shared.replica_id,
                parent_transaction_ids: parents,
                created_at: chrono::Utc::now().to_rfc3339(),
                stream_bases,
                content_hash: None,
            };
            (header, envelopes, new_events)
        };

        let body = serialize_transaction(&header, &envelopes).map_err(|_| {
            FsEventStoreError::Corrupted {
                path: self.shared.roots.root.clone(),
                detail: "failed to serialize merge transaction".to_string(),
            }
        })?;
        let tmp_path = self.shared.roots.tmp_path(transaction_id);
        let final_path = self.shared.roots.event_path(transaction_id);
        write_transaction_file(
            &tmp_path,
            &final_path,
            &self.shared.roots.events,
            &body,
            self.shared.config.fsync,
        )
        .map_err(|source| FsEventStoreError::InitFailed {
            path: final_path.clone(),
            source,
        })?;

        let mut index = self
            .shared
            .index
            .write()
            .map_err(|_| FsEventStoreError::Corrupted {
                path: self.shared.roots.root.clone(),
                detail: "index lock poisoned".to_string(),
            })?;
        let _ = index.headers.insert(transaction_id, header);
        let mut new_entries: Vec<(Uuid, u64)> = Vec::with_capacity(new_events.len());
        for indexed in new_events {
            index
                .streams
                .entry(indexed.stream_id.clone())
                .or_default()
                .push(indexed.clone());
            let (_position, entry) = index.ingest_appended(indexed);
            new_entries.push(entry);
        }
        index.tips = compute_tips(&index.headers);
        index
            .log
            .persist(&self.shared.roots, &new_entries)
            .map_err(|error| FsEventStoreError::Corrupted {
                path: self.shared.roots.root.clone(),
                detail: format!("failed to persist ingestion log: {error}"),
            })?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
