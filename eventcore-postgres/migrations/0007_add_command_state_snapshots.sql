CREATE TABLE IF NOT EXISTS eventcore_command_state_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    state JSONB NOT NULL,
    stream_versions JSONB NOT NULL,
    replay_checkpoints JSONB NOT NULL DEFAULT '[]'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(stream_versions) = 'object'),
    CHECK (jsonb_typeof(replay_checkpoints) = 'array')
);
