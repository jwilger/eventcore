-- Migration: Additional performance indexes for optimized event sourcing operations
-- These indexes support common query patterns in multi-stream event sourcing

-- Composite index for multi-stream reads (multi-stream event sourcing)
-- Optimizes reading multiple streams simultaneously
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_multistream_read 
ON events (stream_id, event_version, created_at);

-- Index for projection catchup scenarios (reading events after a specific timestamp)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_projection_catchup 
ON events (created_at, event_type, stream_id) 
WHERE created_at > NOW() - INTERVAL '7 days';

-- Index for saga coordination (finding events by correlation across streams)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_saga_correlation 
ON events (correlation_id, created_at, stream_id) 
WHERE correlation_id IS NOT NULL;

-- Index for debugging and audit trails (finding causation chains)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_causation_chain 
ON events (causation_id, created_at) 
WHERE causation_id IS NOT NULL;

-- Covering index for stream version checks (avoids table lookups)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_stream_version_covering 
ON events (stream_id, event_version) 
INCLUDE (event_id, created_at);

-- Index for event replay scenarios (temporal queries)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_temporal_replay 
ON events (created_at, stream_id, event_version)
WHERE created_at > NOW() - INTERVAL '90 days';

-- Partial index for error recovery (events without proper metadata)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_error_recovery 
ON events (stream_id, created_at) 
WHERE metadata IS NULL OR correlation_id IS NULL;

-- Index for monitoring and observability (recent events by type)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_monitoring 
ON events (event_type, created_at DESC, stream_id) 
WHERE created_at > NOW() - INTERVAL '1 hour';

-- Comments for documentation
COMMENT ON INDEX idx_events_multistream_read IS 'Optimizes reading multiple streams simultaneously for multi-stream event sourcing';
COMMENT ON INDEX idx_events_projection_catchup IS 'Optimizes projection catchup by reading recent events by type';
COMMENT ON INDEX idx_events_saga_correlation IS 'Supports saga coordination by correlation ID across streams';
COMMENT ON INDEX idx_events_causation_chain IS 'Enables efficient causation chain analysis for debugging';
COMMENT ON INDEX idx_events_stream_version_covering IS 'Covering index to avoid table lookups for version checks';
COMMENT ON INDEX idx_events_temporal_replay IS 'Supports temporal event replay scenarios';
COMMENT ON INDEX idx_events_error_recovery IS 'Helps identify events with missing metadata for error recovery';
COMMENT ON INDEX idx_events_monitoring IS 'Supports real-time monitoring of recent events by type';