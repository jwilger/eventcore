# EventCore Monitoring and Observability

EventCore provides comprehensive monitoring and observability capabilities through integration with popular monitoring systems like OpenTelemetry and Prometheus. This guide covers how to enable and configure these integrations.

## Overview

EventCore includes:
- **Built-in metrics**: Comprehensive metrics for commands, event store operations, projections, and system health
- **Distributed tracing**: Trace context propagation and span management
- **External integrations**: Export metrics and traces to OpenTelemetry and Prometheus
- **Health checks**: System health monitoring endpoints

## Enabling Observability Features

Add the desired features to your `Cargo.toml`:

```toml
[dependencies]
eventcore = { version = "0.1", features = ["opentelemetry"] }
# or
eventcore = { version = "0.1", features = ["prometheus"] }
# or both
eventcore = { version = "0.1", features = ["all-exporters"] }
```

## OpenTelemetry Integration

### Configuration

```rust
use eventcore::monitoring::exporters::opentelemetry::OpenTelemetryExporter;
use eventcore::monitoring::exporters::bridge::MonitoringBuilder;
use eventcore::monitoring::metrics::MetricsRegistry;
use std::sync::Arc;

// Create metrics registry
let metrics_registry = Arc::new(MetricsRegistry::new());

// Configure OpenTelemetry exporter
let otel_exporter = OpenTelemetryExporter::builder()
    .with_endpoint("http://localhost:4317")  // OTLP endpoint
    .with_service_name("my-service")
    .with_service_version("1.0.0")
    .with_environment("production")
    .with_global_label("region", "us-west-2")
    .build()?;

// Set up monitoring with automatic export
let monitoring = MonitoringBuilder::new(metrics_registry)
    .with_metrics_exporter(Arc::new(otel_exporter))
    .with_export_interval(Duration::from_secs(10))
    .build()
    .await;
```

### Docker Compose Setup

```yaml
version: '3.8'
services:
  otel-collector:
    image: otel/opentelemetry-collector-contrib:latest
    command: ["--config=/etc/otel-collector-config.yaml"]
    volumes:
      - ./otel-collector-config.yaml:/etc/otel-collector-config.yaml
    ports:
      - "4317:4317"  # OTLP gRPC
      - "4318:4318"  # OTLP HTTP
      - "8888:8888"  # Prometheus metrics

  # Your application
  app:
    environment:
      - OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
```

### OpenTelemetry Collector Configuration

```yaml
# otel-collector-config.yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318

processors:
  batch:

exporters:
  # Export to Prometheus
  prometheus:
    endpoint: "0.0.0.0:8888"
  
  # Export to Jaeger
  jaeger:
    endpoint: jaeger:14250
    tls:
      insecure: true

service:
  pipelines:
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus]
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [jaeger]
```

## Prometheus Integration

### Configuration

```rust
use eventcore::monitoring::exporters::prometheus::PrometheusExporter;
use axum::{routing::get, Router};
use std::net::SocketAddr;

// Create Prometheus exporter
let prometheus_exporter = Arc::new(
    PrometheusExporter::builder()
        .with_service_name("my-service")
        .with_environment("production")
        .with_global_label("instance", "server-01")
        .build()
);

// Add to monitoring
let monitoring = MonitoringBuilder::new(metrics_registry)
    .with_metrics_exporter(prometheus_exporter.clone())
    .build()
    .await;

// Create HTTP endpoint for Prometheus scraping
let app = Router::new().route("/metrics", get(move || {
    let exporter = prometheus_exporter.clone();
    async move {
        match exporter.gather() {
            Ok(metrics) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                metrics,
            ),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                format!("Error gathering metrics: {}", e),
            ),
        }
    }
}));

// Start metrics server
let addr = SocketAddr::from(([0, 0, 0, 0], 9090));
axum::Server::bind(&addr)
    .serve(app.into_make_service())
    .await?;
```

### Prometheus Configuration

```yaml
# prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'eventcore-app'
    static_configs:
      - targets: ['localhost:9090']
```

## Available Metrics

### Command Metrics
- `eventcore_commands_executed`: Total commands executed
- `eventcore_commands_succeeded`: Successful command executions
- `eventcore_commands_failed`: Failed command executions
- `eventcore_commands_by_type`: Commands by type (labeled)
- `eventcore_command_duration`: Command execution duration (histogram)
- `eventcore_commands_concurrent`: Currently executing commands (gauge)

### Event Store Metrics
- `eventcore_event_store_reads`: Total read operations
- `eventcore_event_store_writes`: Total write operations
- `eventcore_events_written`: Total events written
- `eventcore_events_read`: Total events read
- `eventcore_event_store_read_duration`: Read operation duration
- `eventcore_event_store_write_duration`: Write operation duration
- `eventcore_streams_count`: Number of active streams

### Projection Metrics
- `eventcore_projections_events_processed`: Events processed by projections
- `eventcore_projections_events_skipped`: Events skipped by projections
- `eventcore_projections_errors`: Projection processing errors
- `eventcore_projection_lag_ms`: Projection lag in milliseconds (per projection)
- `eventcore_projections_active`: Number of active projections

### System Metrics
- `eventcore_system_memory_usage_bytes`: Memory usage
- `eventcore_system_cpu_usage_percent`: CPU usage percentage
- `eventcore_connection_pool_size`: Total connections in pool
- `eventcore_connection_pool_available`: Available connections
- `eventcore_circuit_breaker_state`: Circuit breaker states (0=closed, 1=open, 0.5=half-open)

### Error Metrics
- `eventcore_errors_critical`: Critical errors
- `eventcore_errors_transient`: Transient/retryable errors
- `eventcore_errors_permanent`: Permanent errors
- `eventcore_errors_by_type`: Errors by type (labeled)
- `eventcore_errors_rate_percent`: Error rate percentage

## Manual Metrics Recording

If you need to manually record metrics:

```rust
use eventcore::monitoring::metrics::MetricsRegistry;
use std::time::Instant;

let metrics = Arc::new(MetricsRegistry::new());

// Record command execution
let start = Instant::now();
metrics.command_metrics.record_command_start("MyCommand");

match execute_command().await {
    Ok(_) => {
        let duration = start.elapsed();
        metrics.command_metrics.record_command_success(duration);
    }
    Err(e) => {
        metrics.command_metrics.record_command_failure(&e);
    }
}

// Record event store operations
metrics.event_store_metrics.record_write_start(&[stream_id]);
// ... perform write ...
metrics.event_store_metrics.record_write_complete(event_count, duration);
```

## Grafana Dashboard

Example Grafana dashboard JSON for EventCore metrics:

```json
{
  "dashboard": {
    "title": "EventCore Metrics",
    "panels": [
      {
        "title": "Command Rate",
        "targets": [
          {
            "expr": "rate(eventcore_commands_executed[5m])"
          }
        ]
      },
      {
        "title": "Command Success Rate",
        "targets": [
          {
            "expr": "rate(eventcore_commands_succeeded[5m]) / rate(eventcore_commands_executed[5m])"
          }
        ]
      },
      {
        "title": "Command Duration P95",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, eventcore_command_duration_bucket)"
          }
        ]
      },
      {
        "title": "Event Write Throughput",
        "targets": [
          {
            "expr": "rate(eventcore_events_written[5m])"
          }
        ]
      }
    ]
  }
}
```

## Best Practices

1. **Export Interval**: Set appropriate export intervals based on your metric volume
   - High-traffic services: 30-60 seconds
   - Low-traffic services: 10-30 seconds

2. **Label Cardinality**: Avoid high-cardinality labels
   - Good: command_type, error_type, projection_name
   - Bad: user_id, request_id, event_id

3. **Resource Management**: Monitor the overhead of metrics collection
   - Use sampling for high-frequency operations
   - Configure appropriate retention policies

4. **Alerting**: Set up alerts for critical metrics
   - Command error rate > 5%
   - Projection lag > 60 seconds
   - Circuit breaker open state
   - Connection pool exhaustion

## Troubleshooting

### Metrics Not Appearing

1. Verify feature flags are enabled:
   ```bash
   cargo build --features opentelemetry
   ```

2. Check exporter configuration:
   - Correct endpoint URL
   - Network connectivity
   - Authentication if required

3. Enable debug logging:
   ```rust
   tracing_subscriber::fmt()
       .with_max_level(tracing::Level::DEBUG)
       .init();
   ```

### High Memory Usage

- Reduce metric cardinality
- Decrease export interval
- Enable metric sampling

### Performance Impact

- Use async metrics recording
- Batch metric updates
- Consider using a separate thread for export

## Example Application

See the complete example in `examples/observability_example.rs`:

```bash
# Run with Prometheus support
cargo run --example observability_example --features prometheus

# Run with OpenTelemetry support
cargo run --example observability_example --features opentelemetry

# Run with both
cargo run --example observability_example --features all-exporters
```