//! Enhanced tracing and span management for EventCore.
//!
//! This module provides comprehensive distributed tracing capabilities with
//! proper span hierarchy, correlation IDs, and structured logging integration.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, instrument, span, warn, Level, Span};
use uuid::Uuid;

use crate::types::{EventId, StreamId};

/// Trace context for correlation across operations
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Unique trace identifier for the entire operation
    pub trace_id: String,
    /// Span identifier for the current operation
    pub span_id: String,
    /// Parent span identifier if this is a child span
    pub parent_span_id: Option<String>,
    /// Operation name for categorization
    pub operation_name: String,
    /// Additional context metadata
    pub metadata: HashMap<String, String>,
    /// Start time for duration tracking
    pub start_time: Instant,
}

impl TraceContext {
    /// Creates a new root trace context
    pub fn new(operation_name: &str) -> Self {
        let trace_id = Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string();
        let span_id = Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string();

        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            operation_name: operation_name.to_string(),
            metadata: HashMap::new(),
            start_time: Instant::now(),
        }
    }

    /// Creates a child trace context from a parent
    pub fn child(&self, operation_name: &str) -> Self {
        let span_id = Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string();

        Self {
            trace_id: self.trace_id.clone(),
            span_id,
            parent_span_id: Some(self.span_id.clone()),
            operation_name: operation_name.to_string(),
            metadata: self.metadata.clone(),
            start_time: Instant::now(),
        }
    }

    /// Adds metadata to the trace context
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Gets the elapsed time since the span started
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Creates a tracing span from this context
    pub fn create_span(&self) -> Span {
        let span = span!(
            Level::INFO,
            "eventcore_operation",
            trace_id = %self.trace_id,
            span_id = %self.span_id,
            parent_span_id = self.parent_span_id.as_deref().unwrap_or("none"),
            operation = %self.operation_name
        );

        // Add metadata as span fields
        for (key, value) in &self.metadata {
            span.record(key.as_str(), value.as_str());
        }

        span
    }
}

/// Enhanced tracing for command execution
pub struct CommandTracer {
    context: TraceContext,
    command_type: String,
    stream_ids: Vec<StreamId>,
    retry_count: u32,
}

impl CommandTracer {
    /// Creates a new command tracer
    pub fn new(command_type: &str, stream_ids: Vec<StreamId>) -> Self {
        let context = TraceContext::new("command_execution")
            .with_metadata("command_type", command_type)
            .with_metadata("stream_count", &stream_ids.len().to_string());

        Self {
            context,
            command_type: command_type.to_string(),
            stream_ids,
            retry_count: 0,
        }
    }

    /// Creates a child span for specific operations
    pub fn child_span(&self, operation: &str) -> TraceContext {
        self.context
            .child(operation)
            .with_metadata("command_type", &self.command_type)
            .with_metadata("retry_count", &self.retry_count.to_string())
    }

    /// Records retry attempt
    pub fn record_retry(&mut self) {
        self.retry_count += 1;
        warn!(
            trace_id = %self.context.trace_id,
            command_type = %self.command_type,
            retry_count = self.retry_count,
            "Command retry attempt"
        );
    }

    /// Records command completion
    #[instrument(skip(self))]
    pub fn record_completion(&self, success: bool, events_written: usize) {
        let duration = self.context.elapsed();

        if success {
            info!(
                trace_id = %self.context.trace_id,
                command_type = %self.command_type,
                duration_ms = duration.as_millis(),
                retry_count = self.retry_count,
                stream_count = self.stream_ids.len(),
                events_written = events_written,
                "Command execution completed successfully"
            );
        } else {
            error!(
                trace_id = %self.context.trace_id,
                command_type = %self.command_type,
                duration_ms = duration.as_millis(),
                retry_count = self.retry_count,
                stream_count = self.stream_ids.len(),
                "Command execution failed"
            );
        }
    }

    /// Gets the trace context
    pub const fn context(&self) -> &TraceContext {
        &self.context
    }
}

/// Enhanced tracing for event store operations
pub struct EventStoreTracer {
    context: TraceContext,
    operation_type: String,
    stream_ids: Vec<StreamId>,
}

impl EventStoreTracer {
    /// Creates a new event store tracer
    pub fn new(operation_type: &str, stream_ids: Vec<StreamId>) -> Self {
        let context = TraceContext::new("event_store_operation")
            .with_metadata("operation_type", operation_type)
            .with_metadata("stream_count", &stream_ids.len().to_string());

        Self {
            context,
            operation_type: operation_type.to_string(),
            stream_ids,
        }
    }

    /// Records operation start
    #[instrument(skip(self))]
    pub fn record_start(&self) {
        debug!(
            trace_id = %self.context.trace_id,
            operation_type = %self.operation_type,
            stream_count = self.stream_ids.len(),
            stream_ids = ?self.stream_ids,
            "Event store operation started"
        );
    }

    /// Records operation completion
    #[instrument(skip(self))]
    pub fn record_completion(&self, success: bool, event_count: usize) {
        let duration = self.context.elapsed();

        if success {
            debug!(
                trace_id = %self.context.trace_id,
                operation_type = %self.operation_type,
                duration_ms = duration.as_millis(),
                stream_count = self.stream_ids.len(),
                event_count = event_count,
                "Event store operation completed successfully"
            );
        } else {
            warn!(
                trace_id = %self.context.trace_id,
                operation_type = %self.operation_type,
                duration_ms = duration.as_millis(),
                stream_count = self.stream_ids.len(),
                "Event store operation failed"
            );
        }
    }

    /// Gets the trace context
    pub const fn context(&self) -> &TraceContext {
        &self.context
    }
}

/// Enhanced tracing for projection operations
pub struct ProjectionTracer {
    context: TraceContext,
    projection_name: String,
    event_id: Option<EventId>,
}

impl ProjectionTracer {
    /// Creates a new projection tracer
    pub fn new(projection_name: &str, event_id: Option<EventId>) -> Self {
        let mut context = TraceContext::new("projection_processing")
            .with_metadata("projection_name", projection_name);

        if let Some(event_id) = event_id {
            context = context.with_metadata("event_id", &event_id.to_string());
        }

        Self {
            context,
            projection_name: projection_name.to_string(),
            event_id,
        }
    }

    /// Records processing start
    #[instrument(skip(self))]
    pub fn record_start(&self) {
        debug!(
            trace_id = %self.context.trace_id,
            projection_name = %self.projection_name,
            event_id = self.event_id.as_ref().map(std::string::ToString::to_string).as_deref(),
            "Projection processing started"
        );
    }

    /// Records processing completion
    #[instrument(skip(self))]
    pub fn record_completion(&self, success: bool, events_processed: usize) {
        let duration = self.context.elapsed();

        if success {
            debug!(
                trace_id = %self.context.trace_id,
                projection_name = %self.projection_name,
                duration_ms = duration.as_millis(),
                events_processed = events_processed,
                "Projection processing completed successfully"
            );
        } else {
            warn!(
                trace_id = %self.context.trace_id,
                projection_name = %self.projection_name,
                duration_ms = duration.as_millis(),
                "Projection processing failed"
            );
        }
    }

    /// Records lag information
    #[instrument(skip(self))]
    pub fn record_lag(&self, lag_duration: Duration) {
        if lag_duration > Duration::from_secs(30) {
            warn!(
                trace_id = %self.context.trace_id,
                projection_name = %self.projection_name,
                lag_ms = lag_duration.as_millis(),
                "High projection lag detected"
            );
        } else {
            debug!(
                trace_id = %self.context.trace_id,
                projection_name = %self.projection_name,
                lag_ms = lag_duration.as_millis(),
                "Projection lag updated"
            );
        }
    }

    /// Gets the trace context
    pub const fn context(&self) -> &TraceContext {
        &self.context
    }
}

/// Span guard that automatically records completion
pub struct SpanGuard {
    span: Span,
    context: TraceContext,
    operation: String,
}

impl SpanGuard {
    /// Creates a new span guard
    pub fn new(context: TraceContext, operation: &str) -> Self {
        let span = context.create_span();

        info!(
            trace_id = %context.trace_id,
            operation = operation,
            "Operation started"
        );

        Self {
            span,
            context,
            operation: operation.to_string(),
        }
    }

    /// Records success and drops the span
    pub fn success(self) {
        let duration = self.context.elapsed();
        let _guard = self.span.enter();

        info!(
            trace_id = %self.context.trace_id,
            operation = %self.operation,
            duration_ms = duration.as_millis(),
            "Operation completed successfully"
        );
    }

    /// Records failure and drops the span
    pub fn failure(self, error: &str) {
        let duration = self.context.elapsed();
        let _guard = self.span.enter();

        error!(
            trace_id = %self.context.trace_id,
            operation = %self.operation,
            duration_ms = duration.as_millis(),
            error = error,
            "Operation failed"
        );
    }

    /// Gets the trace context
    pub const fn context(&self) -> &TraceContext {
        &self.context
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        let duration = self.context.elapsed();
        let _guard = self.span.enter();

        debug!(
            trace_id = %self.context.trace_id,
            operation = %self.operation,
            duration_ms = duration.as_millis(),
            "Operation span dropped"
        );
    }
}

/// Utility functions for common tracing patterns
pub mod utils {
    use super::{EventId, StreamId, TraceContext, Uuid};

    /// Creates a command execution span
    pub fn command_span(command_type: &str, correlation_id: Option<&str>) -> TraceContext {
        let mut context =
            TraceContext::new("command_execution").with_metadata("command_type", command_type);

        if let Some(correlation_id) = correlation_id {
            context = context.with_metadata("correlation_id", correlation_id);
        }

        context
    }

    /// Creates an event store operation span
    pub fn event_store_span(operation: &str, stream_ids: &[StreamId]) -> TraceContext {
        TraceContext::new("event_store_operation")
            .with_metadata("operation", operation)
            .with_metadata("stream_count", &stream_ids.len().to_string())
    }

    /// Creates a projection processing span
    pub fn projection_span(projection_name: &str, event_id: &EventId) -> TraceContext {
        TraceContext::new("projection_processing")
            .with_metadata("projection_name", projection_name)
            .with_metadata("event_id", &event_id.to_string())
    }

    /// Extracts trace context from span metadata (simplified implementation)
    pub fn extract_trace_context() -> Option<String> {
        // In a real implementation, we'd extract the actual trace_id value from span context
        // This is a simplified version for demonstration that generates a new trace ID
        Some(Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[test]
    fn test_trace_context_creation() {
        let context = TraceContext::new("test_operation");
        assert_eq!(context.operation_name, "test_operation");
        assert!(context.parent_span_id.is_none());
        assert!(!context.trace_id.is_empty());
        assert!(!context.span_id.is_empty());
    }

    #[test]
    fn test_child_context_creation() {
        let parent = TraceContext::new("parent_operation");
        let child = parent.child("child_operation");

        assert_eq!(child.operation_name, "child_operation");
        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id.clone()));
        assert_ne!(child.span_id, parent.span_id);
    }

    #[test]
    fn test_metadata_addition() {
        let context = TraceContext::new("test_operation")
            .with_metadata("key1", "value1")
            .with_metadata("key2", "value2");

        assert_eq!(context.metadata.get("key1"), Some(&"value1".to_string()));
        assert_eq!(context.metadata.get("key2"), Some(&"value2".to_string()));
    }

    #[traced_test]
    #[test]
    fn test_command_tracer() {
        let stream_ids = vec![
            crate::types::StreamId::try_new("stream-1").unwrap(),
            crate::types::StreamId::try_new("stream-2").unwrap(),
        ];

        let mut tracer = CommandTracer::new("TestCommand", stream_ids);
        assert_eq!(tracer.command_type, "TestCommand");
        assert_eq!(tracer.retry_count, 0);

        tracer.record_retry();
        assert_eq!(tracer.retry_count, 1);

        tracer.record_completion(true, 3);
        // Test passes if no panic occurs
    }

    #[traced_test]
    #[test]
    fn test_event_store_tracer() {
        let stream_ids = vec![crate::types::StreamId::try_new("stream-1").unwrap()];

        let tracer = EventStoreTracer::new("read", stream_ids);
        assert_eq!(tracer.operation_type, "read");

        tracer.record_start();
        tracer.record_completion(true, 5);
        // Test passes if no panic occurs
    }

    #[traced_test]
    #[test]
    fn test_projection_tracer() {
        let event_id = crate::types::EventId::new();
        let tracer = ProjectionTracer::new("TestProjection", Some(event_id));

        assert_eq!(tracer.projection_name, "TestProjection");
        assert_eq!(tracer.event_id, Some(event_id));

        tracer.record_start();
        tracer.record_completion(true, 1);
        tracer.record_lag(Duration::from_millis(100));
        // Test passes if no panic occurs
    }

    #[traced_test]
    #[test]
    fn test_span_guard() {
        let context = TraceContext::new("test_operation");
        let guard = SpanGuard::new(context, "test_span");

        // Test success path
        guard.success();
        // Test passes if no panic occurs
    }

    #[traced_test]
    #[test]
    fn test_span_guard_failure() {
        let context = TraceContext::new("test_operation");
        let guard = SpanGuard::new(context, "test_span");

        // Test failure path
        guard.failure("Test error message");
        // Test passes if no panic occurs
    }
}
