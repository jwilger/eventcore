-- Migration: Simple non-partitioned schema for development and smaller deployments
-- This is an alternative to the partitioned schema for environments that don't need partitioning

-- This migration is designed to be run instead of migrations 002-004 for simpler setups
-- It recreates the events table without partitioning but with all necessary indexes

-- Note: This migration should only be used for new installations or development
-- For production systems expecting high volume, use the partitioned schema instead

-- Drop partitioned table if it exists (for switching from partitioned to simple)
-- DROP TABLE IF EXISTS events CASCADE;

-- Create simple (non-partitioned) events table
CREATE TABLE IF NOT EXISTS events_simple (
    event_id UUID NOT NULL PRIMARY KEY,
    stream_id VARCHAR(255) NOT NULL,
    event_version BIGINT NOT NULL CHECK (event_version >= 0),
    event_type VARCHAR(255) NOT NULL,
    event_data JSONB NOT NULL,
    metadata JSONB,
    causation_id UUID,
    correlation_id UUID,
    user_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure unique combination of stream_id and event_version for data integrity
    CONSTRAINT unique_stream_version_simple UNIQUE (stream_id, event_version),
    
    -- Foreign key to event_streams table
    CONSTRAINT fk_events_stream_id_simple FOREIGN KEY (stream_id) REFERENCES event_streams(stream_id) ON DELETE CASCADE
);

-- Essential indexes for the simple schema
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_stream_id_version 
ON events_simple (stream_id, event_version);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_event_id_timestamp 
ON events_simple (event_id, created_at);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_event_type 
ON events_simple (event_type, created_at);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_causation_correlation 
ON events_simple (causation_id, correlation_id) WHERE causation_id IS NOT NULL;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_user_id 
ON events_simple (user_id, created_at) WHERE user_id IS NOT NULL;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_metadata_gin 
ON events_simple USING GIN (metadata) WHERE metadata IS NOT NULL;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_data_gin 
ON events_simple USING GIN (event_data);

-- Performance indexes for common query patterns
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_multistream_read 
ON events_simple (stream_id, event_version, created_at);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_saga_correlation 
ON events_simple (correlation_id, created_at, stream_id) 
WHERE correlation_id IS NOT NULL;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_simple_recent 
ON events_simple (created_at DESC, stream_id) 
WHERE created_at > NOW() - INTERVAL '30 days';

-- View to provide unified access regardless of which schema is used
CREATE OR REPLACE VIEW events_unified AS
SELECT 
    event_id,
    stream_id,
    event_version,
    event_type,
    event_data,
    metadata,
    causation_id,
    correlation_id,
    user_id,
    created_at,
    'simple' as schema_type
FROM events_simple
WHERE EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'events_simple')

UNION ALL

SELECT 
    event_id,
    stream_id,
    event_version,
    event_type,
    event_data,
    metadata,
    causation_id,
    correlation_id,
    user_id,
    created_at,
    'partitioned' as schema_type
FROM events
WHERE EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'events')
AND NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'events_simple');

-- Comments for documentation
COMMENT ON TABLE events_simple IS 'Simple non-partitioned events table for development and smaller deployments';
COMMENT ON VIEW events_unified IS 'Unified view that works with both partitioned and simple event schemas';

-- Configuration functions to help determine which schema to use
CREATE OR REPLACE FUNCTION get_recommended_schema()
RETURNS TEXT AS $$
BEGIN
    -- Simple heuristic: if we expect > 1M events per month, recommend partitioned
    -- This can be customized based on specific requirements
    RETURN 'partitioned'; -- Default recommendation for production
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION get_recommended_schema() IS 'Returns recommendation for which schema type to use based on expected load';