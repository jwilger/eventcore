-- Migration: Create event_streams table
-- This table tracks stream metadata and current versions for optimistic concurrency control

CREATE TABLE IF NOT EXISTS event_streams (
    stream_id VARCHAR(255) NOT NULL PRIMARY KEY,
    stream_version BIGINT NOT NULL DEFAULT 0 CHECK (stream_version >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for efficient version queries during optimistic concurrency checks
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_event_streams_version 
ON event_streams (stream_id, stream_version);

-- Index for timestamp-based queries
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_event_streams_updated_at 
ON event_streams (updated_at);

-- Trigger to automatically update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_event_streams_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER trigger_event_streams_updated_at
    BEFORE UPDATE ON event_streams
    FOR EACH ROW
    EXECUTE FUNCTION update_event_streams_updated_at();

-- Comments for documentation
COMMENT ON TABLE event_streams IS 'Tracks metadata and current version for each event stream';
COMMENT ON COLUMN event_streams.stream_id IS 'Unique identifier for the event stream (max 255 characters)';
COMMENT ON COLUMN event_streams.stream_version IS 'Current version of the stream, used for optimistic concurrency control';
COMMENT ON COLUMN event_streams.created_at IS 'Timestamp when the stream was first created';
COMMENT ON COLUMN event_streams.updated_at IS 'Timestamp when the stream was last updated';