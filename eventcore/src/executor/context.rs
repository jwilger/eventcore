//! Execution context management for command execution.
//! 
//! This module contains the ExecutionContext type and related functionality
//! for managing correlation IDs, user IDs, and metadata during command execution.

use std::collections::HashMap;

/// Context information for command execution.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Correlation ID for request tracing.
    pub correlation_id: String,
    /// User ID for auditing.
    pub user_id: Option<String>,
    /// Additional metadata for the execution.
    pub metadata: HashMap<String, String>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            correlation_id: uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string(),
            user_id: None,
            metadata: HashMap::new(),
        }
    }
}