# Chapter 6.2: Monitoring and Metrics

Effective monitoring is crucial for operating EventCore applications in production. This chapter covers comprehensive observability strategies including metrics, logging, tracing, and alerting.

## Metrics Collection

### Prometheus Integration

EventCore provides built-in Prometheus metrics:

```rust
use prometheus::{
    Counter, Histogram, Gauge, IntGauge,
    register_counter, register_histogram, register_gauge, register_int_gauge,
    Encoder, TextEncoder
};
use axum::{response::Response, http::StatusCode};

lazy_static! {
    // Command execution metrics
    static ref COMMANDS_TOTAL: Counter = register_counter!(
        "eventcore_commands_total",
        "Total number of commands executed"
    ).unwrap();

    static ref COMMAND_DURATION: Histogram = register_histogram!(
        "eventcore_command_duration_seconds",
        "Command execution duration in seconds"
    ).unwrap();

    static ref COMMAND_ERRORS: Counter = register_counter!(
        "eventcore_command_errors_total",
        "Total number of command execution errors"
    ).unwrap();

    // Event store metrics
    static ref EVENTS_WRITTEN: Counter = register_counter!(
        "eventcore_events_written_total",
        "Total number of events written to the store"
    ).unwrap();

    static ref EVENT_STORE_LATENCY: Histogram = register_histogram!(
        "eventcore_event_store_latency_seconds",
        "Event store operation latency in seconds"
    ).unwrap();

    // Stream metrics
    static ref ACTIVE_STREAMS: IntGauge = register_int_gauge!(
        "eventcore_active_streams",
        "Number of active event streams"
    ).unwrap();

    static ref STREAM_VERSIONS: Gauge = register_gauge!(
        "eventcore_stream_versions",
        "Current version of event streams"
    ).unwrap();

    // Projection metrics
    static ref PROJECTION_EVENTS_PROCESSED: Counter = register_counter!(
        "eventcore_projection_events_processed_total",
        "Total events processed by projections"
    ).unwrap();

    static ref PROJECTION_LAG: Gauge = register_gauge!(
        "eventcore_projection_lag_seconds",
        "Projection lag behind latest events in seconds"
    ).unwrap();

    // System metrics
    static ref MEMORY_USAGE: Gauge = register_gauge!(
        "eventcore_memory_usage_bytes",
        "Memory usage in bytes"
    ).unwrap();

    static ref CONNECTION_POOL_SIZE: IntGauge = register_int_gauge!(
        "eventcore_connection_pool_size",
        "Database connection pool size"
    ).unwrap();
}

#[derive(Clone)]
pub struct MetricsService {
    start_time: std::time::Instant,
}

impl MetricsService {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
        }
    }

    pub fn record_command_executed(&self, command_type: &str, duration: std::time::Duration, success: bool) {
        COMMANDS_TOTAL.with_label_values(&[command_type]).inc();
        COMMAND_DURATION.with_label_values(&[command_type]).observe(duration.as_secs_f64());

        if !success {
            COMMAND_ERRORS.with_label_values(&[command_type]).inc();
        }
    }

    pub fn record_events_written(&self, stream_id: &str, count: usize) {
        EVENTS_WRITTEN.with_label_values(&[stream_id]).inc_by(count as f64);
    }

    pub fn record_event_store_operation(&self, operation: &str, duration: std::time::Duration) {
        EVENT_STORE_LATENCY.with_label_values(&[operation]).observe(duration.as_secs_f64());
    }

    pub fn update_active_streams(&self, count: i64) {
        ACTIVE_STREAMS.set(count);
    }

    pub fn update_stream_version(&self, stream_id: &str, version: f64) {
        STREAM_VERSIONS.with_label_values(&[stream_id]).set(version);
    }

    pub fn record_projection_event(&self, projection_name: &str, lag_seconds: f64) {
        PROJECTION_EVENTS_PROCESSED.with_label_values(&[projection_name]).inc();
        PROJECTION_LAG.with_label_values(&[projection_name]).set(lag_seconds);
    }

    pub fn update_memory_usage(&self, bytes: f64) {
        MEMORY_USAGE.set(bytes);
    }

    pub fn update_connection_pool_size(&self, size: i64) {
        CONNECTION_POOL_SIZE.set(size);
    }

    pub async fn export_metrics(&self) -> Result<Response<String>, StatusCode> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();

        match encoder.encode_to_string(&metric_families) {
            Ok(output) => {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", encoder.format_type())
                    .body(output)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                Ok(response)
            }
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

// Metrics endpoint handler
pub async fn metrics_handler(
    State(metrics_service): State<MetricsService>
) -> Result<Response<String>, StatusCode> {
    metrics_service.export_metrics().await
}
```

### Custom Metrics

Define application-specific metrics:

```rust
use prometheus::{register_counter_vec, register_histogram_vec, CounterVec, HistogramVec};

lazy_static! {
    // Business metrics
    static ref USER_REGISTRATIONS: Counter = register_counter!(
        "eventcore_user_registrations_total",
        "Total number of user registrations"
    ).unwrap();

    static ref ORDER_VALUE: Histogram = register_histogram!(
        "eventcore_order_value_dollars",
        "Order value in dollars",
        vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0]
    ).unwrap();

    static ref API_REQUESTS: CounterVec = register_counter_vec!(
        "eventcore_api_requests_total",
        "Total API requests",
        &["method", "endpoint", "status"]
    ).unwrap();

    static ref REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "eventcore_request_duration_seconds",
        "Request duration in seconds",
        &["method", "endpoint"]
    ).unwrap();
}

pub struct BusinessMetrics;

impl BusinessMetrics {
    pub fn record_user_registration() {
        USER_REGISTRATIONS.inc();
    }

    pub fn record_order_placed(value_dollars: f64) {
        ORDER_VALUE.observe(value_dollars);
    }

    pub fn record_api_request(method: &str, endpoint: &str, status: u16, duration: std::time::Duration) {
        API_REQUESTS
            .with_label_values(&[method, endpoint, &status.to_string()])
            .inc();

        REQUEST_DURATION
            .with_label_values(&[method, endpoint])
            .observe(duration.as_secs_f64());
    }
}
```

### Automatic Instrumentation

Instrument EventCore operations automatically:

```rust
use std::time::Instant;
use async_trait::async_trait;

pub struct InstrumentedCommandExecutor {
    inner: CommandExecutor,
    metrics: MetricsService,
}

impl InstrumentedCommandExecutor {
    pub fn new(inner: CommandExecutor, metrics: MetricsService) -> Self {
        Self { inner, metrics }
    }
}

#[async_trait]
impl CommandExecutor for InstrumentedCommandExecutor {
    async fn execute<C: Command>(&self, command: &C) -> CommandResult<ExecutionResult> {
        let start = Instant::now();
        let command_type = std::any::type_name::<C>();

        let result = self.inner.execute(command).await;
        let duration = start.elapsed();
        let success = result.is_ok();

        self.metrics.record_command_executed(command_type, duration, success);

        if let Ok(ref execution_result) = result {
            self.metrics.record_events_written(
                &execution_result.affected_streams[0].to_string(),
                execution_result.events_written.len()
            );
        }

        result
    }
}

// Instrumented event store
pub struct InstrumentedEventStore {
    inner: Arc<dyn EventStore>,
    metrics: MetricsService,
}

#[async_trait]
impl EventStore for InstrumentedEventStore {
    async fn write_events(&self, events: Vec<EventToWrite>) -> EventStoreResult<WriteResult> {
        let start = Instant::now();
        let result = self.inner.write_events(events).await;
        let duration = start.elapsed();

        self.metrics.record_event_store_operation("write", duration);
        result
    }

    async fn read_stream(&self, stream_id: &StreamId, options: ReadOptions) -> EventStoreResult<StreamEvents> {
        let start = Instant::now();
        let result = self.inner.read_stream(stream_id, options).await;
        let duration = start.elapsed();

        self.metrics.record_event_store_operation("read", duration);
        result
    }
}
```

## Structured Logging

### Logging Configuration

```rust
use tracing::{info, warn, error, debug, trace, instrument};
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    fmt,
    EnvFilter,
};
use serde_json::json;

pub fn init_logging(log_level: &str, log_format: &str) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    let fmt_layer = match log_format {
        "json" => {
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .boxed()
        }
        _ => {
            fmt::layer()
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .boxed()
        }
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    Ok(())
}

// Structured logging for command execution
#[instrument(skip(command), fields(command_type = %std::any::type_name::<C>()))]
pub async fn execute_command_with_logging<C: Command>(
    command: &C,
    executor: &CommandExecutor,
) -> CommandResult<ExecutionResult> {
    debug!("Starting command execution");

    let result = executor.execute(command).await;

    match &result {
        Ok(execution_result) => {
            info!(
                events_written = execution_result.events_written.len(),
                affected_streams = execution_result.affected_streams.len(),
                "Command executed successfully"
            );
        }
        Err(error) => {
            error!(
                error = %error,
                "Command execution failed"
            );
        }
    }

    result
}

// Event store logging
#[instrument(skip(events), fields(event_count = events.len()))]
pub async fn write_events_with_logging(
    events: Vec<EventToWrite>,
    event_store: &dyn EventStore,
) -> EventStoreResult<WriteResult> {
    debug!("Writing events to store");

    let stream_ids: Vec<_> = events.iter()
        .map(|e| e.stream_id.to_string())
        .collect();

    let result = event_store.write_events(events).await;

    match &result {
        Ok(write_result) => {
            info!(
                events_written = write_result.events_written,
                streams = ?stream_ids,
                "Events written successfully"
            );
        }
        Err(error) => {
            error!(
                error = %error,
                streams = ?stream_ids,
                "Failed to write events"
            );
        }
    }

    result
}
```

### Log Aggregation

Configure log shipping to centralized systems:

```yaml
# Fluentd configuration for Kubernetes
apiVersion: v1
kind: ConfigMap
metadata:
  name: fluentd-config
  namespace: eventcore
data:
  fluent.conf: |
    <source>
      @type tail
      path /var/log/containers/eventcore-*.log
      pos_file /var/log/fluentd-eventcore.log.pos
      tag eventcore.*
      format json
      time_key time
      time_format %Y-%m-%dT%H:%M:%S.%NZ
    </source>

    <filter eventcore.**>
      @type parser
      key_name log
      format json
      reserve_data true
    </filter>

    <match eventcore.**>
      @type elasticsearch
      host elasticsearch.logging.svc.cluster.local
      port 9200
      index_name eventcore-logs
      type_name _doc
      include_timestamp true
      logstash_format true
      logstash_prefix eventcore

      <buffer>
        @type file
        path /var/log/fluentd-buffers/eventcore
        flush_mode interval
        retry_type exponential_backoff
        flush_thread_count 2
        flush_interval 5s
        retry_forever
        retry_max_interval 30
        chunk_limit_size 2M
        queue_limit_length 8
        overflow_action block
      </buffer>
    </match>
```

## Distributed Tracing

### OpenTelemetry Integration

```rust
use opentelemetry::{
    global,
    trace::{TraceError, Tracer, TracerProvider},
    KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{self, Sampler},
    Resource,
};
use tracing_opentelemetry::OpenTelemetryLayer;

pub fn init_tracing(service_name: &str, otlp_endpoint: &str) -> Result<(), TraceError> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint)
        )
        .with_trace_config(
            trace::config()
                .with_sampler(Sampler::TraceIdRatioBased(1.0))
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", service_name.to_string()),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    KeyValue::new("deployment.environment",
                        std::env::var("ENVIRONMENT").unwrap_or_else(|_| "unknown".to_string())
                    ),
                ]))
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry_layer)
        .init();

    Ok(())
}

// Traced command execution
#[tracing::instrument(skip(command, executor), fields(command_id = %uuid::Uuid::new_v4()))]
pub async fn execute_command_traced<C: Command>(
    command: &C,
    executor: &CommandExecutor,
) -> CommandResult<ExecutionResult> {
    let span = tracing::Span::current();
    span.record("command.type", std::any::type_name::<C>());

    let result = executor.execute(command).await;

    match &result {
        Ok(execution_result) => {
            span.record("command.success", true);
            span.record("events.count", execution_result.events_written.len());
            span.record("streams.count", execution_result.affected_streams.len());
        }
        Err(error) => {
            span.record("command.success", false);
            span.record("error.message", format!("{}", error));
            span.record("error.type", std::any::type_name_of_val(error));
        }
    }

    result
}

// Cross-service trace propagation
use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};

pub async fn trace_propagation_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Extract trace context from headers
    let headers = request.headers();
    let parent_context = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderMapCarrier::new(headers))
    });

    // Create new span with parent context
    let span = tracing::info_span!(
        "http_request",
        method = %request.method(),
        uri = %request.uri(),
        version = ?request.version(),
    );

    // Set parent context
    span.set_parent(parent_context);

    // Execute request within span
    let response = span.in_scope(|| next.run(request)).await;

    response
}

struct HeaderMapCarrier<'a> {
    headers: &'a HeaderMap,
}

impl<'a> HeaderMapCarrier<'a> {
    fn new(headers: &'a HeaderMap) -> Self {
        Self { headers }
    }
}

impl<'a> opentelemetry::propagation::Extractor for HeaderMapCarrier<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key)?.to_str().ok()
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(|k| k.as_str()).collect()
    }
}
```

## Alerting

### Prometheus Alerting Rules

```yaml
# prometheus-alerts.yaml
groups:
  - name: eventcore.rules
    rules:
      # High error rate
      - alert: HighCommandErrorRate
        expr: |
          (
            rate(eventcore_command_errors_total[5m]) /
            rate(eventcore_commands_total[5m])
          ) > 0.05
        for: 2m
        labels:
          severity: warning
          service: eventcore
        annotations:
          summary: "High command error rate detected"
          description: "Command error rate is {{ $value | humanizePercentage }} over the last 5 minutes"

      # High latency
      - alert: HighCommandLatency
        expr: |
          histogram_quantile(0.95, rate(eventcore_command_duration_seconds_bucket[5m])) > 1.0
        for: 3m
        labels:
          severity: warning
          service: eventcore
        annotations:
          summary: "High command latency detected"
          description: "95th percentile command latency is {{ $value }}s"

      # Event store issues
      - alert: EventStoreDown
        expr: up{job="eventcore"} == 0
        for: 1m
        labels:
          severity: critical
          service: eventcore
        annotations:
          summary: "EventCore service is down"
          description: "EventCore service has been down for more than 1 minute"

      # Projection lag
      - alert: ProjectionLag
        expr: eventcore_projection_lag_seconds > 300
        for: 5m
        labels:
          severity: warning
          service: eventcore
        annotations:
          summary: "Projection lag is high"
          description: "Projection {{ $labels.projection_name }} is {{ $value }}s behind"

      # Memory usage
      - alert: HighMemoryUsage
        expr: |
          (eventcore_memory_usage_bytes / (1024 * 1024 * 1024)) > 1.0
        for: 5m
        labels:
          severity: warning
          service: eventcore
        annotations:
          summary: "High memory usage"
          description: "Memory usage is {{ $value | humanize }}GB"

      # Database connection pool
      - alert: DatabaseConnectionPoolExhausted
        expr: eventcore_connection_pool_size / eventcore_connection_pool_max_size > 0.9
        for: 2m
        labels:
          severity: critical
          service: eventcore
        annotations:
          summary: "Database connection pool nearly exhausted"
          description: "Connection pool utilization is {{ $value | humanizePercentage }}"
```

### Alert Manager Configuration

```yaml
# alertmanager.yaml
global:
  smtp_smarthost: "smtp.example.com:587"
  smtp_from: "alerts@eventcore.com"

route:
  group_by: ["alertname", "service"]
  group_wait: 10s
  group_interval: 10s
  repeat_interval: 1h
  receiver: "web.hook"
  routes:
    - match:
        severity: critical
      receiver: "critical-alerts"
    - match:
        severity: warning
      receiver: "warning-alerts"

receivers:
  - name: "web.hook"
    webhook_configs:
      - url: "http://slack-webhook/webhook"

  - name: "critical-alerts"
    email_configs:
      - to: "oncall@eventcore.com"
        subject: "CRITICAL: {{ range .Alerts }}{{ .Annotations.summary }}{{ end }}"
        body: |
          {{ range .Alerts }}
          Alert: {{ .Annotations.summary }}
          Description: {{ .Annotations.description }}
          Labels: {{ range .Labels.SortedPairs }}{{ .Name }}={{ .Value }} {{ end }}
          {{ end }}
    slack_configs:
      - api_url: "https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK"
        channel: "#critical-alerts"
        title: "Critical Alert: {{ range .Alerts }}{{ .Annotations.summary }}{{ end }}"

  - name: "warning-alerts"
    slack_configs:
      - api_url: "https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK"
        channel: "#warnings"
        title: "Warning: {{ range .Alerts }}{{ .Annotations.summary }}{{ end }}"
```

## Grafana Dashboards

### EventCore Operations Dashboard

```json
{
  "dashboard": {
    "title": "EventCore Operations",
    "panels": [
      {
        "title": "Command Execution Rate",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(eventcore_commands_total[5m])",
            "legendFormat": "Commands/sec"
          }
        ]
      },
      {
        "title": "Command Latency",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.50, rate(eventcore_command_duration_seconds_bucket[5m]))",
            "legendFormat": "p50"
          },
          {
            "expr": "histogram_quantile(0.95, rate(eventcore_command_duration_seconds_bucket[5m]))",
            "legendFormat": "p95"
          },
          {
            "expr": "histogram_quantile(0.99, rate(eventcore_command_duration_seconds_bucket[5m]))",
            "legendFormat": "p99"
          }
        ]
      },
      {
        "title": "Error Rate",
        "type": "singlestat",
        "targets": [
          {
            "expr": "rate(eventcore_command_errors_total[5m]) / rate(eventcore_commands_total[5m])",
            "legendFormat": "Error Rate"
          }
        ],
        "thresholds": [
          {
            "value": 0.01,
            "colorMode": "critical"
          }
        ]
      },
      {
        "title": "Active Streams",
        "type": "singlestat",
        "targets": [
          {
            "expr": "eventcore_active_streams",
            "legendFormat": "Streams"
          }
        ]
      },
      {
        "title": "Projection Lag",
        "type": "graph",
        "targets": [
          {
            "expr": "eventcore_projection_lag_seconds",
            "legendFormat": "{{ projection_name }}"
          }
        ]
      },
      {
        "title": "Memory Usage",
        "type": "graph",
        "targets": [
          {
            "expr": "eventcore_memory_usage_bytes / (1024 * 1024 * 1024)",
            "legendFormat": "Memory (GB)"
          }
        ]
      }
    ]
  }
}
```

## Performance Monitoring

### Real-Time Performance Metrics

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub commands_per_second: f64,
    pub events_per_second: f64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub error_rate: f64,
    pub active_streams: i64,
    pub memory_usage_mb: f64,
}

pub struct PerformanceMonitor {
    snapshots: Arc<RwLock<Vec<PerformanceSnapshot>>>,
    max_snapshots: usize,
}

impl PerformanceMonitor {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(Vec::new())),
            max_snapshots,
        }
    }

    pub async fn capture_snapshot(&self) -> PerformanceSnapshot {
        let snapshot = PerformanceSnapshot {
            timestamp: chrono::Utc::now(),
            commands_per_second: self.calculate_command_rate().await,
            events_per_second: self.calculate_event_rate().await,
            avg_latency_ms: self.calculate_avg_latency().await,
            p95_latency_ms: self.calculate_p95_latency().await,
            p99_latency_ms: self.calculate_p99_latency().await,
            error_rate: self.calculate_error_rate().await,
            active_streams: self.get_active_stream_count().await,
            memory_usage_mb: self.get_memory_usage_mb().await,
        };

        let mut snapshots = self.snapshots.write().await;
        snapshots.push(snapshot.clone());

        // Keep only the most recent snapshots
        if snapshots.len() > self.max_snapshots {
            snapshots.remove(0);
        }

        snapshot
    }

    pub async fn get_trend_analysis(&self, minutes: u64) -> TrendAnalysis {
        let snapshots = self.snapshots.read().await;
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(minutes as i64);

        let recent_snapshots: Vec<_> = snapshots
            .iter()
            .filter(|s| s.timestamp > cutoff)
            .collect();

        if recent_snapshots.is_empty() {
            return TrendAnalysis::default();
        }

        TrendAnalysis {
            throughput_trend: self.calculate_trend(&recent_snapshots, |s| s.commands_per_second),
            latency_trend: self.calculate_trend(&recent_snapshots, |s| s.avg_latency_ms),
            error_rate_trend: self.calculate_trend(&recent_snapshots, |s| s.error_rate),
            memory_trend: self.calculate_trend(&recent_snapshots, |s| s.memory_usage_mb),
        }
    }

    async fn calculate_command_rate(&self) -> f64 {
        // Get rate from Prometheus metrics
        // Implementation depends on your metrics backend
        0.0
    }

    async fn calculate_event_rate(&self) -> f64 {
        // Get rate from Prometheus metrics
        0.0
    }

    async fn calculate_avg_latency(&self) -> f64 {
        // Get average latency from metrics
        0.0
    }

    async fn calculate_p95_latency(&self) -> f64 {
        // Get p95 latency from metrics
        0.0
    }

    async fn calculate_p99_latency(&self) -> f64 {
        // Get p99 latency from metrics
        0.0
    }

    async fn calculate_error_rate(&self) -> f64 {
        // Calculate error rate from metrics
        0.0
    }

    async fn get_active_stream_count(&self) -> i64 {
        // Get active stream count from metrics
        0
    }

    async fn get_memory_usage_mb(&self) -> f64 {
        // Get memory usage from system metrics
        0.0
    }

    fn calculate_trend<F>(&self, snapshots: &[&PerformanceSnapshot], extractor: F) -> Trend
    where
        F: Fn(&PerformanceSnapshot) -> f64,
    {
        if snapshots.len() < 2 {
            return Trend::Stable;
        }

        let values: Vec<f64> = snapshots.iter().map(|s| extractor(s)).collect();
        let first_half = &values[0..values.len()/2];
        let second_half = &values[values.len()/2..];

        let first_avg = first_half.iter().sum::<f64>() / first_half.len() as f64;
        let second_avg = second_half.iter().sum::<f64>() / second_half.len() as f64;

        let change_percent = (second_avg - first_avg) / first_avg * 100.0;

        match change_percent {
            x if x > 10.0 => Trend::Increasing,
            x if x < -10.0 => Trend::Decreasing,
            _ => Trend::Stable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    pub throughput_trend: Trend,
    pub latency_trend: Trend,
    pub error_rate_trend: Trend,
    pub memory_trend: Trend,
}

#[derive(Debug, Clone)]
pub enum Trend {
    Increasing,
    Decreasing,
    Stable,
}

impl Default for TrendAnalysis {
    fn default() -> Self {
        Self {
            throughput_trend: Trend::Stable,
            latency_trend: Trend::Stable,
            error_rate_trend: Trend::Stable,
            memory_trend: Trend::Stable,
        }
    }
}
```

## Best Practices

1. **Comprehensive metrics** - Monitor all key system components
2. **Structured logging** - Use consistent, searchable log formats
3. **Distributed tracing** - Track requests across service boundaries
4. **Proactive alerting** - Alert on trends, not just thresholds
5. **Performance baselines** - Establish and monitor performance baselines
6. **Dashboard organization** - Create role-specific dashboards
7. **Alert fatigue** - Tune alerts to reduce noise
8. **Runbook automation** - Automate common response procedures

## Summary

EventCore monitoring and metrics:

- ✅ **Prometheus metrics** - Comprehensive system monitoring
- ✅ **Structured logging** - Searchable, contextual logs
- ✅ **Distributed tracing** - Request flow visibility
- ✅ **Intelligent alerting** - Proactive issue detection
- ✅ **Performance monitoring** - Real-time performance tracking

Key components:

1. Export detailed Prometheus metrics for all operations
2. Implement structured logging with correlation IDs
3. Use distributed tracing for multi-service visibility
4. Configure intelligent alerting with appropriate thresholds
5. Build comprehensive dashboards for different audiences

Next, let's explore [Backup and Recovery](./03-backup-recovery.md) →
