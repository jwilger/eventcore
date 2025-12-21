#![forbid(
    dead_code,
    invalid_value,
    overflowing_literals,
    unconditional_recursion,
    unreachable_pub,
    unused_allocation,
    unsafe_code
)]
#![deny(
    bad_style,
    clippy::allow_attributes,
    deprecated,
    meta_variable_misuse,
    non_ascii_idents,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    rust_2018_idioms,
    rust_2021_compatibility,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_code,
    unused_assignments,
    unused_attributes,
    unused_extern_crates,
    unused_imports,
    unused_must_use,
    unused_mut,
    unused_parens,
    unused_qualifications,
    unused_results,
    unused_variables
)]

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
pub use projection::{EventReader, Projector, StreamPosition};
pub use store::{
    EventStore, EventStoreError, EventStreamReader, EventStreamSlice, Operation, StreamId,
    StreamVersion, StreamWriteEntry, StreamWrites,
};
