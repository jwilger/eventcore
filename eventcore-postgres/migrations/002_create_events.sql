-- Migration: Create events table
-- This table stores the actual events with optimal indexing for event sourcing patterns

CREATE TABLE IF NOT EXISTS events (
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
    CONSTRAINT unique_stream_version UNIQUE (stream_id, event_version),
    
    -- Foreign key to event_streams table
    CONSTRAINT fk_events_stream_id FOREIGN KEY (stream_id) REFERENCES event_streams(stream_id) ON DELETE CASCADE
);

-- Primary index for reading events by stream (most common query pattern)
CREATE INDEX IF NOT EXISTS idx_events_stream_id_version 
ON events (stream_id, event_version);

-- Index for global event ordering using UUIDv7 chronological ordering
CREATE INDEX IF NOT EXISTS idx_events_event_id_timestamp 
ON events (event_id, created_at);

-- Index for querying events by type (useful for projections)
CREATE INDEX IF NOT EXISTS idx_events_event_type 
ON events (event_type, created_at);

-- Index for causation and correlation tracking (useful for debugging and sagas)
CREATE INDEX IF NOT EXISTS idx_events_causation_correlation 
ON events (causation_id, correlation_id) WHERE causation_id IS NOT NULL;

-- Index for user-based event queries
CREATE INDEX IF NOT EXISTS idx_events_user_id 
ON events (user_id, created_at) WHERE user_id IS NOT NULL;

-- Index for recent events (optimization for hot data)
CREATE INDEX IF NOT EXISTS idx_events_recent 
ON events (created_at DESC, stream_id);

-- GIN index for JSONB metadata queries
CREATE INDEX IF NOT EXISTS idx_events_metadata_gin 
ON events USING GIN (metadata) WHERE metadata IS NOT NULL;

-- GIN index for JSONB event_data queries (useful for projections that query event content)
CREATE INDEX IF NOT EXISTS idx_events_data_gin 
ON events USING GIN (event_data);

-- Comments for documentation
COMMENT ON TABLE events IS 'Stores all events in the event store with optimized indexing for event sourcing patterns';
COMMENT ON COLUMN events.event_id IS 'UUIDv7 identifier providing global chronological ordering';
COMMENT ON COLUMN events.stream_id IS 'Identifier of the stream this event belongs to';
COMMENT ON COLUMN events.event_version IS 'Version of the event within its stream (0-based)';
COMMENT ON COLUMN events.event_type IS 'Type name of the event for deserialization';
COMMENT ON COLUMN events.event_data IS 'Serialized event payload as JSONB';
COMMENT ON COLUMN events.metadata IS 'Optional metadata associated with the event';
COMMENT ON COLUMN events.causation_id IS 'ID of the command or event that caused this event';
COMMENT ON COLUMN events.correlation_id IS 'ID for correlating related events across boundaries';
COMMENT ON COLUMN events.user_id IS 'Identifier of the user who triggered this event';
COMMENT ON COLUMN events.created_at IS 'Timestamp when the event was stored';