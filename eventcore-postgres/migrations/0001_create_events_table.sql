CREATE TABLE IF NOT EXISTS eventcore_events (
    event_id UUID PRIMARY KEY,
    stream_id TEXT NOT NULL,
    stream_version BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    event_data JSONB NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS eventcore_events_stream_version_idx
    ON eventcore_events (stream_id, stream_version);

CREATE INDEX IF NOT EXISTS eventcore_events_stream_idx
    ON eventcore_events (stream_id);
