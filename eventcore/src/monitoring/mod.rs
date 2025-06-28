//! Monitoring and observability infrastructure for EventCore.
//!
//! This module provides comprehensive metrics collection capabilities for tracking
//! command execution, event store operations, and projection processing performance.

#[allow(missing_docs)]
pub mod metrics;

pub use metrics::*;
