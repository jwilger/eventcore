//! Command pattern implementation for EventCore.
//!
//! This module provides the core `Command` trait that implements multi-stream
//! event sourcing with dynamic consistency boundaries, eliminating traditional
//! aggregate boundaries in favor of self-contained commands.
//!
//! # Multi-Stream Event Sourcing
//!
//! Traditional event sourcing uses aggregates as consistency boundaries, where each aggregate
//! owns a single stream. Multi-stream event sourcing allows each command to define
//! its own consistency boundary and atomically read from and write to multiple streams.
//!
//! ## Benefits
//!
//! - **Flexibility**: Commands can work across multiple entities without complex sagas
//! - **Atomicity**: Multi-stream writes happen in a single transaction
//! - **Simplicity**: No need to design aggregate boundaries upfront
//! - **Evolution**: Easy to add new commands without restructuring existing streams
//!
//! # Example: Money Transfer
//!
//! ```rust,ignore
//! use eventcore::command::{Command, CommandResult, ReadStreams, StreamResolver, StreamWrite};
//! use eventcore::types::StreamId;
//! use eventcore::event_store::StoredEvent;
//! use async_trait::async_trait;
//! use nutype::nutype;
//!
//! // Self-validating money type
//! #[nutype(validate(greater = 0))]
//! struct Money(u64);
//!
//! #[derive(Clone)]
//! struct TransferMoney {
//!     from_account: AccountId,
//!     to_account: AccountId,
//!     amount: Money,
//! }
//!
//! #[async_trait]
//! impl Command for TransferMoney {
//!     type State = TransferState;
//!     type Event = TransferEvent;
//!     type StreamSet = (); // Phantom type for stream access control
//!
//!     fn read_streams(&self) -> Vec<StreamId> {
//!         // Read from both account streams
//!         vec![
//!             StreamId::try_new(format!("account-{}", self.from_account)).unwrap(),
//!             StreamId::try_new(format!("account-{}", self.to_account)).unwrap(),
//!         ]
//!     }
//!
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         match &event.payload {
//!             TransferEvent::MoneyDebited { account, amount } => {
//!                 if account == &self.from_account {
//!                     state.from_balance -= amount;
//!                 }
//!             }
//!             TransferEvent::MoneyCredited { account, amount } => {
//!                 if account == &self.to_account {
//!                     state.to_balance += amount;
//!                 }
//!             }
//!         }
//!     }
//!
//!     async fn handle(
//!         &self,
//!         read_streams: ReadStreams<Self::StreamSet>,
//!         state: Self::State,
//!         stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Check business rules
//!         if state.from_balance < self.amount {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 "Insufficient funds".to_string()
//!             ));
//!         }
//!
//!         // Return events for both streams with type-safe stream access
//!         Ok(vec![
//!             StreamWrite::new(
//!                 &read_streams,
//!                 StreamId::try_new(format!("account-{}", self.from_account)).unwrap(),
//!                 TransferEvent::MoneyDebited {
//!                     account: self.from_account.clone(),
//!                     amount: self.amount,
//!                 }
//!             )?,
//!             StreamWrite::new(
//!                 &read_streams,
//!                 StreamId::try_new(format!("account-{}", self.to_account)).unwrap(),
//!                 TransferEvent::MoneyCredited {
//!                     account: self.to_account.clone(),
//!                     amount: self.amount,
//!                 }
//!             )?,
//!         ])
//!     }
//! }
//! ```
//!
//! # Stream Access Control
//!
//! EventCore provides compile-time guarantees about stream access through phantom types.
//! Commands can only write to streams they've declared in `read_streams()`.
//!
//! # Dynamic Stream Discovery
//!
//! Commands can discover additional streams during execution by using the `stream_resolver`
//! parameter. The executor will automatically re-run the command with all requested streams.

use crate::errors::CommandError;
pub use crate::errors::CommandResult;
use crate::types::StreamId;
use std::collections::HashSet;
use std::marker::PhantomData;

/// Trait for defining command input and stream access patterns.
///
/// This trait is typically implemented by the `#[derive(Command)]` macro,
/// which automatically generates the implementation based on `#[stream]` field attributes.
///
/// # Example
///
/// ```rust,ignore
/// use eventcore_macros::Command;
/// use eventcore::types::StreamId;
///
/// #[derive(Command)]
/// struct TransferMoney {
///     #[stream]
///     from_account: StreamId,
///     #[stream]
///     to_account: StreamId,
///     amount: Money,
/// }
/// // This generates a complete CommandStreams implementation
/// ```
pub trait CommandStreams: Send + Sync + Clone {
    /// A phantom type representing the set of streams this command accesses.
    ///
    /// Generated automatically by the `#[derive(Command)]` macro as `{CommandName}StreamSet`.
    type StreamSet: Send + Sync;

    /// Returns the stream IDs that this command needs to read from.
    ///
    /// This method is automatically implemented by the `#[derive(Command)]` macro
    /// based on fields marked with `#[stream]`.
    fn read_streams(&self) -> Vec<StreamId>;
}

/// Trait for the domain logic portion of a command.
///
/// This trait must be manually implemented and contains the business logic
/// and event handling for your command.
///
/// # Example
///
/// ```rust,ignore
/// #[async_trait]
/// impl CommandLogic for TransferMoney {
///     type State = AccountBalances;
///     type Event = BankingEvent;
///
///     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
///         match &event.payload {
///             BankingEvent::MoneyTransferred { from, to, amount } => {
///                 state.debit(from, *amount);
///                 state.credit(to, *amount);
///             }
///         }
///     }
///
///     async fn handle(
///         &self,
///         read_streams: ReadStreams<Self::StreamSet>,
///         state: Self::State,
///         stream_resolver: &mut StreamResolver,
///     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///         // Business logic here
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait CommandLogic: CommandStreams {
    /// The state model that this command operates on.
    type State: Default + Send + Sync;

    /// The event type that this command can produce.
    type Event: Send + Sync;

    /// Applies an event to the command's state model.
    fn apply(
        &self,
        state: &mut Self::State,
        stored_event: &crate::event_store::StoredEvent<Self::Event>,
    );

    /// Executes the command's business logic.
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
}

/// The core command trait for EventCore's multi-stream event sourcing.
///
/// This trait combines `CommandStreams` (typically implemented via `#[derive(Command)]`)
/// and `CommandLogic` (manually implemented with your domain logic).
///
/// # New Simplified Usage
///
/// ```rust,ignore
/// use eventcore_macros::Command;
/// use eventcore::{CommandLogic, prelude::*};
///
/// #[derive(Command, Clone)]
/// struct TransferMoney {
///     #[stream]
///     from_account: StreamId,
///     #[stream]
///     to_account: StreamId,
///     amount: Money,
/// }
///
/// #[async_trait]
/// impl CommandLogic for TransferMoney {
///     type State = AccountBalances;
///     type Event = BankingEvent;
///
///     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
///         // Event folding logic
///     }
///
///     async fn handle(
///         &self,
///         read_streams: ReadStreams<Self::StreamSet>,
///         state: Self::State,
///         stream_resolver: &mut StreamResolver,
///     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///         // Business logic
///     }
/// }
/// ```
///
/// # Backward Compatibility
///
/// Existing code that directly implements `Command` will continue to work.
/// The trait still contains all the original associated types and methods.
#[async_trait::async_trait]
pub trait Command: CommandStreams + CommandLogic {
    // All associated types and methods are inherited from CommandStreams and CommandLogic
    // This trait now serves as a convenient combination of both traits
}

// Blanket implementation: anything that implements both CommandStreams and CommandLogic
// automatically implements Command
impl<T> Command for T where T: CommandStreams + CommandLogic {}

/// Type-safe wrapper for streams that were declared for reading.
///
/// This type ensures at compile time that commands can only write to streams
/// they declared they would read from in `read_streams()`.
pub struct ReadStreams<S> {
    /// The actual stream IDs that were declared for reading
    pub(crate) stream_ids: Vec<StreamId>,
    /// Pre-computed hash set for O(1) stream validation lookups
    pub(crate) stream_set: HashSet<StreamId>,
    /// Phantom data to track the stream set at the type level
    _phantom: PhantomData<S>,
}

impl<S> ReadStreams<S> {
    /// Create a new ReadStreams instance from the declared stream IDs.
    pub(crate) fn new(stream_ids: Vec<StreamId>) -> Self {
        let stream_set = stream_ids.iter().cloned().collect();
        Self {
            stream_ids,
            stream_set,
            _phantom: PhantomData,
        }
    }

    /// Get the declared stream IDs.
    pub fn stream_ids(&self) -> &[StreamId] {
        &self.stream_ids
    }
}

/// Represents a stream write operation with compile-time stream access validation.
///
/// This type can only be created for streams that were declared in `read_streams()`,
/// ensuring commands cannot write to undeclared streams.
pub struct StreamWrite<S, E> {
    /// The target stream for this write
    pub stream_id: StreamId,
    /// The event to write
    pub event: E,
    /// Phantom data to ensure this write is only valid for the declared stream set
    _phantom: PhantomData<S>,
}

impl<S, E> StreamWrite<S, E> {
    /// Create a new StreamWrite, but only if the target stream was declared for reading.
    ///
    /// This method performs a runtime check to ensure the target stream was in the
    /// original read set, making it impossible to write to undeclared streams.
    pub fn new(
        read_streams: &ReadStreams<S>,
        stream_id: StreamId,
        event: E,
    ) -> Result<Self, CommandError> {
        // Verify the stream was declared for reading using O(1) hash set lookup
        if !read_streams.stream_set.contains(&stream_id) {
            return Err(CommandError::ValidationFailed(format!(
                "Cannot write to stream '{stream_id}' - it was not declared in read_streams()"
            )));
        }

        Ok(Self {
            stream_id,
            event,
            _phantom: PhantomData,
        })
    }

    /// Extract the stream ID and event for writing.
    ///
    /// This is used internally by the command executor.
    pub fn into_parts(self) -> (StreamId, E) {
        (self.stream_id, self.event)
    }
}

/// Allows commands to dynamically request additional streams during execution.
///
/// The executor will automatically re-read all requested streams and rebuild state.
pub struct StreamResolver {
    pub(crate) additional_streams: Vec<StreamId>,
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamResolver {
    /// Create a new stream resolver
    pub const fn new() -> Self {
        Self {
            additional_streams: Vec::new(),
        }
    }

    /// Request additional streams to be read
    ///
    /// The command can call this method any number of times to dynamically
    /// discover and request additional streams. The executor will automatically
    /// re-read all streams (initial + additional) and rebuild the state.
    pub fn add_streams(&mut self, streams: Vec<StreamId>) {
        self.additional_streams.extend(streams);
    }

    /// Check if any additional streams were requested
    pub fn has_additional_streams(&self) -> bool {
        !self.additional_streams.is_empty()
    }

    /// Take the additional streams, clearing the internal list
    pub fn take_additional_streams(&mut self) -> Vec<StreamId> {
        std::mem::take(&mut self.additional_streams)
    }
}
