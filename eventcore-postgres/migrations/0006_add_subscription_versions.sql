-- Checkpoint positions for event subscriptions/projectors per ADR-026
CREATE TABLE IF NOT EXISTS eventcore_subscription_versions (
    subscription_name TEXT PRIMARY KEY,
    last_position UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS eventcore_subscription_versions_updated_at_idx
    ON eventcore_subscription_versions (updated_at);

COMMENT ON TABLE eventcore_subscription_versions IS 'Tracks checkpoint positions for event subscriptions/projectors per ADR-026';
