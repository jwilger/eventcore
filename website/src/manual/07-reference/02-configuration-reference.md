# Chapter 7.2: Configuration Reference

This chapter provides a complete reference for all EventCore configuration options. Use this as a lookup guide when setting up and tuning your EventCore applications.

## Core Configuration

### EventStore Configuration

Configuration for event store implementations.

#### PostgresConfig

Configuration for PostgreSQL event store.

```rust
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub database_url: String,
    pub pool_config: PoolConfig,
    pub migration_config: MigrationConfig,
    pub performance_config: PerformanceConfig,
    pub security_config: SecurityConfig,
}

impl PostgresConfig {
    pub fn new(database_url: String) -> Self
    pub fn from_env() -> Result<Self, ConfigError>
    pub fn with_pool_config(mut self, config: PoolConfig) -> Self
    pub fn with_migration_config(mut self, config: MigrationConfig) -> Self
}
```

**Example:**
```rust
let config = PostgresConfig::new("postgresql://localhost/eventcore".to_string())
    .with_pool_config(PoolConfig {
        max_connections: 20,
        min_connections: 5,
        connect_timeout: Duration::from_secs(10),
        idle_timeout: Some(Duration::from_secs(300)),
        max_lifetime: Some(Duration::from_secs(1800)),
    })
    .with_migration_config(MigrationConfig {
        auto_migrate: true,
        migration_timeout: Duration::from_secs(60),
    });
```

#### PoolConfig

Database connection pool configuration.

```rust
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    
    /// Minimum number of connections to maintain
    pub min_connections: u32,
    
    /// Timeout for establishing new connections
    pub connect_timeout: Duration,
    
    /// Maximum time a connection can be idle before being closed
    pub idle_timeout: Option<Duration>,
    
    /// Maximum lifetime of a connection
    pub max_lifetime: Option<Duration>,
    
    /// Test connections before use
    pub test_before_acquire: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 2,
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(3600)),
            test_before_acquire: true,
        }
    }
}
```

**Tuning Guidelines:**
- **max_connections**: 2-4x CPU cores for CPU-bound workloads, higher for I/O-bound
- **min_connections**: 10-20% of max_connections
- **connect_timeout**: 5-10 seconds for local databases, 15-30 seconds for remote
- **idle_timeout**: 5-10 minutes to balance connection reuse and resource usage
- **max_lifetime**: 30-60 minutes to prevent connection staleness

#### MigrationConfig

Database migration configuration.

```rust
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Automatically run migrations on startup
    pub auto_migrate: bool,
    
    /// Timeout for migration operations
    pub migration_timeout: Duration,
    
    /// Lock timeout for migration coordination
    pub lock_timeout: Duration,
    
    /// Migration table name
    pub migration_table: String,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            auto_migrate: false,
            migration_timeout: Duration::from_secs(300),
            lock_timeout: Duration::from_secs(60),
            migration_table: "_sqlx_migrations".to_string(),
        }
    }
}
```

### Command Execution Configuration

#### CommandExecutorConfig

Configuration for command execution behavior.

```rust
#[derive(Debug, Clone)]
pub struct CommandExecutorConfig {
    pub retry_config: RetryConfig,
    pub timeout_config: TimeoutConfig,
    pub concurrency_config: ConcurrencyConfig,
    pub metrics_config: MetricsConfig,
}

impl Default for CommandExecutorConfig {
    fn default() -> Self {
        Self {
            retry_config: RetryConfig::default(),
            timeout_config: TimeoutConfig::default(),
            concurrency_config: ConcurrencyConfig::default(),
            metrics_config: MetricsConfig::default(),
        }
    }
}
```

#### RetryConfig

Configuration for command retry behavior.

```rust
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    
    /// Initial delay before first retry
    pub initial_delay: Duration,
    
    /// Maximum delay between retries
    pub max_delay: Duration,
    
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    
    /// Which types of errors to retry
    pub retry_policy: RetryPolicy,
    
    /// Add jitter to prevent thundering herd
    pub jitter: bool,
}

impl RetryConfig {
    pub fn none() -> Self {
        Self {
            max_attempts: 0,
            ..Default::default()
        }
    }
    
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 10,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 1.5,
            retry_policy: RetryPolicy::All,
            jitter: true,
        }
    }
    
    pub fn conservative() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            backoff_multiplier: 2.0,
            retry_policy: RetryPolicy::ConcurrencyConflictsOnly,
            jitter: true,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            retry_policy: RetryPolicy::TransientErrorsOnly,
            jitter: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// Never retry
    None,
    
    /// Only retry concurrency conflicts
    ConcurrencyConflictsOnly,
    
    /// Only retry transient errors (connection issues, timeouts)
    TransientErrorsOnly,
    
    /// Retry all retryable errors
    All,
}
```

**Retry Policy Guidelines:**
- **ConcurrencyConflictsOnly**: Use for high-conflict scenarios where immediate retry is beneficial
- **TransientErrorsOnly**: Use for stable systems where business logic errors shouldn't be retried
- **All**: Use for development or systems where any failure might be recoverable

#### TimeoutConfig

Configuration for command timeouts.

```rust
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Default timeout for command execution
    pub default_timeout: Duration,
    
    /// Timeout for reading streams
    pub read_timeout: Duration,
    
    /// Timeout for writing events
    pub write_timeout: Duration,
    
    /// Timeout for stream discovery
    pub discovery_timeout: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(15),
            discovery_timeout: Duration::from_secs(5),
        }
    }
}
```

#### ConcurrencyConfig

Configuration for concurrent command execution.

```rust
#[derive(Debug, Clone)]
pub struct ConcurrencyConfig {
    /// Maximum number of concurrent commands
    pub max_concurrent_commands: usize,
    
    /// Maximum iterations for stream discovery
    pub max_discovery_iterations: usize,
    
    /// Enable command batching
    pub enable_batching: bool,
    
    /// Maximum batch size for event writes
    pub max_batch_size: usize,
    
    /// Batch timeout
    pub batch_timeout: Duration,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_commands: 100,
            max_discovery_iterations: 10,
            enable_batching: true,
            max_batch_size: 1000,
            batch_timeout: Duration::from_millis(100),
        }
    }
}
```

**Concurrency Tuning:**
- **max_concurrent_commands**: Balance between throughput and resource usage
- **max_discovery_iterations**: Higher values allow more complex stream patterns but increase latency
- **max_batch_size**: Larger batches improve throughput but increase memory usage and latency

## Projection Configuration

### ProjectionConfig

Configuration for projection management.

```rust
#[derive(Debug, Clone)]
pub struct ProjectionConfig {
    pub checkpoint_config: CheckpointConfig,
    pub processing_config: ProcessingConfig,
    pub recovery_config: RecoveryConfig,
}

#### CheckpointConfig

Configuration for projection checkpointing.

```rust
#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    /// How often to save checkpoints
    pub checkpoint_interval: Duration,
    
    /// Number of events to process before checkpointing
    pub events_per_checkpoint: usize,
    
    /// Store for checkpoint persistence
    pub checkpoint_store: CheckpointStoreConfig,
    
    /// Enable checkpoint compression
    pub compress_checkpoints: bool,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: Duration::from_secs(30),
            events_per_checkpoint: 1000,
            checkpoint_store: CheckpointStoreConfig::Database,
            compress_checkpoints: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CheckpointStoreConfig {
    /// Store checkpoints in the main database
    Database,
    
    /// Store checkpoints in Redis
    Redis { connection_string: String },
    
    /// Store checkpoints in memory (testing only)
    InMemory,
    
    /// Custom checkpoint store
    Custom { store_type: String, config: HashMap<String, String> },
}
```

#### ProcessingConfig

Configuration for event processing.

```rust
#[derive(Debug, Clone)]
pub struct ProcessingConfig {
    /// Number of events to process in each batch
    pub batch_size: usize,
    
    /// Timeout for processing a single event
    pub event_timeout: Duration,
    
    /// Timeout for processing a batch
    pub batch_timeout: Duration,
    
    /// Number of parallel processors
    pub parallelism: usize,
    
    /// Buffer size for event queues
    pub buffer_size: usize,
    
    /// Error handling strategy
    pub error_handling: ErrorHandlingStrategy,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            event_timeout: Duration::from_secs(5),
            batch_timeout: Duration::from_secs(30),
            parallelism: 1,
            buffer_size: 10000,
            error_handling: ErrorHandlingStrategy::SkipAndLog,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ErrorHandlingStrategy {
    /// Skip failed events and log errors
    SkipAndLog,
    
    /// Stop processing on first error
    FailFast,
    
    /// Retry failed events with backoff
    Retry { max_attempts: u32, backoff: Duration },
    
    /// Send failed events to dead letter queue
    DeadLetter { queue_config: DeadLetterConfig },
}
```

## Monitoring Configuration

### MetricsConfig

Configuration for metrics collection.

```rust
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    
    /// Metrics export format
    pub export_format: MetricsFormat,
    
    /// Export interval
    pub export_interval: Duration,
    
    /// Histogram buckets for latency metrics
    pub latency_buckets: Vec<f64>,
    
    /// Labels to add to all metrics
    pub default_labels: HashMap<String, String>,
    
    /// Metrics to collect
    pub collectors: Vec<MetricsCollector>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_format: MetricsFormat::Prometheus,
            export_interval: Duration::from_secs(15),
            latency_buckets: vec![
                0.001, 0.005, 0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
            ],
            default_labels: HashMap::new(),
            collectors: vec![
                MetricsCollector::Commands,
                MetricsCollector::Events,
                MetricsCollector::Projections,
                MetricsCollector::System,
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub enum MetricsFormat {
    Prometheus,
    OpenTelemetry,
    StatsD,
    Custom { format: String },
}

#[derive(Debug, Clone)]
pub enum MetricsCollector {
    Commands,
    Events,
    Projections,
    System,
    Custom { name: String },
}
```

### TracingConfig

Configuration for distributed tracing.

```rust
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Enable tracing
    pub enabled: bool,
    
    /// Tracing exporter configuration
    pub exporter: TracingExporter,
    
    /// Sampling configuration
    pub sampling: SamplingConfig,
    
    /// Resource attributes
    pub resource_attributes: HashMap<String, String>,
    
    /// Span attributes to add to all spans
    pub default_span_attributes: HashMap<String, String>,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exporter: TracingExporter::Jaeger {
                endpoint: "http://localhost:14268/api/traces".to_string(),
            },
            sampling: SamplingConfig::default(),
            resource_attributes: HashMap::new(),
            default_span_attributes: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TracingExporter {
    Jaeger { endpoint: String },
    Zipkin { endpoint: String },
    OpenTelemetry { endpoint: String },
    Console,
    None,
}

#[derive(Debug, Clone)]
pub struct SamplingConfig {
    /// Sampling rate (0.0 to 1.0)
    pub sample_rate: f64,
    
    /// Always sample errors
    pub always_sample_errors: bool,
    
    /// Sampling strategy
    pub strategy: SamplingStrategy,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 0.1,
            always_sample_errors: true,
            strategy: SamplingStrategy::Probabilistic,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SamplingStrategy {
    /// Always sample
    Always,
    
    /// Never sample
    Never,
    
    /// Probabilistic sampling
    Probabilistic,
    
    /// Rate limiting sampling
    RateLimit { max_per_second: u32 },
}
```

### LoggingConfig

Configuration for structured logging.

```rust
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level
    pub level: LogLevel,
    
    /// Log format
    pub format: LogFormat,
    
    /// Output destination
    pub output: LogOutput,
    
    /// Include timestamps
    pub include_timestamps: bool,
    
    /// Include source code locations
    pub include_locations: bool,
    
    /// Correlation ID header name
    pub correlation_id_header: String,
    
    /// Fields to include in all log entries
    pub default_fields: HashMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Json,
            output: LogOutput::Stdout,
            include_timestamps: true,
            include_locations: false,
            correlation_id_header: "x-correlation-id".to_string(),
            default_fields: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub enum LogFormat {
    Json,
    Logfmt,
    Pretty,
    Compact,
}

#[derive(Debug, Clone)]
pub enum LogOutput {
    Stdout,
    Stderr,
    File { path: String, rotation: RotationConfig },
    Syslog { facility: String },
    Network { endpoint: String },
}

#[derive(Debug, Clone)]
pub struct RotationConfig {
    pub max_size_mb: u64,
    pub max_files: u32,
    pub compress: bool,
}
```

## Security Configuration

### SecurityConfig

Configuration for security features.

```rust
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub tls_config: Option<TlsConfig>,
    pub auth_config: AuthConfig,
    pub encryption_config: EncryptionConfig,
}

#### TlsConfig

Configuration for TLS encryption.

```rust
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to certificate file
    pub cert_file: String,
    
    /// Path to private key file
    pub key_file: String,
    
    /// Path to CA certificate file (for client verification)
    pub ca_file: Option<String>,
    
    /// Require client certificates
    pub require_client_cert: bool,
    
    /// Minimum TLS version
    pub min_version: TlsVersion,
    
    /// Allowed cipher suites
    pub cipher_suites: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TlsVersion {
    V1_2,
    V1_3,
}
```

#### AuthConfig

Configuration for authentication.

```rust
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Authentication provider
    pub provider: AuthProvider,
    
    /// Token validation settings
    pub token_validation: TokenValidationConfig,
    
    /// Session configuration
    pub session_config: SessionConfig,
}

#[derive(Debug, Clone)]
pub enum AuthProvider {
    /// JWT-based authentication
    Jwt { 
        secret_key: String,
        algorithm: JwtAlgorithm,
        issuer: Option<String>,
        audience: Option<String>,
    },
    
    /// OAuth2 authentication
    OAuth2 {
        client_id: String,
        client_secret: String,
        auth_url: String,
        token_url: String,
        scopes: Vec<String>,
    },
    
    /// API key authentication
    ApiKey {
        header_name: String,
        query_param: Option<String>,
    },
    
    /// Custom authentication
    Custom { provider_type: String, config: HashMap<String, String> },
}

#[derive(Debug, Clone)]
pub enum JwtAlgorithm {
    HS256,
    HS384,
    HS512,
    RS256,
    RS384,
    RS512,
    ES256,
    ES384,
    ES512,
}
```

#### EncryptionConfig

Configuration for data encryption.

```rust
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// Enable encryption at rest
    pub encrypt_at_rest: bool,
    
    /// Encryption algorithm
    pub algorithm: EncryptionAlgorithm,
    
    /// Key management configuration
    pub key_management: KeyManagementConfig,
    
    /// Fields to encrypt
    pub encrypted_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum EncryptionAlgorithm {
    AES256GCM,
    ChaCha20Poly1305,
    XChaCha20Poly1305,
}

#[derive(Debug, Clone)]
pub enum KeyManagementConfig {
    /// Environment variable
    Environment { key_var: String },
    
    /// AWS KMS
    AwsKms { key_id: String, region: String },
    
    /// HashiCorp Vault
    Vault { endpoint: String, token: String, key_path: String },
    
    /// File-based key storage
    File { key_file: String },
}
```

## Environment Variables

EventCore supports configuration via environment variables with the `EVENTCORE_` prefix:

### Core Settings
```bash
# Database configuration
EVENTCORE_DATABASE_URL=postgresql://localhost/eventcore
EVENTCORE_DATABASE_MAX_CONNECTIONS=20
EVENTCORE_DATABASE_MIN_CONNECTIONS=5
EVENTCORE_DATABASE_CONNECT_TIMEOUT=10
EVENTCORE_DATABASE_IDLE_TIMEOUT=300
EVENTCORE_DATABASE_MAX_LIFETIME=1800

# Command execution
EVENTCORE_COMMAND_DEFAULT_TIMEOUT=30
EVENTCORE_COMMAND_MAX_RETRIES=5
EVENTCORE_COMMAND_RETRY_DELAY_MS=50
EVENTCORE_COMMAND_MAX_CONCURRENT=100

# Projections
EVENTCORE_PROJECTION_BATCH_SIZE=100
EVENTCORE_PROJECTION_CHECKPOINT_INTERVAL=30
EVENTCORE_PROJECTION_EVENTS_PER_CHECKPOINT=1000

# Metrics and monitoring
EVENTCORE_METRICS_ENABLED=true
EVENTCORE_METRICS_EXPORT_INTERVAL=15
EVENTCORE_TRACING_ENABLED=true
EVENTCORE_TRACING_SAMPLE_RATE=0.1

# Security
EVENTCORE_JWT_SECRET=your-secret-key
EVENTCORE_TLS_CERT_FILE=/path/to/cert.pem
EVENTCORE_TLS_KEY_FILE=/path/to/key.pem
EVENTCORE_ENCRYPT_AT_REST=true
```

### Logging Configuration
```bash
EVENTCORE_LOG_LEVEL=info
EVENTCORE_LOG_FORMAT=json
EVENTCORE_LOG_OUTPUT=stdout
EVENTCORE_LOG_INCLUDE_TIMESTAMPS=true
EVENTCORE_LOG_INCLUDE_LOCATIONS=false
```

### Development Settings
```bash
# Development mode settings
EVENTCORE_DEV_MODE=true
EVENTCORE_DEV_AUTO_MIGRATE=true
EVENTCORE_DEV_RESET_DB=false
EVENTCORE_DEV_SEED_DATA=true

# Testing settings  
EVENTCORE_TEST_DATABASE_URL=postgresql://localhost/eventcore_test
EVENTCORE_TEST_PARALLEL=true
EVENTCORE_TEST_RESET_BETWEEN_TESTS=true
```

## Configuration Files

### TOML Configuration Example

```toml
# eventcore.toml

[database]
url = "postgresql://localhost/eventcore"
max_connections = 20
min_connections = 5
connect_timeout = "10s"
idle_timeout = "5m"
max_lifetime = "30m"

[commands]
default_timeout = "30s"
max_retries = 5
retry_delay = "50ms"
max_concurrent = 100
max_discovery_iterations = 10

[projections]
batch_size = 100
checkpoint_interval = "30s"
events_per_checkpoint = 1000
parallelism = 1

[metrics]
enabled = true
export_format = "prometheus"
export_interval = "15s"
latency_buckets = [0.001, 0.005, 0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]

[tracing]
enabled = true
exporter = "jaeger"
jaeger_endpoint = "http://localhost:14268/api/traces"
sample_rate = 0.1
always_sample_errors = true

[logging]
level = "info"
format = "json"
output = "stdout"
include_timestamps = true
include_locations = false

[security]
encrypt_at_rest = true
jwt_secret = "${JWT_SECRET}"

[security.tls]
cert_file = "/etc/ssl/certs/eventcore.pem"
key_file = "/etc/ssl/private/eventcore.key"
require_client_cert = false
min_version = "1.3"
```

### YAML Configuration Example

```yaml
# eventcore.yaml

database:
  url: postgresql://localhost/eventcore
  pool:
    max_connections: 20
    min_connections: 5
    connect_timeout: 10s
    idle_timeout: 5m
    max_lifetime: 30m
  migration:
    auto_migrate: false
    migration_timeout: 5m

commands:
  timeout:
    default_timeout: 30s
    read_timeout: 10s
    write_timeout: 15s
  retry:
    max_attempts: 5
    initial_delay: 50ms
    max_delay: 1s
    backoff_multiplier: 2.0
    policy: transient_errors_only
    jitter: true
  concurrency:
    max_concurrent_commands: 100
    max_discovery_iterations: 10
    enable_batching: true
    max_batch_size: 1000

projections:
  checkpoint:
    interval: 30s
    events_per_checkpoint: 1000
    store: database
    compress: true
  processing:
    batch_size: 100
    event_timeout: 5s
    batch_timeout: 30s
    parallelism: 1
    error_handling: skip_and_log

monitoring:
  metrics:
    enabled: true
    export_format: prometheus
    export_interval: 15s
    collectors:
      - commands
      - events
      - projections
      - system
  tracing:
    enabled: true
    exporter:
      type: jaeger
      endpoint: http://localhost:14268/api/traces
    sampling:
      sample_rate: 0.1
      always_sample_errors: true
  logging:
    level: info
    format: json
    output: stdout
    correlation_id_header: x-correlation-id

security:
  auth:
    provider:
      type: jwt
      secret_key: ${JWT_SECRET}
      algorithm: HS256
  encryption:
    encrypt_at_rest: true
    algorithm: AES256GCM
    key_management:
      type: environment
      key_var: ENCRYPTION_KEY
```

## Configuration Loading

EventCore supports multiple configuration sources with the following precedence order:

1. **Command line arguments** (highest priority)
2. **Environment variables**
3. **Configuration files** (TOML, YAML, JSON)
4. **Default values** (lowest priority)

### Loading Configuration in Code

```rust
use eventcore::config::{EventCoreConfig, ConfigBuilder};

// Load from environment and files
let config = EventCoreConfig::from_env()
    .expect("Failed to load configuration");

// Custom configuration loading
let config = ConfigBuilder::new()
    .load_from_file("config/eventcore.toml")?
    .load_from_env()?
    .override_with_args(std::env::args())?
    .build()?;

// Validate configuration
config.validate()?;
```

This completes the configuration reference. All EventCore configuration options are documented with examples, default values, and tuning guidelines.

Next, explore [Error Reference](./03-error-reference.md) →