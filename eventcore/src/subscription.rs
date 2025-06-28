//! Event subscription system for processing events from event streams.
//!
//! This module provides the core abstractions for subscribing to and processing
//! events from the event store. Subscriptions can be used to build projections,
//! process managers, or any other event-driven components.

use crate::{
    errors::{EventStoreError, ProjectionError},
    event::StoredEvent,
    types::{EventId, StreamId},
};
use async_trait::async_trait;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Options for configuring how a subscription processes events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionOptions {
    /// Subscribe to all events from all streams starting from the beginning.
    CatchUpFromBeginning,
    /// Subscribe to all events from all streams starting from a specific position.
    CatchUpFromPosition(SubscriptionPosition),
    /// Subscribe to new events only (live subscription).
    LiveOnly,
    /// Subscribe to specific streams only, starting from the beginning.
    SpecificStreamsFromBeginning(SpecificStreamsMode),
    /// Subscribe to specific streams only, starting from a position.
    SpecificStreamsFromPosition(SpecificStreamsMode, SubscriptionPosition),
    /// Subscribe to all streams from a specific event position.
    AllStreams {
        /// Starting position for the subscription.
        from_position: Option<EventId>,
    },
    /// Subscribe to specific streams from a position.
    SpecificStreams {
        /// The streams to subscribe to.
        streams: Vec<StreamId>,
        /// Starting position for the subscription.
        from_position: Option<EventId>,
    },
}

/// Mode for subscribing to specific streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecificStreamsMode {
    /// Subscribe to events from specific named streams.
    Named,
    /// Subscribe to events from streams matching a pattern.
    Pattern,
}

/// Represents a position in the global event stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SubscriptionPosition {
    /// The last processed event ID.
    pub last_event_id: EventId,
    /// Per-stream checkpoints for tracking progress.
    pub stream_checkpoints: BTreeMap<StreamId, Checkpoint>,
}

impl SubscriptionPosition {
    /// Creates a new subscription position.
    pub const fn new(last_event_id: EventId) -> Self {
        Self {
            last_event_id,
            stream_checkpoints: BTreeMap::new(),
        }
    }

    /// Updates the checkpoint for a specific stream.
    pub fn update_checkpoint(&mut self, stream_id: StreamId, checkpoint: Checkpoint) {
        self.stream_checkpoints.insert(stream_id, checkpoint);
    }

    /// Gets the checkpoint for a specific stream.
    pub fn get_checkpoint(&self, stream_id: &StreamId) -> Option<&Checkpoint> {
        self.stream_checkpoints.get(stream_id)
    }
}

/// Checkpoint tracking for a specific stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The event ID of the last processed event in this stream.
    pub event_id: EventId,
    /// The version number of the last processed event.
    pub version: u64,
}

impl Checkpoint {
    /// Creates a new checkpoint.
    pub const fn new(event_id: EventId, version: u64) -> Self {
        Self { event_id, version }
    }
}

/// Name for a subscription, used for checkpoint storage and identification.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Serialize,
        Deserialize
    )
)]
pub struct SubscriptionName(String);

/// Result type for subscription operations.
pub type SubscriptionResult<T> = Result<T, SubscriptionError>;

/// Errors that can occur during subscription processing.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Event store error occurred.
    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// Projection error occurred.
    #[error("Projection error: {0}")]
    Projection(#[from] ProjectionError),

    /// Subscription was cancelled.
    #[error("Subscription cancelled")]
    Cancelled,

    /// Failed to save checkpoint.
    #[error("Failed to save checkpoint: {0}")]
    CheckpointSaveFailed(String),

    /// Failed to load checkpoint.
    #[error("Failed to load checkpoint: {0}")]
    CheckpointLoadFailed(String),
}

/// Trait for processing events from a subscription.
#[async_trait]
pub trait EventProcessor: Send + Sync {
    /// The type of events this processor handles.
    type Event: Send + Sync;

    /// Processes a single event.
    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq;

    /// Called when the subscription catches up to the live position.
    async fn on_live(&mut self) -> SubscriptionResult<()> {
        Ok(())
    }
}

/// Trait for managing event subscriptions.
#[async_trait]
pub trait Subscription: Send + Sync {
    /// The type of events this subscription handles.
    type Event: Send + Sync;

    /// Starts the subscription with the given processor.
    async fn start(
        &mut self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq;

    /// Stops the subscription.
    async fn stop(&mut self) -> SubscriptionResult<()>;

    /// Pauses the subscription.
    async fn pause(&mut self) -> SubscriptionResult<()>;

    /// Resumes the subscription.
    async fn resume(&mut self) -> SubscriptionResult<()>;

    /// Gets the current position of the subscription.
    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>>;

    /// Saves a checkpoint for the subscription.
    async fn save_checkpoint(&mut self, position: SubscriptionPosition) -> SubscriptionResult<()>;

    /// Loads the last saved checkpoint for the subscription.
    async fn load_checkpoint(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>>;
}

/// Stub implementation for subscription functionality.
pub struct SubscriptionImpl<E> {
    _phantom: std::marker::PhantomData<E>,
}

impl<E> Default for SubscriptionImpl<E> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<E> SubscriptionImpl<E> {
    /// Creates a new subscription implementation.
    pub const fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<E> Subscription for SubscriptionImpl<E>
where
    E: Send + Sync,
{
    type Event = E;

    async fn start(
        &mut self,
        _name: SubscriptionName,
        _options: SubscriptionOptions,
        _processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq,
    {
        todo!("Implement subscription start")
    }

    async fn stop(&mut self) -> SubscriptionResult<()> {
        todo!("Implement subscription stop")
    }

    async fn pause(&mut self) -> SubscriptionResult<()> {
        todo!("Implement subscription pause")
    }

    async fn resume(&mut self) -> SubscriptionResult<()> {
        todo!("Implement subscription resume")
    }

    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>> {
        todo!("Implement get position")
    }

    async fn save_checkpoint(&mut self, _position: SubscriptionPosition) -> SubscriptionResult<()> {
        todo!("Implement save checkpoint")
    }

    async fn load_checkpoint(
        &self,
        _name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>> {
        todo!("Implement load checkpoint")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    // Property test generators
    prop_compose! {
        fn arb_event_id()(uuid in any::<u128>()) -> EventId {
            let uuid = Uuid::from_u128(uuid);
            // Create a UUIDv7 with sortrand version
            let mut bytes = *uuid.as_bytes();
            // Set version bits for UUIDv7 (0111 in the version field)
            bytes[6] = (bytes[6] & 0x0F) | 0x70;
            // Set variant bits
            bytes[8] = (bytes[8] & 0x3F) | 0x80;
            EventId::try_new(Uuid::from_bytes(bytes)).unwrap()
        }
    }

    prop_compose! {
        fn arb_stream_id()(s in "[a-zA-Z0-9_-]{1,50}") -> StreamId {
            StreamId::try_new(s).unwrap()
        }
    }

    prop_compose! {
        fn arb_checkpoint()(
            event_id in arb_event_id(),
            version in 0u64..1_000_000
        ) -> Checkpoint {
            Checkpoint::new(event_id, version)
        }
    }

    prop_compose! {
        fn arb_subscription_position()(
            last_event_id in arb_event_id(),
            checkpoints in prop::collection::btree_map(
                arb_stream_id(),
                arb_checkpoint(),
                0..10
            )
        ) -> SubscriptionPosition {
            let mut pos = SubscriptionPosition::new(last_event_id);
            for (stream_id, checkpoint) in checkpoints {
                pos.update_checkpoint(stream_id, checkpoint);
            }
            pos
        }
    }

    proptest! {
        #[test]
        fn subscription_position_ordering_respects_event_id(
            pos1 in arb_subscription_position(),
            pos2 in arb_subscription_position()
        ) {
            // Subscription positions should be ordered by their last_event_id
            let ordering = pos1.cmp(&pos2);
            let event_ordering = pos1.last_event_id.cmp(&pos2.last_event_id);
            prop_assert_eq!(ordering, event_ordering);
        }

        #[test]
        fn checkpoint_ordering_respects_event_id_then_version(
            checkpoint1 in arb_checkpoint(),
            checkpoint2 in arb_checkpoint()
        ) {
            // Checkpoints should be ordered first by event_id, then by version
            let ordering = checkpoint1.cmp(&checkpoint2);
            let expected = match checkpoint1.event_id.cmp(&checkpoint2.event_id) {
                std::cmp::Ordering::Equal => checkpoint1.version.cmp(&checkpoint2.version),
                other => other,
            };
            prop_assert_eq!(ordering, expected);
        }

        #[test]
        fn subscription_position_update_checkpoint_updates_correctly(
            mut position in arb_subscription_position(),
            stream_id in arb_stream_id(),
            checkpoint in arb_checkpoint()
        ) {
            position.update_checkpoint(stream_id.clone(), checkpoint);
            prop_assert_eq!(position.get_checkpoint(&stream_id), Some(&checkpoint));
        }

        #[test]
        fn subscription_position_get_checkpoint_returns_none_for_missing_stream(
            position in arb_subscription_position(),
            missing_stream in arb_stream_id()
        ) {
            // Ensure the missing stream is not in the position
            prop_assume!(!position.stream_checkpoints.contains_key(&missing_stream));
            prop_assert_eq!(position.get_checkpoint(&missing_stream), None);
        }
    }

    #[test]
    fn test_subscription_name_validation() {
        // Valid names
        assert!(SubscriptionName::try_new("valid_name").is_ok());
        assert!(SubscriptionName::try_new("projection-1").is_ok());
        assert!(SubscriptionName::try_new("MyProjection").is_ok());

        // Invalid names
        assert!(SubscriptionName::try_new("").is_err()); // Empty
        assert!(SubscriptionName::try_new("   ").is_err()); // Only whitespace
        assert!(SubscriptionName::try_new("a".repeat(256)).is_err()); // Too long
    }

    #[test]
    fn test_subscription_options() {
        // Test serialization/deserialization roundtrip
        let options = vec![
            SubscriptionOptions::CatchUpFromBeginning,
            SubscriptionOptions::LiveOnly,
            SubscriptionOptions::SpecificStreamsFromBeginning(SpecificStreamsMode::Named),
            SubscriptionOptions::SpecificStreamsFromBeginning(SpecificStreamsMode::Pattern),
        ];

        for opt in options {
            let serialized = serde_json::to_string(&opt).unwrap();
            let deserialized: SubscriptionOptions = serde_json::from_str(&serialized).unwrap();
            assert_eq!(opt, deserialized);
        }
    }
}
