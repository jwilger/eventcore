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
//! // Self-validating input type
//! #[nutype(validate(greater = 0))]
//! struct Money(u64);
//!
//! struct TransferMoneyInput {
//!     from_account: AccountId,
//!     to_account: AccountId,
//!     amount: Money,
//! }
//!
//! struct TransferMoney;
//!
//! #[async_trait]
//! impl Command for TransferMoney {
//!     type Input = TransferMoneyInput;
//!     type State = TransferState;
//!     type Event = TransferEvent;
//!     type StreamSet = (); // Phantom type for stream access control
//!
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         // Read from both account streams
//!         vec![
//!             StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
//!             StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
//!         ]
//!     }
//!
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         match &event.payload {
//!             TransferEvent::MoneyDebited { account, amount } => {
//!                 if account == &state.from_account {
//!                     state.from_balance -= amount;
//!                 }
//!             }
//!             TransferEvent::MoneyCredited { account, amount } => {
//!                 if account == &state.to_account {
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
//!         input: Self::Input,
//!         stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Check business rules
//!         if state.from_balance < input.amount {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 "Insufficient funds".to_string()
//!             ));
//!         }
//!
//!         // Return events for both streams with type-safe stream access
//!         Ok(vec![
//!             StreamWrite::new(
//!                 &read_streams,
//!                 StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
//!                 TransferEvent::MoneyDebited {
//!                     account: input.from_account,
//!                     amount: input.amount,
//!                 }
//!             )?,
//!             StreamWrite::new(
//!                 &read_streams,
//!                 StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
//!                 TransferEvent::MoneyCredited {
//!                     account: input.to_account,
//!                     amount: input.amount,
//!                 }
//!             )?,
//!         ])
//!     }
//! }
//! ```

use crate::errors::CommandError;
use crate::types::StreamId;
use async_trait::async_trait;
use std::collections::HashSet;
use std::marker::PhantomData;

/// A resolver that allows commands to dynamically request additional streams.
///
/// Commands receive this as a parameter and can call `add_streams()` to
/// dynamically expand their stream set. The executor will automatically
/// re-read streams when new ones are added.
pub struct StreamResolver {
    pub(crate) additional_streams: Vec<StreamId>,
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
    ///
    /// # Arguments
    ///
    /// * `streams` - Additional stream IDs to read
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In command.handle():
    /// // After analyzing the current state, request product streams
    /// let product_streams: Vec<StreamId> = state.order.items.keys()
    ///     .map(|id| StreamId::try_new(format!("product-{}", id)).unwrap())
    ///     .collect();
    /// stream_resolver.add_streams(product_streams);
    ///
    /// // The executor will automatically re-read and rebuild state
    /// ```
    pub fn add_streams(&mut self, streams: Vec<StreamId>) {
        for stream in streams {
            if !self.additional_streams.contains(&stream) {
                self.additional_streams.push(stream);
            }
        }
    }

    /// Get all additional streams that have been requested
    pub fn additional_streams(&self) -> &[StreamId] {
        &self.additional_streams
    }

    /// Check if any additional streams have been requested
    pub fn has_additional_streams(&self) -> bool {
        !self.additional_streams.is_empty()
    }
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for command operation results.
///
/// All command methods return this result type, which either contains the success value
/// or a `CommandError` describing what went wrong.
///
/// # Examples
///
/// ```rust,ignore
/// async fn execute_command() -> CommandResult<()> {
///     // Command logic here
///     Ok(())
/// }
/// ```
pub type CommandResult<T> = Result<T, CommandError>;

/// Type-safe representation of streams that a command has declared it will access.
///
/// This type ensures that commands can only write to streams they declared they would read from,
/// making it impossible to violate the concurrency control contract at compile time.
///
/// The `ReadStreams<S>` type uses phantom types to track which specific streams were declared
/// in the `read_streams` method, and only allows creating write events for those streams.
#[derive(Debug, Clone)]
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
    ///
    /// This is called internally by the command executor after calling `read_streams`.
    /// User code should not call this directly.
    pub(crate) fn new(stream_ids: Vec<StreamId>) -> Self {
        let stream_set = stream_ids.iter().cloned().collect();
        Self {
            stream_ids,
            stream_set,
            _phantom: PhantomData,
        }
    }

    /// Get the stream IDs that were declared for reading.
    ///
    /// This is used by the command executor to know which streams to read from
    /// and which stream versions to check for concurrency control.
    pub fn stream_ids(&self) -> &[StreamId] {
        &self.stream_ids
    }
}

/// A type-safe event write that can only target streams that were declared for reading.
///
/// This type ensures that commands cannot write to streams they didn't declare they would
/// access, preventing concurrency control violations at compile time.
#[derive(Debug, Clone)]
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

/// Core trait for implementing type-safe event sourcing commands.
///
/// The `Command` trait is the heart of EventCore's multi-stream event sourcing.
/// Each command implementation defines its own consistency boundary and can work
/// with multiple event streams atomically.
///
/// # Type Safety Guarantees
///
/// This trait provides compile-time and runtime guarantees that:
/// 1. Commands can only write to streams they declared they would read from
/// 2. All read streams are checked for version conflicts, not just written streams
/// 3. Concurrency control violations are impossible at the type level
///
/// # Design Philosophy
///
/// Commands in EventCore are self-contained units of business logic that:
/// - Define which streams they need to read
/// - Specify how to fold events into state
/// - Implement the business logic that produces new events
/// - Can work across multiple streams in a single atomic operation
/// - CANNOT write to streams they didn't declare for reading
///
/// # Type Parameters
///
/// * `Input` - The command's input type. Must be self-validating through smart constructors.
/// * `State` - The state model this command operates on. Must implement `Default`.
/// * `Event` - The event type this command produces.
/// * `StreamSet` - A phantom type representing the set of streams this command accesses.
///
/// # Implementation Guide
///
/// 1. **Input Types**: Use smart constructors with validation
/// 2. **State Types**: Keep them focused on what the command needs
/// 3. **Event Types**: Design for event sourcing, not current needs
/// 4. **Business Logic**: Keep it pure in the `handle` method
/// 5. **Stream Safety**: Use `StreamWrite::new()` to enforce access control
///
/// # Example: Order Placement
///
/// ```rust,ignore
/// use eventcore::command::{Command, CommandResult, ReadStreams, StreamResolver, StreamWrite};
/// use eventcore::types::StreamId;
/// use eventcore::event_store::StoredEvent;
/// use async_trait::async_trait;
///
/// // Command implementation
/// struct PlaceOrder;
///
/// #[async_trait]
/// impl Command for PlaceOrder {
///     type Input = PlaceOrderInput;  // Self-validating input
///     type State = OrderPlacementState;  // Focused state model
///     type Event = OrderEvent;  // Domain events
///     type StreamSet = (); // Phantom type for stream access control
///
///     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
///         // Read order, inventory, and customer streams
///         vec![
///             input.order_stream(),
///             input.inventory_stream(),
///             input.customer_stream(),
///         ]
///     }
///
///     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
///         // Update state based on events from any relevant stream
///         match &event.payload {
///             OrderEvent::ItemReserved { product_id, quantity } => {
///                 state.reserved_items.insert(*product_id, *quantity);
///             }
///             OrderEvent::CustomerValidated { customer_id } => {
///                 state.customer_validated = true;
///             }
///             // ... other events
///         }
///     }
///
///     async fn handle(
///         &self,
///         read_streams: ReadStreams<Self::StreamSet>,
///         state: Self::State,
///         input: Self::Input,
///         stream_resolver: &mut StreamResolver,
///     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///         // Pure business logic
///         if !state.customer_validated {
///             return Err(CommandError::Unauthorized("Invalid customer".into()));
///         }
///
///         // Check inventory
///         for item in &input.items {
///             if !state.has_sufficient_inventory(item) {
///                 return Err(CommandError::BusinessRuleViolation(
///                     format!("Insufficient inventory for {}", item.product_id)
///                 ));
///             }
///         }
///
///         // Generate events for multiple streams with type-safe access
///         let mut events = vec![];
///         
///         // Order stream events
///         events.push(StreamWrite::new(
///             &read_streams,
///             input.order_stream(),
///             OrderEvent::OrderPlaced { /* ... */ }
///         )?);
///
///         // Inventory stream events
///         for item in input.items {
///             events.push(StreamWrite::new(
///                 &read_streams,
///                 input.inventory_stream(),
///                 OrderEvent::ItemReserved {
///                     product_id: item.product_id,
///                     quantity: item.quantity,
///                 }
///             )?);
///         }
///
///         Ok(events)
///     }
/// }
/// ```

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
pub trait CommandStreams: Send + Sync {
    /// The input type for this command.
    ///
    /// For simple commands, this is often `Self`, meaning the command struct
    /// itself serves as the input. For more complex scenarios, a separate
    /// input type may be used.
    type Input: Send + Sync + Clone;

    /// A phantom type representing the set of streams this command accesses.
    ///
    /// Generated automatically by the `#[derive(Command)]` macro as `{CommandName}StreamSet`.
    type StreamSet: Send + Sync;

    /// Returns the stream IDs that this command needs to read from.
    ///
    /// This method is automatically implemented by the `#[derive(Command)]` macro
    /// based on fields marked with `#[stream]`.
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId>;
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
///         input: Self::Input,
///         stream_resolver: &mut StreamResolver,
///     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
///         // Business logic here
///     }
/// }
/// ```
#[async_trait]
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
        input: Self::Input,
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
///         input: Self::Input,
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
#[async_trait]
pub trait Command: CommandStreams + CommandLogic {
    // All associated types and methods are inherited from CommandStreams and CommandLogic
    // This trait now serves as a convenient combination of both traits
}
    ///
    /// ```rust,ignore
    /// use nutype::nutype;
    ///
    /// #[nutype(validate(not_empty, len_char_max = 100))]
    /// struct CustomerName(String);
    ///
    /// struct CreateCustomerInput {
    ///     name: CustomerName,  // Always valid if it exists
    ///     email: EmailAddress, // Another validated type
    /// }
    /// ```
    type Input: Send + Sync + Clone;

    /// The state model that this command operates on.
    ///
    /// The state type should contain only the data needed for this command's decisions.
    /// It must implement `Default` for initialization when streams are empty, and be
    /// thread-safe (`Send + Sync`) for concurrent processing.
    ///
    /// # Design Tips
    ///
    /// - Keep state focused on what the command needs
    /// - Don't try to model the entire aggregate
    /// - Include data from multiple streams if needed
    /// - Make fields optional if they might not be initialized
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(Default)]
    /// struct TransferState {
    ///     from_balance: Money,
    ///     to_balance: Money,
    ///     from_account_frozen: bool,
    ///     to_account_frozen: bool,
    /// }
    /// ```
    type State: Default + Send + Sync;

    /// The event type that this command can produce.
    ///
    /// Events represent facts about what happened, not commands or intentions.
    /// They should be named in past tense and contain all data needed to
    /// reconstruct state.
    ///
    /// # Design Guidelines
    ///
    /// - Use past tense: `OrderPlaced`, not `PlaceOrder`
    /// - Include all relevant data in the event
    /// - Events are immutable once created
    /// - Design for event sourcing, not just current needs
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// enum AccountEvent {
    ///     AccountOpened { owner: CustomerId, initial_balance: Money },
    ///     MoneyDeposited { amount: Money, reference: String },
    ///     MoneyWithdrawn { amount: Money, reference: String },
    ///     AccountFrozen { reason: String },
    /// }
    /// ```
    type Event: Send + Sync;

    /// Returns the stream IDs that this command needs to read from.
    ///
    /// This method defines the command's consistency boundary by specifying which
    /// streams contain events that must be considered. The command executor will
    /// load all events from these streams and fold them into the state before
    /// calling `handle`.
    ///
    /// # Concurrency Control
    ///
    /// The returned streams are tracked for optimistic concurrency control. If any
    /// of these streams are modified between reading and writing, the command will
    /// be retried with fresh state.
    ///
    /// # Arguments
    ///
    /// * `input` - The validated command input
    ///
    /// # Returns
    ///
    /// A vector of stream IDs that must be read. Can be empty if the command
    /// doesn't need to read any existing state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
    ///     vec![
    ///         StreamId::try_new(format!("account-{}", input.account_id)).unwrap(),
    ///         StreamId::try_new("system-config").unwrap(),
    ///     ]
    /// }
    /// ```
    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId>;

    /// Applies an event to the command's state model.
    ///
    /// This method is the core of event sourcing - it folds events into state.
    /// It's called for each event from the streams returned by `read_streams`,
    /// in chronological order.
    ///
    /// # Implementation Guidelines
    ///
    /// - Must be a pure function (no side effects)
    /// - Should handle events from any relevant stream
    /// - Must be idempotent - applying the same event twice shouldn't break
    /// - Should ignore events it doesn't understand (forward compatibility)
    ///
    /// # Arguments
    ///
    /// * `state` - Mutable reference to the current state
    /// * `stored_event` - The event with metadata (stream_id, version, timestamp, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
    ///     // Check which stream the event came from
    ///     let stream_id = &event.stream_id;
    ///     
    ///     match &event.payload {
    ///         AccountEvent::Deposited { amount } => {
    ///             if stream_id == &state.account_stream {
    ///                 state.balance += amount;
    ///             }
    ///         }
    ///         AccountEvent::Withdrawn { amount } => {
    ///             if stream_id == &state.account_stream {
    ///                 state.balance -= amount;
    ///             }
    ///         }
    ///         _ => {} // Ignore unknown events
    ///     }
    /// }
    /// ```
    fn apply(
        &self,
        state: &mut Self::State,
        stored_event: &crate::event_store::StoredEvent<Self::Event>,
    );

    /// Executes the command's business logic.
    ///
    /// This is where your domain logic lives. The method receives the current state
    /// (reconstructed by calling `apply` for each historical event) and the validated
    /// input, then decides what new events should be created.
    ///
    /// # Pure Function Requirement
    ///
    /// This method must be pure - no database calls, no API calls, no random numbers,
    /// no timestamps. All data must come from the state or input. This ensures:
    /// - Deterministic behavior
    /// - Easy testing
    /// - Safe retries
    ///
    /// # Return Value
    ///
    /// Returns a vector of (StreamId, Event) pairs. These will be written atomically
    /// to the event store. If any stream has been modified since reading, the entire
    /// command will be retried.
    ///
    /// # Error Handling
    ///
    /// Return specific `CommandError` variants to indicate why the command failed:
    /// - `BusinessRuleViolation` - Domain rules prevent this operation
    /// - `Unauthorized` - User lacks permission
    /// - `ValidationFailed` - Additional runtime validation failed
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// async fn handle(
    ///     &self,
    ///     state: Self::State,
    ///     input: Self::Input,
    /// ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
    ///     // Check business rules
    ///     if state.balance < input.amount {
    ///         return Err(CommandError::BusinessRuleViolation(
    ///             "Insufficient funds".to_string()
    ///         ));
    ///     }
    ///
    ///     // Create events
    ///     Ok(vec![(
    ///         input.account_stream(),
    ///         AccountEvent::MoneyWithdrawn {
    ///             amount: input.amount,
    ///             reference: input.reference,
    ///         }
    ///     )])
    /// }
    /// ```
    /// A phantom type that represents the set of streams this command accesses.
    ///
    /// This is used at the type level to ensure write safety. Define an empty struct for this.
    type StreamSet: Send + Sync;

    /// Executes the command's business logic with type-safe stream access.
    ///
    /// CRITICAL: This method receives:
    /// - `read_streams`: Type-safe representation of the declared streams
    /// - `state`: Reconstructed state from all declared streams  
    /// - `input`: The validated command input
    ///
    /// It must return `StreamWrite` instances that can only be created for declared streams.
    /// The executor will:
    /// 1. Write the events to their target streams
    /// 2. Check expected versions of ALL declared streams (read_streams), not just written ones
    /// 3. Retry if any declared stream has been modified since reading
    ///
    /// # Type Safety
    ///
    /// `StreamWrite::new()` ensures you can only write to streams declared in `read_streams()`.
    /// This makes it impossible to violate the concurrency control contract.
    ///
    /// # Complete Concurrency Control
    ///
    /// Expected versions are checked for ALL streams that were read, even if they're
    /// not being written to. This prevents commands from making decisions based on stale data.
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
}
