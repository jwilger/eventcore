//! Production configuration example for EventCore applications
//! 
//! This example demonstrates best practices for configuring EventCore
//! in production environments, including connection pooling, monitoring,
//! error handling, and graceful shutdown.

use eventcore::{
    prelude::*,
    monitoring::{
        exporters::{bridge::MonitoringBuilder, opentelemetry::OpenTelemetryExporter},
        metrics::MetricsRegistry,
    },
};
use eventcore_postgres::{PostgresConfig, PostgresEventStore};
use std::{sync::Arc, time::Duration};
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Production-ready configuration for EventCore
#[derive(Debug, Clone)]
pub struct ProductionConfig {
    /// Database configuration
    pub database: DatabaseConfig,
    /// Server configuration
    pub server: ServerConfig,
    /// Monitoring configuration
    pub monitoring: MonitoringConfig,
    /// Resilience configuration
    pub resilience: ResilienceConfig,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub shutdown_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    pub metrics_port: u16,
    pub otlp_endpoint: Option<String>,
    pub service_name: String,
    pub service_version: String,
    pub environment: String,
}

#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_timeout: Duration,
    pub retry_max_attempts: u32,
    pub retry_backoff: Duration,
    pub timeout_per_operation: Duration,
}

impl ProductionConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            database: DatabaseConfig {
                url: std::env::var("DATABASE_URL")?,
                max_connections: std::env::var("DB_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "100".to_string())
                    .parse()?,
                min_connections: std::env::var("DB_MIN_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()?,
                connection_timeout: Duration::from_secs(
                    std::env::var("DB_CONNECTION_TIMEOUT_SECS")
                        .unwrap_or_else(|_| "30".to_string())
                        .parse()?,
                ),
                idle_timeout: Duration::from_secs(
                    std::env::var("DB_IDLE_TIMEOUT_SECS")
                        .unwrap_or_else(|_| "600".to_string())
                        .parse()?,
                ),
                max_lifetime: Duration::from_secs(
                    std::env::var("DB_MAX_LIFETIME_SECS")
                        .unwrap_or_else(|_| "1800".to_string())
                        .parse()?,
                ),
            },
            server: ServerConfig {
                host: std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: std::env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()?,
                workers: std::env::var("SERVER_WORKERS")
                    .unwrap_or_else(|_| num_cpus::get().to_string())
                    .parse()?,
                shutdown_timeout: Duration::from_secs(30),
            },
            monitoring: MonitoringConfig {
                metrics_port: std::env::var("METRICS_PORT")
                    .unwrap_or_else(|_| "9090".to_string())
                    .parse()?,
                otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
                service_name: std::env::var("SERVICE_NAME")
                    .unwrap_or_else(|_| "eventcore-app".to_string()),
                service_version: std::env::var("SERVICE_VERSION")
                    .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
                environment: std::env::var("ENVIRONMENT")
                    .unwrap_or_else(|_| "production".to_string()),
            },
            resilience: ResilienceConfig {
                circuit_breaker_threshold: 5,
                circuit_breaker_timeout: Duration::from_secs(60),
                retry_max_attempts: 3,
                retry_backoff: Duration::from_millis(100),
                timeout_per_operation: Duration::from_secs(30),
            },
        })
    }
}

/// Initialize structured logging for production
pub fn init_logging() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let formatting_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(formatting_layer)
        .init();
}

/// Create a production-ready event store with proper configuration
pub async fn create_event_store(
    config: &DatabaseConfig,
) -> Result<Arc<PostgresEventStore>, Box<dyn std::error::Error>> {
    let postgres_config = PostgresConfig::builder()
        .url(&config.url)
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connection_timeout(config.connection_timeout)
        .idle_timeout(Some(config.idle_timeout))
        .max_lifetime(Some(config.max_lifetime))
        .build()?;

    let store = PostgresEventStore::new(postgres_config).await?;
    
    // Run migrations
    store.run_migrations().await?;
    
    Ok(Arc::new(store))
}

/// Set up monitoring and observability
pub async fn setup_monitoring(
    config: &MonitoringConfig,
) -> Result<Arc<MetricsRegistry>, Box<dyn std::error::Error>> {
    let metrics_registry = Arc::new(MetricsRegistry::new());

    // Set up OpenTelemetry if configured
    if let Some(endpoint) = &config.otlp_endpoint {
        let otel_exporter = OpenTelemetryExporter::builder()
            .with_endpoint(endpoint)
            .with_service_name(&config.service_name)
            .with_service_version(&config.service_version)
            .with_environment(&config.environment)
            .build()?;

        let _monitoring = MonitoringBuilder::new(metrics_registry.clone())
            .with_metrics_exporter(Arc::new(otel_exporter))
            .with_export_interval(Duration::from_secs(10))
            .build()
            .await?;
    }

    // Start Prometheus metrics server
    let metrics_registry_clone = metrics_registry.clone();
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        if let Err(e) = start_metrics_server(metrics_registry_clone, metrics_port).await {
            error!("Failed to start metrics server: {}", e);
        }
    });

    Ok(metrics_registry)
}

/// Start Prometheus metrics server
async fn start_metrics_server(
    _metrics_registry: Arc<MetricsRegistry>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    use warp::Filter;

    let metrics = warp::path("metrics")
        .map(move || {
            // In a real implementation, format metrics for Prometheus
            "# HELP eventcore_commands_total Total number of commands executed\n\
             # TYPE eventcore_commands_total counter\n\
             eventcore_commands_total 42\n"
        });

    let health = warp::path("health")
        .map(|| warp::reply::json(&serde_json::json!({"status": "healthy"})));

    let routes = metrics.or(health);

    info!("Starting metrics server on port {}", port);
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
    Ok(())
}

/// Graceful shutdown handler
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, starting graceful shutdown");
        },
        _ = terminate => {
            info!("Received terminate signal, starting graceful shutdown");
        },
    }
}

/// Example circuit breaker wrapper for commands
pub struct CircuitBreaker<E: EventStore> {
    executor: CommandExecutor<E>,
    failure_count: std::sync::atomic::AtomicU32,
    last_failure: std::sync::Mutex<Option<std::time::Instant>>,
    config: ResilienceConfig,
}

impl<E: EventStore> CircuitBreaker<E> {
    pub fn new(executor: CommandExecutor<E>, config: ResilienceConfig) -> Self {
        Self {
            executor,
            failure_count: std::sync::atomic::AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(None),
            config,
        }
    }

    pub async fn execute<C>(&self, command: &C) -> CommandResult<CommandExecutionResult>
    where
        C: Command,
    {
        // Check if circuit is open
        if self.is_open() {
            return Err(CommandError::ServiceUnavailable(
                "Circuit breaker is open".to_string(),
            ));
        }

        // Execute with timeout
        let result = tokio::time::timeout(
            self.config.timeout_per_operation,
            self.executor.execute(command),
        )
        .await;

        match result {
            Ok(Ok(result)) => {
                // Reset on success
                self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
                Ok(result)
            }
            Ok(Err(e)) | Err(_) => {
                // Increment failure count
                let failures = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                
                if failures >= self.config.circuit_breaker_threshold {
                    *self.last_failure.lock().unwrap() = Some(std::time::Instant::now());
                }

                match result {
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(CommandError::ServiceUnavailable("Operation timed out".to_string())),
                }
            }
        }
    }

    fn is_open(&self) -> bool {
        let failures = self.failure_count.load(std::sync::atomic::Ordering::Relaxed);
        if failures < self.config.circuit_breaker_threshold {
            return false;
        }

        // Check if timeout has passed
        if let Some(last_failure) = *self.last_failure.lock().unwrap() {
            if last_failure.elapsed() > self.config.circuit_breaker_timeout {
                // Reset circuit
                self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
                return false;
            }
        }

        true
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();
    
    info!("Starting EventCore application");

    // Load configuration
    let config = ProductionConfig::from_env()?;
    info!("Configuration loaded successfully");

    // Create event store
    let event_store = create_event_store(&config.database).await?;
    info!("Event store initialized");

    // Set up monitoring
    let _metrics = setup_monitoring(&config.monitoring).await?;
    info!("Monitoring initialized");

    // Create command executor with circuit breaker
    let executor = CommandExecutor::new(event_store);
    let circuit_breaker = Arc::new(CircuitBreaker::new(executor, config.resilience.clone()));

    // Example: Start your application server here
    // let app = create_app(circuit_breaker.clone());
    // let server = warp::serve(app).run(([0, 0, 0, 0], config.server.port));

    // Wait for shutdown signal
    info!(
        "Application started on {}:{}",
        config.server.host, config.server.port
    );
    
    shutdown_signal().await;
    
    info!("Shutting down gracefully...");
    
    // Give ongoing requests time to complete
    tokio::time::sleep(config.server.shutdown_timeout).await;
    
    info!("Shutdown complete");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        std::env::set_var("DATABASE_URL", "postgres://localhost/test");
        std::env::set_var("DB_MAX_CONNECTIONS", "50");
        
        let config = ProductionConfig::from_env().unwrap();
        assert_eq!(config.database.max_connections, 50);
        assert_eq!(config.server.port, 8080);
    }
}