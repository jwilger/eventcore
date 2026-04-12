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
//! - Core traits: `Event`, `EventStore`, `CommandLogic`, `CommandStreams`, `StreamResolver`
//! - Domain types: `StreamId`, `StreamVersion`, `StreamWrites`, `StreamWriteEntry`
//! - Event handling: `EventStreamReader`, `EventStreamSlice`, `NewEvents`
//! - Command types: `StreamDeclarations`, `StreamDeclarationsError`
//! - Errors: `EventStoreError`, `CommandError`, `Operation`

mod command;
mod errors;
mod projection;
mod store;
mod validation;

pub use command::{
    CommandLogic, CommandStreams, Event, NewEvents, StreamDeclarations, StreamDeclarationsError,
    StreamResolver,
};
pub use errors::CommandError;
pub use projection::{
    AttemptNumber, BackoffMultiplier, BatchSize, CheckpointStore, DelayMilliseconds, EventFilter,
    EventPage, EventReader, FailureContext, FailureStrategy, MaxConsecutiveFailures, MaxRetries,
    MaxRetryAttempts, Projector, ProjectorCoordinator, RetryCount, StreamPosition,
};
pub use store::{
    EventStore, EventStoreError, EventStreamReader, EventStreamSlice, Operation, StreamId,
    StreamPrefix, StreamVersion, StreamWriteEntry, StreamWrites,
};
