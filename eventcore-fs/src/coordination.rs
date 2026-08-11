//! Locking and projector coordination (ADR-0040): the cross-process store
//! lock, the checkpoint store, and per-subscription advisory leadership locks.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use eventcore_types::{CheckpointStore, ProjectorCoordinator, StreamPosition};
use uuid::Uuid;

use crate::error::{FsCheckpointError, FsCoordinationError, FsEventStoreError};

/// Encode a subscription name into a collision-free, filesystem-safe stem by
/// hex-encoding its UTF-8 bytes. Injective: distinct names never collide.
fn sanitize(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len() * 2);
    for byte in name.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

/// Holds an exclusive advisory lock on `<root>/.lock` for the store's lifetime.
/// The lock is released by the OS when the `File` is dropped.
#[derive(Debug)]
pub(crate) struct StoreLockGuard {
    _file: File,
}

impl StoreLockGuard {
    pub(crate) fn acquire(path: &Path) -> Result<Self, FsEventStoreError> {
        let file = File::create(path).map_err(|source| FsEventStoreError::InitFailed {
            path: path.to_path_buf(),
            source,
        })?;
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(Self { _file: file }),
            Err(error) => Err(map_try_lock_to_store_locked(error, path)),
        }
    }
}

fn map_try_lock_to_store_locked(error: fs4::TryLockError, path: &Path) -> FsEventStoreError {
    match error {
        fs4::TryLockError::WouldBlock => FsEventStoreError::StoreLocked {
            path: path.to_path_buf(),
        },
        fs4::TryLockError::Error(source) => FsEventStoreError::InitFailed {
            path: path.to_path_buf(),
            source,
        },
    }
}

/// A file-based [`CheckpointStore`]: one JSON file per subscription.
#[derive(Debug, Clone)]
pub struct FileCheckpointStore {
    dir: PathBuf,
}

impl FileCheckpointStore {
    /// Open (or create) a checkpoint store under `<root>/checkpoints`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, FsCheckpointError> {
        let dir = root.as_ref().join("checkpoints");
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", sanitize(name)))
    }
}

impl CheckpointStore for FileCheckpointStore {
    type Error = FsCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let path = self.path(name);
        match fs::read_to_string(&path) {
            Ok(contents) => {
                let uuid = Uuid::parse_str(contents.trim()).map_err(|error| {
                    FsCheckpointError::Corrupted {
                        path: path.clone(),
                        detail: error.to_string(),
                    }
                })?;
                Ok(Some(StreamPosition::new(uuid)))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(FsCheckpointError::Io(error)),
        }
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let path = self.path(name);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, position.into_inner().to_string())?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

/// A file-based [`ProjectorCoordinator`]: per-subscription OS advisory locks.
#[derive(Debug, Clone)]
pub struct FileProjectorCoordinator {
    dir: PathBuf,
}

impl FileProjectorCoordinator {
    /// Open (or create) a coordinator under `<root>/locks`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, FsCoordinationError> {
        let dir = root.as_ref().join("locks");
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl ProjectorCoordinator for FileProjectorCoordinator {
    type Error = FsCoordinationError;
    type Guard = FileLeadershipGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        let path = self
            .dir
            .join(format!("{}.lock", sanitize(subscription_name)));
        let file = File::create(&path)?;
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(FileLeadershipGuard { _file: file }),
            Err(fs4::TryLockError::WouldBlock) => Err(FsCoordinationError::LeadershipNotAcquired {
                subscription_name: subscription_name.to_string(),
            }),
            Err(fs4::TryLockError::Error(error)) => Err(FsCoordinationError::Io(error)),
        }
    }
}

/// Leadership guard. Dropping it closes the file, releasing the advisory lock.
#[derive(Debug)]
pub struct FileLeadershipGuard {
    _file: File,
}

#[cfg(test)]
#[path = "coordination.test.rs"]
mod tests;
