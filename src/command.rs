/// Trait defining the behavior of a command.
///
/// Commands encapsulate business operations that read from multiple event streams,
/// validate business rules, and produce events to be written back to streams.
///
/// Implementers define:
/// - The state type accumulated from events
/// - How to apply events to build state (`apply()`)
/// - Business logic and event production (`handle()`)
pub trait CommandLogic {}