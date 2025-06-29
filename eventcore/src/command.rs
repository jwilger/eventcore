//! Command pattern implementation for EventCore.
//!
//! This module provides the core `Command` trait that implements the aggregate-per-command
//! pattern, a revolutionary approach to event sourcing that eliminates traditional aggregate
//! boundaries in favor of self-contained commands.
//!
//! # The Aggregate-Per-Command Pattern
//!
//! Traditional event sourcing uses aggregates as consistency boundaries, where each aggregate
//! owns a single stream. The aggregate-per-command pattern inverts this: each command defines
//! its own consistency boundary and can atomically read from and write to multiple streams.
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
//! use eventcore::command::{Command, CommandResult};
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
//!         state: Self::State,
//!         input: Self::Input,
//!     ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
//!         // Check business rules
//!         if state.from_balance < input.amount {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 "Insufficient funds".to_string()
//!             ));
//!         }
//!
//!         // Return events for both streams
//!         Ok(vec![
//!             (
//!                 StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
//!                 TransferEvent::MoneyDebited {
//!                     account: input.from_account,
//!                     amount: input.amount,
//!                 }
//!             ),
//!             (
//!                 StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
//!                 TransferEvent::MoneyCredited {
//!                     account: input.to_account,
//!                     amount: input.amount,
//!                 }
//!             ),
//!         ])
//!     }
//! }
//! ```

use crate::errors::CommandError;
use crate::types::StreamId;
use async_trait::async_trait;

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

/// Core trait for implementing event sourcing commands.
///
/// The `Command` trait is the heart of EventCore's aggregate-per-command pattern.
/// Each command implementation defines its own consistency boundary and can work
/// with multiple event streams atomically.
///
/// # Design Philosophy
///
/// Commands in EventCore are self-contained units of business logic that:
/// - Define which streams they need to read
/// - Specify how to fold events into state
/// - Implement the business logic that produces new events
/// - Can work across multiple streams in a single atomic operation
///
/// # Type Parameters
///
/// * `Input` - The command's input type. Must be self-validating through smart constructors.
///   If an instance exists, it's guaranteed to be valid.
/// * `State` - The state model this command operates on. Must implement `Default` for
///   initialization when streams are empty.
/// * `Event` - The event type this command produces.
///
/// # Implementation Guide
///
/// 1. **Input Types**: Use smart constructors with validation
/// 2. **State Types**: Keep them focused on what the command needs
/// 3. **Event Types**: Design for event sourcing, not current needs
/// 4. **Business Logic**: Keep it pure in the `handle` method
///
/// # Example: Order Placement
///
/// ```rust,ignore
/// use eventcore::command::{Command, CommandResult};
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
///         state: Self::State,
///         input: Self::Input,
///     ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
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
///         // Generate events for multiple streams
///         let mut events = vec![];
///         
///         // Order stream events
///         events.push((
///             input.order_stream(),
///             OrderEvent::OrderPlaced { /* ... */ }
///         ));
///
///         // Inventory stream events
///         for item in input.items {
///             events.push((
///                 input.inventory_stream(),
///                 OrderEvent::ItemReserved {
///                     product_id: item.product_id,
///                     quantity: item.quantity,
///                 }
///             ));
///         }
///
///         Ok(events)
///     }
/// }
/// ```
#[async_trait]
pub trait Command: Send + Sync {
    /// The input type for this command.
    ///
    /// Input types must be self-validating through smart constructors. Use types like
    /// `nutype` to ensure validation happens at construction time. Once an Input instance
    /// exists, it's guaranteed to be valid throughout the system.
    ///
    /// # Example
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
    type Input: Send + Sync;

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
    async fn handle(
        &self,
        state: Self::State,
        input: Self::Input,
    ) -> CommandResult<Vec<(StreamId, Self::Event)>>;
}
