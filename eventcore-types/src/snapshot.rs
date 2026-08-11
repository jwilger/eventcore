use std::collections::HashMap;

use nutype::nutype;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{StreamId, StreamVersion};

/// Stable identifier for a command-state projection.
///
/// Command authors choose this value to identify the state reconstructed for a
/// particular consistency boundary. It must include every command-specific
/// dimension that changes how the command folds state, including a state-schema
/// version when an older serialized form cannot be read.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Into,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct CommandStateSnapshotId(String);

/// A durable command-state read-model projection.
///
/// The serialized state is paired with the stream versions it represents so an
/// executor can catch it up from later events. Replay checkpoints retain the
/// grouped stream/discovery order needed when a tail in an earlier stream means
/// later streams must be replayed in full.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStateSnapshot {
    /// JSON representation of command state supplied by the command.
    pub state: Value,
    /// Per-stream versions included in `state`.
    pub stream_versions: HashMap<StreamId, StreamVersion>,
    /// Serialized states immediately after each stream in replay order.
    ///
    /// These checkpoints preserve grouped multi-stream replay semantics when an
    /// earlier stream receives new events: its tail can be folded into its own
    /// completed state before later streams are replayed from scratch.
    #[serde(default)]
    pub replay_checkpoints: Vec<CommandStateReplayCheckpoint>,
}

/// A command-state projection checkpoint after one stream was fully replayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStateReplayCheckpoint {
    /// Stream that was just completed in replay order.
    pub stream_id: StreamId,
    /// Serialized command state after that stream.
    pub state: Value,
    /// Version vector represented by `state`.
    pub stream_versions: HashMap<StreamId, StreamVersion>,
}

impl CommandStateSnapshot {
    /// Creates a snapshot from a serialized state and its stream version vector.
    pub fn new(state: Value, stream_versions: HashMap<StreamId, StreamVersion>) -> Self {
        Self {
            state,
            stream_versions,
            replay_checkpoints: Vec::new(),
        }
    }

    /// Adds replay-order checkpoints to this projection.
    pub fn with_replay_checkpoints(
        mut self,
        replay_checkpoints: Vec<CommandStateReplayCheckpoint>,
    ) -> Self {
        self.replay_checkpoints = replay_checkpoints;
        self
    }

    /// Returns whether this projection includes every stream version represented
    /// by `other`, preventing an older command attempt from regressing a newer
    /// durable projection.
    pub fn covers(&self, other: &Self) -> bool {
        other.stream_versions.iter().all(|(stream_id, version)| {
            self.stream_versions
                .get(stream_id)
                .is_some_and(|current| current >= version)
        })
    }
}
