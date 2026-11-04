//! Dev-only contract types for cross-crate modeled mapping regressions.

use eventcore::ModelEvent;

/// An event payload owned by a separate contract crate.
#[derive(ModelEvent)]
pub struct RecordedAmount {
    /// The amount accepted by the executable mapping.
    pub amount: u64,
}

/// An enum payload owned by the same separate contract crate.
#[derive(ModelEvent)]
pub enum AuditEvent {
    /// The amount recorded by the executable mapping.
    Recorded(u64),
}
