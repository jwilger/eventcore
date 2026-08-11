//! Store configuration and the resolved on-disk directory layout.

use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::FsEventStoreError;

/// Whether to fsync written files and their directory for durability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FsyncPolicy {
    /// fsync the transaction file and the events directory on every append.
    #[default]
    Full,
    /// Skip fsync (faster; for tests or non-durable use).
    None,
}

/// Configuration for opening a [`crate::FileEventStore`].
#[derive(Debug, Clone)]
pub struct FsConfig {
    pub(crate) root: PathBuf,
    pub(crate) fsync: FsyncPolicy,
    pub(crate) replica_id: Option<Uuid>,
}

impl FsConfig {
    /// Create a config rooted at `root` with full fsync durability.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            fsync: FsyncPolicy::Full,
            replica_id: None,
        }
    }

    /// Override the fsync policy.
    pub fn with_fsync(mut self, policy: FsyncPolicy) -> Self {
        self.fsync = policy;
        self
    }

    /// Set this working copy's `replica_id` explicitly, bypassing the
    /// fingerprint-bound lazy generation (ADR-0044). Use this for environments
    /// where the filesystem identity is unreliable or replicas are provisioned
    /// deliberately (containers, CI). The operator is responsible for ensuring
    /// configured ids are genuinely distinct across independent writers; the
    /// reconcile-time collision check still applies as a backstop.
    pub fn with_replica_id(mut self, replica_id: Uuid) -> Self {
        self.replica_id = Some(replica_id);
        self
    }
}

/// Resolved paths under a store root. Only `events/` is committed to git;
/// `tmp/`, `index/`, and `.eventcore/` (including command-state snapshots),
/// plus the `.lock` file, are derived/local.
#[derive(Debug, Clone)]
pub(crate) struct Roots {
    pub(crate) root: PathBuf,
    pub(crate) events: PathBuf,
    pub(crate) tmp: PathBuf,
    pub(crate) eventcore: PathBuf,
    pub(crate) snapshots: PathBuf,
    pub(crate) index: PathBuf,
}

impl Roots {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            events: root.join("events"),
            tmp: root.join("tmp"),
            eventcore: root.join(".eventcore"),
            snapshots: root.join(".eventcore/snapshots"),
            index: root.join("index"),
        }
    }

    pub(crate) fn create_dirs(&self) -> Result<(), FsEventStoreError> {
        for dir in [
            &self.events,
            &self.tmp,
            &self.eventcore,
            &self.snapshots,
            &self.index,
        ] {
            fs::create_dir_all(dir).map_err(|source| FsEventStoreError::InitFailed {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }

    pub(crate) fn store_lock_path(&self) -> PathBuf {
        self.root.join(".lock")
    }

    pub(crate) fn replica_id_path(&self) -> PathBuf {
        self.eventcore.join("replica_id")
    }

    pub(crate) fn replica_fingerprint_path(&self) -> PathBuf {
        self.eventcore.join("replica_fingerprint")
    }

    pub(crate) fn ingestion_log_path(&self) -> PathBuf {
        self.index.join("ingestion.log")
    }

    pub(crate) fn snapshot_path(&self, filename: &str) -> PathBuf {
        self.snapshots.join(format!("{filename}.json"))
    }

    pub(crate) fn snapshot_tmp_path(&self, filename: &str) -> PathBuf {
        self.snapshots.join(format!("{filename}.json.tmp"))
    }

    pub(crate) fn event_path(&self, transaction_id: Uuid) -> PathBuf {
        self.events.join(format!("{transaction_id}.jsonl"))
    }

    pub(crate) fn tmp_path(&self, transaction_id: Uuid) -> PathBuf {
        self.tmp.join(format!("{transaction_id}.jsonl.tmp"))
    }
}
