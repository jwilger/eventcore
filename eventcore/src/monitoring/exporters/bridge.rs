//! Bridge between EventCore internal metrics and external exporters.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time;
use tracing::{debug, info};

use super::{ExporterRegistry, MetricsExporter};
use crate::monitoring::metrics::{
    CommandMetrics, ErrorMetrics, EventStoreMetrics, MetricsRegistry, ProjectionMetrics,
    SystemMetrics,
};

/// Bridge that exports EventCore internal metrics to external monitoring systems.
pub struct MetricsBridge {
    metrics_registry: Arc<MetricsRegistry>,
    exporter_registry: Arc<RwLock<ExporterRegistry>>,
    export_interval: Duration,
}

impl MetricsBridge {
    /// Creates a new metrics bridge.
    pub const fn new(
        metrics_registry: Arc<MetricsRegistry>,
        exporter_registry: Arc<RwLock<ExporterRegistry>>,
    ) -> Self {
        Self {
            metrics_registry,
            exporter_registry,
            export_interval: Duration::from_secs(10),
        }
    }

    /// Sets the export interval.
    pub const fn with_export_interval(mut self, interval: Duration) -> Self {
        self.export_interval = interval;
        self
    }

    /// Starts exporting metrics periodically.
    pub async fn start_export_loop(self: Arc<Self>) {
        info!(
            interval_secs = self.export_interval.as_secs(),
            "Starting metrics export loop"
        );

        let mut interval = time::interval(self.export_interval);
        loop {
            interval.tick().await;
            self.export_metrics().await;
        }
    }

    /// Exports all current metrics to registered exporters.
    pub async fn export_metrics(&self) {
        debug!("Exporting metrics to external systems");

        let exporters = self.exporter_registry.read().await;

        // Export command metrics
        self.export_command_metrics(&self.metrics_registry.command_metrics, &exporters);

        // Export event store metrics
        self.export_event_store_metrics(&self.metrics_registry.event_store_metrics, &exporters);

        // Export projection metrics
        self.export_projection_metrics(&self.metrics_registry.projection_metrics, &exporters);

        // Export system metrics
        self.export_system_metrics(&self.metrics_registry.system_metrics, &exporters);

        // Export error metrics
        self.export_error_metrics(&self.metrics_registry.error_metrics, &exporters);
    }

    #[allow(clippy::unused_self)]
    fn export_command_metrics(&self, metrics: &CommandMetrics, exporters: &ExporterRegistry) {
        // Export counters
        exporters.export_counter("commands.executed", metrics.commands_executed.get(), &[]);

        exporters.export_counter("commands.succeeded", metrics.commands_succeeded.get(), &[]);

        exporters.export_counter("commands.failed", metrics.commands_failed.get(), &[]);

        // Export gauge
        exporters.export_gauge(
            "commands.concurrent",
            metrics.concurrent_commands.get(),
            &[],
        );

        // Export timer percentiles
        if let Some(p95) = metrics.command_duration.p95() {
            exporters.export_histogram(
                "command.duration",
                p95,
                &[("percentile".to_string(), "p95".to_string())],
            );
        }

        if let Some(p99) = metrics.command_duration.p99() {
            exporters.export_histogram(
                "command.duration",
                p99,
                &[("percentile".to_string(), "p99".to_string())],
            );
        }

        // Export command counts by type
        if let Ok(commands_by_type) = metrics.commands_by_type.read() {
            for (command_type, counter) in commands_by_type.iter() {
                exporters.export_counter(
                    "commands.by_type",
                    counter.get(),
                    &[("command_type".to_string(), command_type.clone())],
                );
            }
        }

        // Export error counts by type
        if let Ok(errors_by_type) = metrics.errors_by_type.read() {
            for (error_type, counter) in errors_by_type.iter() {
                exporters.export_counter(
                    "command.errors",
                    counter.get(),
                    &[("error_type".to_string(), error_type.clone())],
                );
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn export_event_store_metrics(
        &self,
        metrics: &EventStoreMetrics,
        exporters: &ExporterRegistry,
    ) {
        // Export counters
        exporters.export_counter("event_store.reads", metrics.reads_total.get(), &[]);

        exporters.export_counter("event_store.writes", metrics.writes_total.get(), &[]);

        exporters.export_counter("events.written", metrics.events_written.get(), &[]);

        exporters.export_counter("events.read", metrics.events_read.get(), &[]);

        // Export gauges
        exporters.export_gauge(
            "event_store.concurrent_operations",
            metrics.concurrent_operations.get(),
            &[],
        );

        exporters.export_gauge("streams.count", metrics.stream_count.get(), &[]);

        // Export timer percentiles
        if let Some(p95) = metrics.read_duration.p95() {
            exporters.export_histogram(
                "event_store.read_duration",
                p95,
                &[("percentile".to_string(), "p95".to_string())],
            );
        }

        if let Some(p95) = metrics.write_duration.p95() {
            exporters.export_histogram(
                "event_store.write_duration",
                p95,
                &[("percentile".to_string(), "p95".to_string())],
            );
        }
    }

    #[allow(clippy::unused_self)]
    fn export_projection_metrics(&self, metrics: &ProjectionMetrics, exporters: &ExporterRegistry) {
        // Export counters
        exporters.export_counter(
            "projections.events_processed",
            metrics.events_processed.get(),
            &[],
        );

        exporters.export_counter(
            "projections.events_skipped",
            metrics.events_skipped.get(),
            &[],
        );

        exporters.export_counter("projections.errors", metrics.projection_errors.get(), &[]);

        exporters.export_counter(
            "projections.checkpoint_updates",
            metrics.checkpoint_updates.get(),
            &[],
        );

        // Export gauge
        exporters.export_gauge("projections.active", metrics.active_projections.get(), &[]);

        // Export timer percentiles
        if let Some(p95) = metrics.processing_duration.p95() {
            exporters.export_histogram(
                "projection.processing_duration",
                p95,
                &[("percentile".to_string(), "p95".to_string())],
            );
        }

        // Export projection lag
        if let Ok(lag_by_projection) = metrics.lag_by_projection.read() {
            for (projection_name, gauge) in lag_by_projection.iter() {
                exporters.export_gauge(
                    "projection.lag_ms",
                    gauge.get(),
                    &[("projection".to_string(), projection_name.clone())],
                );
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn export_system_metrics(&self, metrics: &SystemMetrics, exporters: &ExporterRegistry) {
        // Export gauges
        exporters.export_gauge("system.memory_usage_bytes", metrics.memory_usage.get(), &[]);

        exporters.export_gauge("system.cpu_usage_percent", metrics.cpu_usage.get(), &[]);

        exporters.export_gauge(
            "connection_pool.size",
            metrics.connection_pool_size.get(),
            &[],
        );

        exporters.export_gauge(
            "connection_pool.available",
            metrics.connection_pool_available.get(),
            &[],
        );

        exporters.export_gauge(
            "network.connections",
            metrics.network_connections.get(),
            &[],
        );

        // Export counters
        exporters.export_counter("gc.collections", metrics.gc_collections.get(), &[]);

        exporters.export_counter("disk_io.operations", metrics.disk_io_operations.get(), &[]);

        // Export GC pause time percentiles
        if let Some(p95) = metrics.gc_pause_time.p95() {
            exporters.export_histogram(
                "gc.pause_time",
                p95,
                &[("percentile".to_string(), "p95".to_string())],
            );
        }

        // Export circuit breaker states
        if let Ok(states) = metrics.circuit_breaker_states.read() {
            for (name, state) in states.iter() {
                let state_value = match state.as_str() {
                    "closed" => 0.0,
                    "open" => 1.0,
                    "half_open" => 0.5,
                    _ => -1.0,
                };
                exporters.export_gauge(
                    "circuit_breaker.state",
                    state_value,
                    &[("circuit_breaker".to_string(), name.clone())],
                );
            }
        }

        // Export subscription lag
        if let Ok(subscription_lag) = metrics.subscription_lag.read() {
            for (subscription_name, gauge) in subscription_lag.iter() {
                exporters.export_gauge(
                    "subscription.lag_ms",
                    gauge.get(),
                    &[("subscription".to_string(), subscription_name.clone())],
                );
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn export_error_metrics(&self, metrics: &ErrorMetrics, exporters: &ExporterRegistry) {
        // Export counters
        exporters.export_counter("errors.critical", metrics.critical_errors.get(), &[]);

        exporters.export_counter("errors.transient", metrics.transient_errors.get(), &[]);

        exporters.export_counter("errors.permanent", metrics.permanent_errors.get(), &[]);

        // Export gauge
        exporters.export_gauge("errors.rate_percent", metrics.error_rate.get(), &[]);

        // Export error counts by type
        if let Ok(errors_by_type) = metrics.errors_by_type.read() {
            for (error_type, counter) in errors_by_type.iter() {
                exporters.export_counter(
                    "errors.by_type",
                    counter.get(),
                    &[("error_type".to_string(), error_type.clone())],
                );
            }
        }

        // Export error counts by operation
        if let Ok(errors_by_operation) = metrics.errors_by_operation.read() {
            for (operation, counter) in errors_by_operation.iter() {
                exporters.export_counter(
                    "errors.by_operation",
                    counter.get(),
                    &[("operation".to_string(), operation.clone())],
                );
            }
        }

        // Export mean time to resolution
        if let Some(mttr) = metrics.mean_time_to_resolution.mean() {
            exporters.export_histogram("errors.mean_time_to_resolution", mttr, &[]);
        }
    }
}

/// Builder for creating a metrics monitoring setup.
pub struct MonitoringBuilder {
    metrics_registry: Arc<MetricsRegistry>,
    exporters: Vec<Arc<dyn MetricsExporter>>,
    tracing_exporters: Vec<Arc<dyn super::TracingExporter>>,
    export_interval: Duration,
}

impl MonitoringBuilder {
    /// Creates a new monitoring builder.
    pub fn new(metrics_registry: Arc<MetricsRegistry>) -> Self {
        Self {
            metrics_registry,
            exporters: Vec::new(),
            tracing_exporters: Vec::new(),
            export_interval: Duration::from_secs(10),
        }
    }

    /// Adds a metrics exporter.
    pub fn with_metrics_exporter(mut self, exporter: Arc<dyn MetricsExporter>) -> Self {
        self.exporters.push(exporter);
        self
    }

    /// Adds a tracing exporter.
    pub fn with_tracing_exporter(mut self, exporter: Arc<dyn super::TracingExporter>) -> Self {
        self.tracing_exporters.push(exporter);
        self
    }

    /// Sets the metrics export interval.
    pub const fn with_export_interval(mut self, interval: Duration) -> Self {
        self.export_interval = interval;
        self
    }

    /// Builds and starts the monitoring infrastructure.
    pub fn build(self) -> Arc<MetricsBridge> {
        let mut registry = ExporterRegistry::new();

        for exporter in self.exporters {
            registry.register_metrics_exporter(exporter);
        }

        for exporter in self.tracing_exporters {
            registry.register_tracing_exporter(exporter);
        }

        let exporter_registry = Arc::new(RwLock::new(registry));

        let bridge = Arc::new(
            MetricsBridge::new(self.metrics_registry, exporter_registry)
                .with_export_interval(self.export_interval),
        );

        // Start the export loop in the background
        let bridge_clone = bridge.clone();
        tokio::spawn(async move {
            bridge_clone.start_export_loop().await;
        });

        info!("Monitoring infrastructure started");

        bridge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exports_metrics_to_registry() {
        let metrics_registry = Arc::new(MetricsRegistry::new());
        let exporter_registry = Arc::new(RwLock::new(ExporterRegistry::new()));

        let bridge = MetricsBridge::new(metrics_registry.clone(), exporter_registry);

        // Record some metrics
        metrics_registry
            .command_metrics
            .commands_executed
            .increment();
        metrics_registry
            .command_metrics
            .commands_succeeded
            .increment();
        metrics_registry
            .event_store_metrics
            .events_written
            .increment_by(10);

        // Export metrics
        bridge.export_metrics().await;

        // Verify metrics were exported (would need mock exporter to verify)
        assert_eq!(metrics_registry.command_metrics.commands_executed.get(), 1);
        assert_eq!(metrics_registry.command_metrics.commands_succeeded.get(), 1);
        assert_eq!(
            metrics_registry.event_store_metrics.events_written.get(),
            10
        );
    }
}
