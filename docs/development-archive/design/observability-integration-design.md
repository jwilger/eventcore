# EventCore Observability Integration Design

## Overview

EventCore already has comprehensive internal metrics and tracing infrastructure. This design document outlines the approach to integrate EventCore with popular observability tools like OpenTelemetry and Prometheus, allowing users to export metrics and traces to their preferred monitoring backends.

## Current State

EventCore provides:
- **Metrics**: Counter, Gauge, Timer implementations with comprehensive domain-specific metrics
- **Tracing**: TraceContext and span management with correlation IDs
- **Health checks**: System health monitoring capabilities
- **Structured logging**: With trace correlation

## Integration Approach

### 1. Bridge Pattern

Create a bridge layer that translates EventCore's internal metrics to external formats:

```rust
// eventcore/src/monitoring/exporters/mod.rs
pub trait MetricsExporter: Send + Sync {
    fn export_counter(&self, name: &str, value: u64, labels: &[(String, String)]);
    fn export_gauge(&self, name: &str, value: f64, labels: &[(String, String)]);
    fn export_histogram(&self, name: &str, value: Duration, labels: &[(String, String)]);
}

pub trait TracingExporter: Send + Sync {
    fn export_span(&self, context: &TraceContext);
}
```

### 2. OpenTelemetry Integration

#### Dependencies
```toml
[dependencies]
opentelemetry = { version = "0.27", features = ["metrics", "trace"], optional = true }
opentelemetry-otlp = { version = "0.27", optional = true }
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"], optional = true }
tracing-opentelemetry = { version = "0.28", optional = true }
```

#### Implementation Strategy
1. Create `OpenTelemetryExporter` that implements both `MetricsExporter` and `TracingExporter`
2. Map EventCore metrics to OpenTelemetry instruments:
   - Counter → Counter
   - Gauge → UpDownCounter or ObservableGauge
   - Timer → Histogram
3. Map EventCore TraceContext to OpenTelemetry spans
4. Provide configuration for OTLP endpoint, headers, etc.

### 3. Prometheus Integration

#### Dependencies
```toml
[dependencies]
prometheus = { version = "0.13", optional = true }
```

#### Implementation Strategy
1. Create `PrometheusExporter` that implements `MetricsExporter`
2. Map EventCore metrics to Prometheus metrics:
   - Counter → prometheus::Counter
   - Gauge → prometheus::Gauge
   - Timer → prometheus::Histogram
3. Provide HTTP endpoint for Prometheus scraping
4. Add metric naming conventions (e.g., `eventcore_commands_total`)

### 4. Feature Flags

Make integrations optional via feature flags:
```toml
[features]
default = []
opentelemetry = ["dep:opentelemetry", "dep:opentelemetry-otlp", "dep:opentelemetry_sdk", "dep:tracing-opentelemetry"]
prometheus = ["dep:prometheus"]
all-exporters = ["opentelemetry", "prometheus"]
```

### 5. Configuration API

```rust
// Example usage
let mut event_store = PostgresEventStore::new(config).await?;

// Enable OpenTelemetry
let otel_exporter = OpenTelemetryExporter::builder()
    .with_endpoint("http://localhost:4317")
    .with_service_name("my-service")
    .build()?;
event_store.register_exporter(Box::new(otel_exporter));

// Enable Prometheus
let prometheus_exporter = PrometheusExporter::new();
let metrics_endpoint = prometheus_exporter.create_http_endpoint(9090);
event_store.register_exporter(Box::new(prometheus_exporter));
```

## Metric Naming Conventions

### OpenTelemetry
- `eventcore.commands.total` (counter)
- `eventcore.commands.duration` (histogram)
- `eventcore.events.written` (counter)
- `eventcore.streams.active` (gauge)

### Prometheus
- `eventcore_commands_total` (counter)
- `eventcore_command_duration_seconds` (histogram)
- `eventcore_events_written_total` (counter)
- `eventcore_streams_active` (gauge)

## Implementation Plan

1. Create exporter trait definitions
2. Implement OpenTelemetry exporter
3. Implement Prometheus exporter
4. Add configuration API to EventStore trait
5. Create example application
6. Write documentation

## Testing Strategy

1. Unit tests for metric translation
2. Integration tests with mock collectors
3. Example application with docker-compose setup including:
   - EventCore application
   - OpenTelemetry Collector
   - Prometheus
   - Grafana dashboards