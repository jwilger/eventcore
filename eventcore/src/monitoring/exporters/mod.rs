//! Export interfaces for integrating EventCore metrics with external monitoring systems.
//!
//! This module provides trait definitions and implementations for exporting EventCore's
//! internal metrics and traces to popular observability platforms like OpenTelemetry
//! and Prometheus.

use std::sync::Arc;
use std::time::Duration;

use crate::monitoring::TraceContext;

#[cfg(feature = "opentelemetry")]
pub mod opentelemetry;

#[cfg(feature = "prometheus")]
pub mod prometheus;

pub mod bridge;

/// Trait for exporting metrics to external monitoring systems.
///
/// Implementations of this trait bridge EventCore's internal metrics
/// to external monitoring platforms, allowing seamless integration
/// with existing observability infrastructure.
pub trait MetricsExporter: Send + Sync {
    /// Exports a counter metric value.
    ///
    /// # Arguments
    /// * `name` - The metric name (e.g., "commands_executed")
    /// * `value` - The counter value
    /// * `labels` - Key-value pairs for metric labels/tags
    fn export_counter(&self, name: &str, value: u64, labels: &[(String, String)]);

    /// Exports a gauge metric value.
    ///
    /// # Arguments
    /// * `name` - The metric name (e.g., "concurrent_operations")
    /// * `value` - The gauge value
    /// * `labels` - Key-value pairs for metric labels/tags
    fn export_gauge(&self, name: &str, value: f64, labels: &[(String, String)]);

    /// Exports a histogram/timer metric value.
    ///
    /// # Arguments
    /// * `name` - The metric name (e.g., "command_duration")
    /// * `value` - The duration to record
    /// * `labels` - Key-value pairs for metric labels/tags
    fn export_histogram(&self, name: &str, value: Duration, labels: &[(String, String)]);
}

/// Trait for exporting traces to external tracing systems.
///
/// Implementations of this trait bridge EventCore's internal tracing
/// to external distributed tracing platforms.
pub trait TracingExporter: Send + Sync {
    /// Exports a trace span.
    ///
    /// # Arguments
    /// * `context` - The trace context containing span information
    fn export_span(&self, context: &TraceContext);
}

/// Registry for managing multiple exporters.
///
/// Allows EventCore to export metrics and traces to multiple
/// backends simultaneously.
pub struct ExporterRegistry {
    metrics_exporters: Vec<Arc<dyn MetricsExporter>>,
    tracing_exporters: Vec<Arc<dyn TracingExporter>>,
}

impl ExporterRegistry {
    /// Creates a new empty exporter registry.
    pub fn new() -> Self {
        Self {
            metrics_exporters: Vec::new(),
            tracing_exporters: Vec::new(),
        }
    }

    /// Registers a metrics exporter.
    pub fn register_metrics_exporter(&mut self, exporter: Arc<dyn MetricsExporter>) {
        self.metrics_exporters.push(exporter);
    }

    /// Registers a tracing exporter.
    pub fn register_tracing_exporter(&mut self, exporter: Arc<dyn TracingExporter>) {
        self.tracing_exporters.push(exporter);
    }

    /// Exports a counter metric to all registered exporters.
    pub fn export_counter(&self, name: &str, value: u64, labels: &[(String, String)]) {
        for exporter in &self.metrics_exporters {
            exporter.export_counter(name, value, labels);
        }
    }

    /// Exports a gauge metric to all registered exporters.
    pub fn export_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
        for exporter in &self.metrics_exporters {
            exporter.export_gauge(name, value, labels);
        }
    }

    /// Exports a histogram metric to all registered exporters.
    pub fn export_histogram(&self, name: &str, value: Duration, labels: &[(String, String)]) {
        for exporter in &self.metrics_exporters {
            exporter.export_histogram(name, value, labels);
        }
    }

    /// Exports a trace span to all registered exporters.
    pub fn export_span(&self, context: &TraceContext) {
        for exporter in &self.tracing_exporters {
            exporter.export_span(context);
        }
    }
}

impl Default for ExporterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for metric and trace exporters.
#[derive(Debug, Clone)]
pub struct ExporterConfig {
    /// Service name for identification in monitoring systems
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Environment (e.g., "production", "staging")
    pub environment: String,
    /// Additional global labels/tags
    pub global_labels: Vec<(String, String)>,
}

impl Default for ExporterConfig {
    fn default() -> Self {
        Self {
            service_name: "eventcore-service".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            environment: "development".to_string(),
            global_labels: Vec::new(),
        }
    }
}
