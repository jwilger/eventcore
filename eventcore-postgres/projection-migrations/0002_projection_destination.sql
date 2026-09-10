CREATE TABLE IF NOT EXISTS eventcore_projection_progress (
    projector_name TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    selection_id TEXT NOT NULL,
    last_position BIGINT NOT NULL CHECK (last_position > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
