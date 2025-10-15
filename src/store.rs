/// Trait defining the contract for event store implementations.
///
/// Event stores are responsible for:
/// - Persisting events to streams with optimistic concurrency control
/// - Loading events from streams for state reconstruction
/// - Resolving stream identifiers
///
/// Implementations include:
/// - `eventcore-postgres`: Production PostgreSQL backend
/// - `eventcore-memory`: In-memory backend for testing
pub trait EventStore {}