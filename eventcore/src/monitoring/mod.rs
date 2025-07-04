//! Monitoring and observability infrastructure for EventCore.
//!
//! This module provides comprehensive observability capabilities including:
//! - Detailed metrics collection for performance monitoring
//! - Distributed tracing with proper span hierarchy
//! - Structured logging with correlation IDs
//! - Health check functionality for system monitoring
//! - Performance alerting and monitoring guidelines
//! - Integration with external monitoring systems (OpenTelemetry, Prometheus)

#[allow(missing_docs)]
pub mod exporters;
#[allow(missing_docs)]
pub mod health;
#[allow(missing_docs)]
pub mod logging;
#[allow(missing_docs)]
pub mod metrics;
#[allow(missing_docs)]
pub mod resilience;
#[allow(missing_docs)]
pub mod tracing;

#[allow(unused_imports)]
pub use health::*;
#[allow(unused_imports)]
pub use logging::*;
#[allow(unused_imports)]
pub use metrics::*;
#[allow(unused_imports)]
pub use resilience::*;
#[allow(unused_imports)]
pub use tracing::*;
