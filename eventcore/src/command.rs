use crate::errors::CommandError;
use crate::types::StreamId;
use async_trait::async_trait;

/// Result type for command operations
pub type CommandResult<T> = Result<T, CommandError>;

/// Core trait for implementing event sourcing commands using the aggregate-per-command pattern.
///
/// Each command owns its state model and processing logic, eliminating traditional aggregate
/// boundaries in favor of self-contained commands that can read from and write to multiple
/// streams atomically.
///
/// # Type Parameters
///
/// * `Input` - The command input type, which must be self-validating through its construction.
///   No separate validate method is needed - if you have an Input, it's guaranteed to be valid.
/// * `State` - The state model that this command operates on. Must implement Default for
///   initialization and be thread-safe.
/// * `Event` - The event type that this command can produce.
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::command::{Command, CommandResult};
/// use eventcore::types::StreamId;
/// use async_trait::async_trait;
///
/// struct TransferMoney;
///
/// #[async_trait]
/// impl Command for TransferMoney {
///     type Input = TransferMoneyInput;
///     type State = AccountState;
///     type Event = AccountEvent;
///
///     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
///         vec![input.from_account_stream(), input.to_account_stream()]
///     }
///
///     fn apply(&self, state: &mut Self::State, event: &Self::Event) {
///         // Apply event to state
///     }
///
///     async fn handle(
///         &self,
///         state: Self::State,
///         input: Self::Input,
///     ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
///         // Business logic here
///         todo!()
///     }
/// }
/// ```
#[async_trait]
pub trait Command: Send + Sync {
    /// The input type for this command. Must be self-validating through its type construction.
    ///
    /// Input types should use smart constructors with validation at creation time. Once an
    /// Input instance exists, it's guaranteed to be valid throughout the system.
    type Input: Send + Sync;

    /// The state model that this command operates on.
    ///
    /// Must implement Default for initialization when no events exist for the streams.
    /// Must be thread-safe (Send + Sync) for concurrent processing.
    type State: Default + Send + Sync;

    /// The event type that this command can produce.
    ///
    /// Events represent the outcomes of command execution and are persisted to event streams.
    type Event: Send + Sync;

    /// Returns the stream IDs that this command needs to read from.
    ///
    /// This method is called during command execution to determine which streams need to be
    /// loaded and their events folded into the state model. The command executor uses this
    /// information for optimistic concurrency control.
    ///
    /// # Arguments
    ///
    /// * `input` - The validated command input
    ///
    /// # Returns
    ///
    /// A vector of stream IDs that must be read to execute this command.
    fn read_streams(&self, _input: &Self::Input) -> Vec<StreamId> {
        todo!("Implement read_streams method")
    }

    /// Applies an event to the command's state model.
    ///
    /// This method is called during state reconstruction to fold events from the relevant
    /// streams into the current state. It should be a pure function that modifies the state
    /// based on the event content.
    ///
    /// # Arguments
    ///
    /// * `state` - Mutable reference to the current state
    /// * `event` - The event to apply to the state
    fn apply(&self, _state: &mut Self::State, _event: &Self::Event) {
        todo!("Implement apply method")
    }

    /// Executes the command business logic.
    ///
    /// This method contains the core business logic of the command. It receives the current
    /// state (reconstructed from events) and the validated input, then produces a list of
    /// events to be written to specific streams.
    ///
    /// The method should be pure - no side effects other than the returned events. All I/O
    /// and persistence is handled by the command executor framework.
    ///
    /// # Arguments
    ///
    /// * `state` - The current state reconstructed from events
    /// * `input` - The validated command input
    ///
    /// # Returns
    ///
    /// A result containing a vector of (stream_id, event) pairs to be persisted atomically,
    /// or a CommandError if the command cannot be executed.
    async fn handle(
        &self,
        _state: Self::State,
        _input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
        todo!("Implement handle method")
    }
}
