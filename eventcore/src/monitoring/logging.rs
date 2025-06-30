//! Structured logging infrastructure for EventCore.
//!
//! This module provides comprehensive structured logging capabilities with
//! consistent log formats, contextual information, and performance monitoring.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, Level};

use crate::errors::{CommandError, EventStoreError};
use crate::types::{EventId, StreamId};

/// Log level enumeration for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => Self::TRACE,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Info => Self::INFO,
            LogLevel::Warn => Self::WARN,
            LogLevel::Error => Self::ERROR,
        }
    }
}

/// Structured log entry with consistent format
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Log level
    pub level: LogLevel,
    /// Log message
    pub message: String,
    /// Timestamp of the log entry
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Trace ID for correlation
    pub trace_id: Option<String>,
    /// Component that generated the log
    pub component: String,
    /// Operation being performed
    pub operation: String,
    /// Structured fields
    pub fields: HashMap<String, Value>,
    /// Error information if applicable
    pub error: Option<String>,
    /// Duration information if applicable
    pub duration_ms: Option<u64>,
}

impl LogEntry {
    /// Creates a new log entry
    pub fn new(level: LogLevel, message: &str, component: &str, operation: &str) -> Self {
        Self {
            level,
            message: message.to_string(),
            timestamp: chrono::Utc::now(),
            trace_id: None,
            component: component.to_string(),
            operation: operation.to_string(),
            fields: HashMap::new(),
            error: None,
            duration_ms: None,
        }
    }

    /// Adds a trace ID for correlation
    pub fn with_trace_id(mut self, trace_id: String) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    /// Adds a field to the log entry
    pub fn with_field(mut self, key: &str, value: Value) -> Self {
        self.fields.insert(key.to_string(), value);
        self
    }

    /// Adds error information
    pub fn with_error(mut self, error: &str) -> Self {
        self.error = Some(error.to_string());
        self
    }

    /// Adds duration information
    #[allow(clippy::cast_possible_truncation)]
    pub const fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(duration.as_millis() as u64);
        self
    }

    /// Converts to JSON for structured output
    pub fn to_json(&self) -> Value {
        let mut json_obj = json!({
            "level": format!("{:?}", self.level).to_lowercase(),
            "message": self.message,
            "timestamp": self.timestamp.to_rfc3339(),
            "component": self.component,
            "operation": self.operation,
            "fields": self.fields
        });

        if let Some(trace_id) = &self.trace_id {
            json_obj["trace_id"] = json!(trace_id);
        }

        if let Some(error) = &self.error {
            json_obj["error"] = json!(error);
        }

        if let Some(duration_ms) = self.duration_ms {
            json_obj["duration_ms"] = json!(duration_ms);
        }

        json_obj
    }

    /// Logs the entry using tracing
    #[allow(clippy::cognitive_complexity)]
    pub fn log(&self) {
        let json_str = self.to_json().to_string();

        match self.level {
            LogLevel::Trace => tracing::trace!("{}", json_str),
            LogLevel::Debug => tracing::debug!("{}", json_str),
            LogLevel::Info => tracing::info!("{}", json_str),
            LogLevel::Warn => tracing::warn!("{}", json_str),
            LogLevel::Error => tracing::error!("{}", json_str),
        }
    }
}

/// Structured logger for EventCore components
pub struct StructuredLogger {
    component: String,
    default_fields: HashMap<String, Value>,
}

impl StructuredLogger {
    /// Creates a new structured logger for a component
    pub fn new(component: &str) -> Self {
        Self {
            component: component.to_string(),
            default_fields: HashMap::new(),
        }
    }

    /// Adds a default field that will be included in all log entries
    pub fn with_default_field(mut self, key: &str, value: Value) -> Self {
        self.default_fields.insert(key.to_string(), value);
        self
    }

    /// Creates a log entry with default fields
    fn create_entry(&self, level: LogLevel, message: &str, operation: &str) -> LogEntry {
        let mut entry = LogEntry::new(level, message, &self.component, operation);

        for (key, value) in &self.default_fields {
            entry = entry.with_field(key, value.clone());
        }

        entry
    }

    /// Logs a trace message
    pub fn trace(&self, operation: &str, message: &str) -> LogEntry {
        let entry = self.create_entry(LogLevel::Trace, message, operation);
        entry.log();
        entry
    }

    /// Logs a debug message
    pub fn debug(&self, operation: &str, message: &str) -> LogEntry {
        let entry = self.create_entry(LogLevel::Debug, message, operation);
        entry.log();
        entry
    }

    /// Logs an info message
    pub fn info(&self, operation: &str, message: &str) -> LogEntry {
        let entry = self.create_entry(LogLevel::Info, message, operation);
        entry.log();
        entry
    }

    /// Logs a warning message
    pub fn warn(&self, operation: &str, message: &str) -> LogEntry {
        let entry = self.create_entry(LogLevel::Warn, message, operation);
        entry.log();
        entry
    }

    /// Logs an error message
    pub fn error(&self, operation: &str, message: &str) -> LogEntry {
        let entry = self.create_entry(LogLevel::Error, message, operation);
        entry.log();
        entry
    }

    /// Logs command execution start
    pub fn log_command_start(
        &self,
        trace_id: &str,
        command_type: &str,
        stream_ids: &[StreamId],
        retry_count: u32,
    ) {
        self.info("command_execution", "Command execution started")
            .with_trace_id(trace_id.to_string())
            .with_field("command_type", json!(command_type))
            .with_field("stream_count", json!(stream_ids.len()))
            .with_field(
                "stream_ids",
                json!(stream_ids
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()),
            )
            .with_field("retry_count", json!(retry_count))
            .log();
    }

    /// Logs command execution completion
    pub fn log_command_completion(
        &self,
        trace_id: &str,
        command_type: &str,
        result: &Result<usize, CommandError>,
        duration: Duration,
        retry_count: u32,
    ) {
        let mut entry = match result {
            Ok(events_written) => self
                .info(
                    "command_execution",
                    "Command execution completed successfully",
                )
                .with_field("events_written", json!(events_written)),
            Err(error) => self
                .error("command_execution", "Command execution failed")
                .with_error(&format!("{error}"))
                .with_field("error_type", json!(Self::classify_command_error(error))),
        };

        entry = entry
            .with_trace_id(trace_id.to_string())
            .with_field("command_type", json!(command_type))
            .with_field("retry_count", json!(retry_count))
            .with_duration(duration);

        entry.log();
    }

    /// Logs event store operation start
    pub fn log_event_store_start(&self, trace_id: &str, operation: &str, stream_ids: &[StreamId]) {
        self.debug(
            "event_store_operation",
            &format!("Event store {operation} operation started"),
        )
        .with_trace_id(trace_id.to_string())
        .with_field("operation_type", json!(operation))
        .with_field("stream_count", json!(stream_ids.len()))
        .with_field(
            "stream_ids",
            json!(stream_ids
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()),
        )
        .log();
    }

    /// Logs event store operation completion
    pub fn log_event_store_completion(
        &self,
        trace_id: &str,
        operation: &str,
        result: &Result<usize, EventStoreError>,
        duration: Duration,
    ) {
        let mut entry = match result {
            Ok(event_count) => self
                .debug(
                    "event_store_operation",
                    &format!("Event store {operation} operation completed successfully"),
                )
                .with_field("event_count", json!(event_count)),
            Err(error) => self
                .warn(
                    "event_store_operation",
                    &format!("Event store {operation} operation failed"),
                )
                .with_error(&format!("{error}"))
                .with_field("error_type", json!(Self::classify_event_store_error(error))),
        };

        entry = entry
            .with_trace_id(trace_id.to_string())
            .with_field("operation_type", json!(operation))
            .with_duration(duration);

        entry.log();
    }

    /// Logs projection processing
    pub fn log_projection_processing(
        &self,
        trace_id: &str,
        projection_name: &str,
        event_id: &EventId,
        result: &Result<(), String>,
        duration: Duration,
    ) {
        let mut entry = match result {
            Ok(()) => self.debug(
                "projection_processing",
                "Projection processing completed successfully",
            ),
            Err(error) => self
                .warn("projection_processing", "Projection processing failed")
                .with_error(error),
        };

        entry = entry
            .with_trace_id(trace_id.to_string())
            .with_field("projection_name", json!(projection_name))
            .with_field("event_id", json!(event_id.to_string()))
            .with_duration(duration);

        entry.log();
    }

    /// Logs system health metrics
    pub fn log_health_metrics(
        &self,
        memory_usage_mb: f64,
        cpu_usage_percent: f64,
        connection_pool_utilization: f64,
        error_rate_percent: f64,
    ) {
        let level = if error_rate_percent > 10.0
            || cpu_usage_percent > 90.0
            || connection_pool_utilization > 95.0
        {
            LogLevel::Warn
        } else {
            LogLevel::Info
        };

        self.create_entry(level, "System health metrics updated", "health_check")
            .with_field("memory_usage_mb", json!(memory_usage_mb))
            .with_field("cpu_usage_percent", json!(cpu_usage_percent))
            .with_field(
                "connection_pool_utilization_percent",
                json!(connection_pool_utilization),
            )
            .with_field("error_rate_percent", json!(error_rate_percent))
            .log();
    }

    /// Logs performance alerts
    pub fn log_performance_alert(
        &self,
        alert_type: &str,
        threshold: f64,
        current_value: f64,
        context: &HashMap<String, Value>,
    ) {
        let mut entry = self
            .warn(
                "performance_alert",
                &format!("Performance threshold exceeded: {alert_type}"),
            )
            .with_field("alert_type", json!(alert_type))
            .with_field("threshold", json!(threshold))
            .with_field("current_value", json!(current_value))
            .with_field("exceeded_by", json!(current_value - threshold));

        for (key, value) in context {
            entry = entry.with_field(key, value.clone());
        }

        entry.log();
    }

    /// Logs security events
    pub fn log_security_event(
        &self,
        event_type: &str,
        user_id: Option<&str>,
        resource: &str,
        action: &str,
        result: &str,
    ) {
        let level = if result == "denied" || result == "failed" {
            LogLevel::Warn
        } else {
            LogLevel::Info
        };

        let mut entry = self
            .create_entry(level, &format!("Security event: {event_type}"), "security")
            .with_field("event_type", json!(event_type))
            .with_field("resource", json!(resource))
            .with_field("action", json!(action))
            .with_field("result", json!(result));

        if let Some(user_id) = user_id {
            entry = entry.with_field("user_id", json!(user_id));
        }

        entry.log();
    }

    /// Classifies command errors for consistent error categorization
    const fn classify_command_error(error: &CommandError) -> &'static str {
        match error {
            CommandError::ValidationFailed(_) => "validation_failed",
            CommandError::BusinessRuleViolation(_) => "business_rule_violation",
            CommandError::DomainError { .. } => "domain_error",
            CommandError::ConcurrencyConflict { .. } => "concurrency_conflict",
            CommandError::StreamNotFound(_) => "stream_not_found",
            CommandError::InvalidStreamAccess { .. } => "invalid_stream_access",
            CommandError::StreamNotDeclared { .. } => "stream_not_declared",
            CommandError::TypeMismatch { .. } => "type_mismatch",
            CommandError::Unauthorized(_) => "unauthorized",
            CommandError::EventStore(_) => "event_store_error",
            CommandError::Internal(_) => "internal_error",
            CommandError::Timeout(_) => "timeout",
        }
    }

    /// Classifies event store errors for consistent error categorization
    const fn classify_event_store_error(error: &EventStoreError) -> &'static str {
        match error {
            EventStoreError::StreamNotFound(_) => "stream_not_found",
            EventStoreError::VersionConflict { .. } => "version_conflict",
            EventStoreError::DuplicateEventId(_) => "duplicate_event_id",
            EventStoreError::ConnectionFailed(_) => "connection_failed",
            EventStoreError::Configuration(_) => "configuration_error",
            EventStoreError::TransactionRollback(_) => "transaction_rollback",
            EventStoreError::SerializationFailed(_) => "serialization_failed",
            EventStoreError::DeserializationFailed(_) => "deserialization_failed",
            EventStoreError::SchemaEvolutionError(_) => "schema_evolution_error",
            EventStoreError::Io(_) => "io_error",
            EventStoreError::Timeout(_) => "timeout",
            EventStoreError::Unavailable(_) => "unavailable",
            EventStoreError::Internal(_) => "internal_error",
        }
    }
}

/// Pre-configured loggers for common EventCore components
pub mod loggers {
    use super::StructuredLogger;

    /// Creates a command executor logger
    pub fn command_executor() -> StructuredLogger {
        StructuredLogger::new("command_executor")
    }

    /// Creates an event store logger
    pub fn event_store() -> StructuredLogger {
        StructuredLogger::new("event_store")
    }

    /// Creates a projection logger
    pub fn projection_manager() -> StructuredLogger {
        StructuredLogger::new("projection_manager")
    }

    /// Creates a subscription logger
    pub fn subscription_manager() -> StructuredLogger {
        StructuredLogger::new("subscription_manager")
    }

    /// Creates a health monitor logger
    pub fn health_monitor() -> StructuredLogger {
        StructuredLogger::new("health_monitor")
    }

    /// Creates a security logger
    pub fn security() -> StructuredLogger {
        StructuredLogger::new("security")
    }
}

/// Logging configuration for EventCore
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Minimum log level
    pub level: LogLevel,
    /// Whether to include source location in logs
    pub include_source: bool,
    /// Whether to use JSON format
    pub json_format: bool,
    /// Additional fields to include in all logs
    pub global_fields: HashMap<String, Value>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            include_source: false,
            json_format: true,
            global_fields: HashMap::new(),
        }
    }
}

impl LoggingConfig {
    /// Creates a new logging configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the minimum log level
    pub const fn with_level(mut self, level: LogLevel) -> Self {
        self.level = level;
        self
    }

    /// Enables source location in logs
    pub const fn with_source_location(mut self) -> Self {
        self.include_source = true;
        self
    }

    /// Enables plain text format instead of JSON
    pub const fn with_plain_format(mut self) -> Self {
        self.json_format = false;
        self
    }

    /// Adds a global field to all log entries
    pub fn with_global_field(mut self, key: &str, value: Value) -> Self {
        self.global_fields.insert(key.to_string(), value);
        self
    }

    /// Applies the configuration to the tracing subscriber
    pub fn apply(&self) {
        // Note: In a real implementation, this would configure tracing-subscriber
        // with the specified settings. For now, we'll just log the configuration.
        info!(
            level = ?self.level,
            include_source = self.include_source,
            json_format = self.json_format,
            global_fields = ?self.global_fields,
            "Logging configuration applied"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tracing_test::traced_test;

    #[test]
    fn test_log_entry_creation() {
        let entry = LogEntry::new(
            LogLevel::Info,
            "Test message",
            "test_component",
            "test_operation",
        );

        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Test message");
        assert_eq!(entry.component, "test_component");
        assert_eq!(entry.operation, "test_operation");
        assert!(entry.trace_id.is_none());
        assert!(entry.error.is_none());
        assert!(entry.duration_ms.is_none());
    }

    #[test]
    fn test_log_entry_with_fields() {
        let entry = LogEntry::new(
            LogLevel::Info,
            "Test message",
            "test_component",
            "test_operation",
        )
        .with_trace_id("trace-123".to_string())
        .with_field("key1", json!("value1"))
        .with_field("key2", json!(42))
        .with_error("Test error")
        .with_duration(Duration::from_millis(100));

        assert_eq!(entry.trace_id, Some("trace-123".to_string()));
        assert_eq!(entry.fields.get("key1"), Some(&json!("value1")));
        assert_eq!(entry.fields.get("key2"), Some(&json!(42)));
        assert_eq!(entry.error, Some("Test error".to_string()));
        assert_eq!(entry.duration_ms, Some(100));
    }

    #[test]
    fn test_log_entry_json_serialization() {
        let entry = LogEntry::new(
            LogLevel::Info,
            "Test message",
            "test_component",
            "test_operation",
        )
        .with_trace_id("trace-123".to_string())
        .with_field("key1", json!("value1"));

        let json_value = entry.to_json();

        assert_eq!(json_value["level"], "info");
        assert_eq!(json_value["message"], "Test message");
        assert_eq!(json_value["component"], "test_component");
        assert_eq!(json_value["operation"], "test_operation");
        assert_eq!(json_value["trace_id"], "trace-123");
        assert_eq!(json_value["fields"]["key1"], "value1");
    }

    #[test]
    fn test_structured_logger_creation() {
        let logger = StructuredLogger::new("test_component")
            .with_default_field("default_key", json!("default_value"));

        assert_eq!(logger.component, "test_component");
        assert_eq!(
            logger.default_fields.get("default_key"),
            Some(&json!("default_value"))
        );
    }

    #[traced_test]
    #[test]
    fn test_structured_logger_methods() {
        let logger = StructuredLogger::new("test_component");

        // Test all log level methods
        logger.trace("test_operation", "Trace message");
        logger.debug("test_operation", "Debug message");
        logger.info("test_operation", "Info message");
        logger.warn("test_operation", "Warn message");
        logger.error("test_operation", "Error message");

        // Test passes if no panic occurs
    }

    #[test]
    fn test_command_error_classification() {
        let error = CommandError::ValidationFailed("Test validation error".to_string());
        let classification = StructuredLogger::classify_command_error(&error);
        assert_eq!(classification, "validation_failed");
    }

    #[test]
    fn test_event_store_error_classification() {
        let error = EventStoreError::ConnectionFailed("Test connection error".to_string());
        let classification = StructuredLogger::classify_event_store_error(&error);
        assert_eq!(classification, "connection_failed");
    }

    #[test]
    fn test_logging_config() {
        let config = LoggingConfig::new()
            .with_level(LogLevel::Debug)
            .with_source_location()
            .with_plain_format()
            .with_global_field("service", json!("eventcore"));

        assert_eq!(config.level, LogLevel::Debug);
        assert!(config.include_source);
        assert!(!config.json_format);
        assert_eq!(
            config.global_fields.get("service"),
            Some(&json!("eventcore"))
        );
    }

    #[traced_test]
    #[test]
    fn test_pre_configured_loggers() {
        let cmd_logger = loggers::command_executor();
        let es_logger = loggers::event_store();
        let proj_logger = loggers::projection_manager();
        let sub_logger = loggers::subscription_manager();
        let health_logger = loggers::health_monitor();
        let security_logger = loggers::security();

        assert_eq!(cmd_logger.component, "command_executor");
        assert_eq!(es_logger.component, "event_store");
        assert_eq!(proj_logger.component, "projection_manager");
        assert_eq!(sub_logger.component, "subscription_manager");
        assert_eq!(health_logger.component, "health_monitor");
        assert_eq!(security_logger.component, "security");
    }
}
