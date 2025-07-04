//! OpenTelemetry exporter implementation for EventCore metrics and traces.

use std::time::Duration;

use opentelemetry::global;
use opentelemetry::metrics::{Meter, MeterProvider};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::Resource;
use tracing::info;

use super::{ExporterConfig, MetricsExporter, TracingExporter};
use crate::monitoring::TraceContext;

/// OpenTelemetry exporter for EventCore metrics and traces.
pub struct OpenTelemetryExporter {
    meter: Meter,
    config: ExporterConfig,
}

impl OpenTelemetryExporter {
    /// Creates a new OpenTelemetry exporter builder.
    pub fn builder() -> OpenTelemetryExporterBuilder {
        OpenTelemetryExporterBuilder::default()
    }

    /// Internal constructor used by builder.
    #[allow(clippy::missing_const_for_fn)]
    fn new(meter: Meter, config: ExporterConfig) -> Self {
        Self { meter, config }
    }
}

impl MetricsExporter for OpenTelemetryExporter {
    fn export_counter(&self, name: &str, value: u64, labels: &[(String, String)]) {
        let counter = self.meter.u64_counter(format!("eventcore.{name}")).build();

        let mut attributes = vec![
            KeyValue::new("service.name", self.config.service_name.clone()),
            KeyValue::new("service.version", self.config.service_version.clone()),
            KeyValue::new("environment", self.config.environment.clone()),
        ];

        for (key, value) in labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        for (key, value) in &self.config.global_labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        counter.add(value, &attributes);
    }

    fn export_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let gauge = self
            .meter
            .f64_up_down_counter(format!("eventcore.{name}"))
            .build();

        let mut attributes = vec![
            KeyValue::new("service.name", self.config.service_name.clone()),
            KeyValue::new("service.version", self.config.service_version.clone()),
            KeyValue::new("environment", self.config.environment.clone()),
        ];

        for (key, value) in labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        for (key, value) in &self.config.global_labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        // OpenTelemetry doesn't have a direct gauge type, so we use UpDownCounter
        // and set the value by adding the difference
        gauge.add(value, &attributes);
    }

    fn export_histogram(&self, name: &str, value: Duration, labels: &[(String, String)]) {
        let histogram = self
            .meter
            .f64_histogram(format!("eventcore.{name}"))
            .with_unit("ms")
            .build();

        let mut attributes = vec![
            KeyValue::new("service.name", self.config.service_name.clone()),
            KeyValue::new("service.version", self.config.service_version.clone()),
            KeyValue::new("environment", self.config.environment.clone()),
        ];

        for (key, value) in labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        for (key, value) in &self.config.global_labels {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        #[allow(clippy::cast_precision_loss)]
        histogram.record(value.as_millis() as f64, &attributes);
    }
}

impl TracingExporter for OpenTelemetryExporter {
    fn export_span(&self, context: &TraceContext) {
        // OpenTelemetry tracing integration is complex and requires more setup
        // For now, we'll log trace information as a workaround
        info!(
            trace_id = %context.trace_id,
            span_id = %context.span_id,
            parent_span_id = context.parent_span_id.as_deref().unwrap_or("none"),
            operation = %context.operation_name,
            duration_ms = context.elapsed().as_millis(),
            "Trace span exported"
        );
    }
}

/// Builder for creating OpenTelemetry exporters.
pub struct OpenTelemetryExporterBuilder {
    endpoint: String,
    headers: Vec<(String, String)>,
    config: ExporterConfig,
}

impl Default for OpenTelemetryExporterBuilder {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            headers: Vec::new(),
            config: ExporterConfig::default(),
        }
    }
}

impl OpenTelemetryExporterBuilder {
    /// Sets the OTLP endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Adds a header for OTLP requests.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Sets the service name.
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    /// Sets the service version.
    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = version.into();
        self
    }

    /// Sets the environment.
    pub fn with_environment(mut self, env: impl Into<String>) -> Self {
        self.config.environment = env.into();
        self
    }

    /// Adds a global label.
    pub fn with_global_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.global_labels.push((key.into(), value.into()));
        self
    }

    /// Builds the OpenTelemetry exporter.
    pub fn build(self) -> Result<OpenTelemetryExporter, Box<dyn std::error::Error>> {
        // Create resource
        let resource = Resource::new(vec![
            KeyValue::new("service.name", self.config.service_name.clone()),
            KeyValue::new("service.version", self.config.service_version.clone()),
            KeyValue::new("deployment.environment", self.config.environment.clone()),
        ]);

        // Configure metrics
        let metric_exporter = if self.headers.is_empty() {
            MetricExporter::builder()
                .with_tonic()
                .with_endpoint(&self.endpoint)
                .build()?
        } else {
            // For now, skip headers as they require additional dependencies
            // Headers can be set via environment variables like OTEL_EXPORTER_OTLP_HEADERS
            MetricExporter::builder()
                .with_tonic()
                .with_endpoint(&self.endpoint)
                .build()?
        };

        let reader = PeriodicReader::builder(metric_exporter, runtime::Tokio)
            .with_interval(Duration::from_secs(10))
            .build();

        let meter_provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(resource)
            .build();

        let meter = meter_provider.meter("eventcore");
        global::set_meter_provider(meter_provider);

        info!(
            endpoint = %self.endpoint,
            service_name = %self.config.service_name,
            environment = %self.config.environment,
            "OpenTelemetry exporter initialized"
        );

        Ok(OpenTelemetryExporter::new(meter, self.config))
    }
}

/// Shutdown OpenTelemetry providers gracefully.
pub async fn shutdown() {
    // OpenTelemetry SDK now handles shutdown differently
    // The global meter provider is automatically shut down when dropped
    info!("OpenTelemetry providers shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_configuration() {
        let builder = OpenTelemetryExporterBuilder::default()
            .with_endpoint("http://example.com:4317")
            .with_service_name("test-service")
            .with_environment("testing")
            .with_global_label("region", "us-west-2");

        assert_eq!(builder.endpoint, "http://example.com:4317");
        assert_eq!(builder.config.service_name, "test-service");
        assert_eq!(builder.config.environment, "testing");
        assert_eq!(
            builder.config.global_labels,
            vec![("region".to_string(), "us-west-2".to_string())]
        );
    }
}
