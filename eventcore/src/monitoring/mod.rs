//! Monitoring and observability infrastructure for EventCore.
//!
//! This module provides comprehensive metrics collection capabilities for tracking
//! command execution, event store operations, and projection processing performance,
//! as well as health check functionality for system monitoring.

#[allow(missing_docs)]
pub mod health;
#[allow(missing_docs)]
pub mod metrics;
#[allow(missing_docs)]
pub mod resilience;

#[allow(unused_imports)]
pub use health::*;
#[allow(unused_imports)]
pub use metrics::*;
#[allow(unused_imports)]
pub use resilience::*;
