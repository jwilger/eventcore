//! Prometheus exporter implementation for EventCore metrics.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use prometheus::{
    register_counter_vec, register_gauge_vec, CounterVec, Encoder, GaugeVec, HistogramVec,
    Registry, TextEncoder,
};
use tracing::info;

use super::{ExporterConfig, MetricsExporter};

/// Prometheus exporter for EventCore metrics.
pub struct PrometheusExporter {
    registry: Registry,
    counters: Arc<RwLock<HashMap<String, CounterVec>>>,
    gauges: Arc<RwLock<HashMap<String, GaugeVec>>>,
    histograms: Arc<RwLock<HashMap<String, HistogramVec>>>,
    config: ExporterConfig,
}

impl PrometheusExporter {
    /// Creates a new Prometheus exporter with the default registry.
    pub fn new() -> Self {
        Self::with_registry(Registry::new())
    }

    /// Creates a new Prometheus exporter with a custom registry.
    pub fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
            config: ExporterConfig::default(),
        }
    }

    /// Creates a new builder for configuring the Prometheus exporter.
    pub fn builder() -> PrometheusExporterBuilder {
        PrometheusExporterBuilder::default()
    }

    /// Gathers all metrics and returns them in Prometheus text format.
    pub fn gather(&self) -> Result<String, Box<dyn std::error::Error>> {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    /// Returns the Prometheus registry for advanced usage.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Helper to get or create a counter vector.
    fn get_or_create_counter(&self, name: &str, label_keys: &[&str]) -> CounterVec {
        let metric_name = format!("eventcore_{}", name);

        // Try to get existing counter
        if let Ok(counters) = self.counters.read() {
            if let Some(counter) = counters.get(&metric_name) {
                return counter.clone();
            }
        }

        // Create new counter
        let mut all_labels = vec!["service_name", "service_version", "environment"];
        all_labels.extend_from_slice(label_keys);

        let counter = register_counter_vec!(
            prometheus::opts!(
                metric_name.clone(),
                format!("EventCore counter metric: {}", name)
            ),
            &all_labels
        )
        .unwrap_or_else(|_| {
            // If registration fails (likely due to duplicate), create unregistered
            CounterVec::new(
                prometheus::opts!(
                    metric_name.clone(),
                    format!("EventCore counter metric: {}", name)
                ),
                &all_labels,
            )
            .unwrap()
        });

        // Try to register with our registry
        let _ = self.registry.register(Box::new(counter.clone()));

        // Store for future use
        if let Ok(mut counters) = self.counters.write() {
            counters.insert(metric_name, counter.clone());
        }

        counter
    }

    /// Helper to get or create a gauge vector.
    fn get_or_create_gauge(&self, name: &str, label_keys: &[&str]) -> GaugeVec {
        let metric_name = format!("eventcore_{}", name);

        // Try to get existing gauge
        if let Ok(gauges) = self.gauges.read() {
            if let Some(gauge) = gauges.get(&metric_name) {
                return gauge.clone();
            }
        }

        // Create new gauge
        let mut all_labels = vec!["service_name", "service_version", "environment"];
        all_labels.extend_from_slice(label_keys);

        let gauge = register_gauge_vec!(
            prometheus::opts!(
                metric_name.clone(),
                format!("EventCore gauge metric: {}", name)
            ),
            &all_labels
        )
        .unwrap_or_else(|_| {
            // If registration fails (likely due to duplicate), create unregistered
            GaugeVec::new(
                prometheus::opts!(
                    metric_name.clone(),
                    format!("EventCore gauge metric: {}", name)
                ),
                &all_labels,
            )
            .unwrap()
        });

        // Try to register with our registry
        let _ = self.registry.register(Box::new(gauge.clone()));

        // Store for future use
        if let Ok(mut gauges) = self.gauges.write() {
            gauges.insert(metric_name, gauge.clone());
        }

        gauge
    }

    /// Helper to get or create a histogram vector.
    fn get_or_create_histogram(&self, name: &str, label_keys: &[&str]) -> HistogramVec {
        let metric_name = format!("eventcore_{}", name);

        // Try to get existing histogram
        if let Ok(histograms) = self.histograms.read() {
            if let Some(histogram) = histograms.get(&metric_name) {
                return histogram.clone();
            }
        }

        // Create new histogram
        let mut all_labels = vec!["service_name", "service_version", "environment"];
        all_labels.extend_from_slice(label_keys);

        // Use exponential buckets for duration metrics
        let buckets = prometheus::exponential_buckets(0.001, 2.0, 15).unwrap();

        let histogram = HistogramVec::new(
            prometheus::histogram_opts!(
                metric_name.clone(),
                format!("EventCore histogram metric: {}", name),
                buckets
            ),
            &all_labels,
        )
        .unwrap();

        // Try to register with our registry
        let _ = self.registry.register(Box::new(histogram.clone()));

        // Store for future use
        if let Ok(mut histograms) = self.histograms.write() {
            histograms.insert(metric_name, histogram.clone());
        }

        histogram
    }

    /// Builds label values including global labels.
    fn build_label_values(&self, labels: &[(String, String)]) -> HashMap<String, String> {
        let mut label_map = HashMap::new();

        // Add standard labels
        label_map.insert("service_name".to_string(), self.config.service_name.clone());
        label_map.insert(
            "service_version".to_string(),
            self.config.service_version.clone(),
        );
        label_map.insert("environment".to_string(), self.config.environment.clone());

        // Add provided labels
        for (key, value) in labels {
            label_map.insert(key.clone(), value.clone());
        }

        // Add global labels
        for (key, value) in &self.config.global_labels {
            label_map.insert(key.clone(), value.clone());
        }

        label_map
    }
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsExporter for PrometheusExporter {
    fn export_counter(&self, name: &str, value: u64, labels: &[(String, String)]) {
        let label_keys: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
        let counter = self.get_or_create_counter(name, &label_keys);

        let mut values = vec![
            self.config.service_name.as_str(),
            self.config.service_version.as_str(),
            self.config.environment.as_str(),
        ];

        for (_, v) in labels {
            values.push(v.as_str());
        }

        counter.with_label_values(&values).inc_by(value as f64);
    }

    fn export_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let label_keys: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
        let gauge = self.get_or_create_gauge(name, &label_keys);

        let mut values = vec![
            self.config.service_name.as_str(),
            self.config.service_version.as_str(),
            self.config.environment.as_str(),
        ];

        for (_, v) in labels {
            values.push(v.as_str());
        }

        gauge.with_label_values(&values).set(value);
    }

    fn export_histogram(&self, name: &str, value: Duration, labels: &[(String, String)]) {
        let label_keys: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
        let histogram = self.get_or_create_histogram(name, &label_keys);

        let mut values = vec![
            self.config.service_name.as_str(),
            self.config.service_version.as_str(),
            self.config.environment.as_str(),
        ];

        for (_, v) in labels {
            values.push(v.as_str());
        }

        histogram
            .with_label_values(&values)
            .observe(value.as_secs_f64());
    }
}

/// Builder for creating Prometheus exporters.
pub struct PrometheusExporterBuilder {
    registry: Option<Registry>,
    config: ExporterConfig,
}

impl Default for PrometheusExporterBuilder {
    fn default() -> Self {
        Self {
            registry: None,
            config: ExporterConfig::default(),
        }
    }
}

impl PrometheusExporterBuilder {
    /// Sets a custom Prometheus registry.
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = Some(registry);
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

    /// Builds the Prometheus exporter.
    pub fn build(self) -> PrometheusExporter {
        let registry = self.registry.unwrap_or_else(Registry::new);

        info!(
            service_name = %self.config.service_name,
            environment = %self.config.environment,
            "Prometheus exporter initialized"
        );

        PrometheusExporter {
            registry,
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
            config: self.config,
        }
    }
}

/// HTTP handler for Prometheus metrics endpoint.
///
/// This can be used with any HTTP framework to expose metrics.
pub struct PrometheusMetricsHandler {
    exporter: Arc<PrometheusExporter>,
}

impl PrometheusMetricsHandler {
    /// Creates a new metrics handler.
    pub fn new(exporter: Arc<PrometheusExporter>) -> Self {
        Self { exporter }
    }

    /// Handles a metrics request and returns the response body.
    pub async fn handle(&self) -> Result<String, Box<dyn std::error::Error>> {
        self.exporter.gather()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_updates_metrics() {
        let exporter = PrometheusExporter::new();

        // Test counter
        exporter.export_counter(
            "test_counter",
            5,
            &[("operation".to_string(), "read".to_string())],
        );

        // Test gauge
        exporter.export_gauge(
            "test_gauge",
            42.0,
            &[("resource".to_string(), "cpu".to_string())],
        );

        // Test histogram
        exporter.export_histogram(
            "test_histogram",
            Duration::from_millis(100),
            &[("endpoint".to_string(), "/api/test".to_string())],
        );

        // Verify metrics can be gathered
        let metrics_text = exporter.gather().unwrap();
        assert!(metrics_text.contains("eventcore_test_counter"));
        assert!(metrics_text.contains("eventcore_test_gauge"));
        assert!(metrics_text.contains("eventcore_test_histogram"));
    }

    #[test]
    fn builder_configures_exporter() {
        let exporter = PrometheusExporter::builder()
            .with_service_name("test-service")
            .with_environment("testing")
            .with_global_label("datacenter", "us-east-1")
            .build();

        assert_eq!(exporter.config.service_name, "test-service");
        assert_eq!(exporter.config.environment, "testing");
        assert_eq!(
            exporter.config.global_labels,
            vec![("datacenter".to_string(), "us-east-1".to_string())]
        );
    }
}
