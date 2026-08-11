//! Shared vocabulary types and traits for the EventCore event sourcing library.
//!
//! This crate provides the foundational types that are shared between the main
//! `eventcore` crate and adapter implementations like `eventcore-postgres`.
//! By extracting these types into a separate crate, we enable feature flag-based
//! adapter re-exports without circular dependencies.
//!
//! # Overview
//!
//! This crate contains:
//! - **Command traits**: `CommandLogic`, `CommandStreams`, `StreamResolver`, `Event`, `NewEvents`
//! - **Command types**: `StreamDeclarations`, `StreamDeclarationsError`
//! - **Store trait**: `EventStore`, with `EventStoreError` and `Operation`
//! - **Stream types**: `StreamId`, `StreamVersion`, `StreamWrites`, `StreamWriteEntry`,
//!   `StreamPattern`, `StreamPrefix`
//! - **Event streaming**: `EventStream`, `EventStreamSlice`, `collect_events`
//! - **Projection traits**: `Projector`, `EventReader`, `CheckpointStore`, `ProjectorCoordinator`
//! - **Projection types**: `EventFilter`, `EventPage`, `StreamPosition`, `FailureContext`,
//!   `FailureStrategy`, `MaxRetries`, `BatchSize`, `DelayMilliseconds`, `BackoffMultiplier`
//! - **Errors**: `CommandError`, `BusinessRuleMessage`

mod command;
mod errors;
mod projection;
mod snapshot;
mod store;
mod validation;

pub use command::{
    CommandLogic, CommandStreams, Event, NewEvents, StreamDeclarations, StreamDeclarationsError,
    StreamResolver,
};
pub use errors::{BusinessRuleMessage, CommandError};
pub use projection::{
    AttemptNumber, BackoffMultiplier, BatchSize, CheckpointStore, DelayMilliseconds, EventFilter,
    EventPage, EventReader, FailureContext, FailureStrategy, MaxConsecutiveFailures, MaxRetries,
    MaxRetryAttempts, Projector, ProjectorCoordinator, RetryCount, StreamPosition,
};
pub use snapshot::{CommandStateReplayCheckpoint, CommandStateSnapshot, CommandStateSnapshotId};
pub use store::{
    EventStore, EventStoreError, EventStream, EventStreamSlice, Operation, StreamId, StreamPattern,
    StreamPrefix, StreamVersion, StreamWriteEntry, StreamWrites, collect_events,
};
