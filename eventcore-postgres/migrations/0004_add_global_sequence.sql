-- Add global_sequence column for cross-stream event ordering in subscriptions.
--
-- global_sequence provides a monotonically increasing sequence number across all streams,
-- enabling subscriptions to deliver events in the order they were committed globally.
-- This is essential for projections that need to see all events in commit order.

-- Create sequence for global ordering
CREATE SEQUENCE IF NOT EXISTS eventcore_global_sequence;

-- Add global_sequence column (nullable initially for existing rows)
ALTER TABLE eventcore_events
    ADD COLUMN IF NOT EXISTS global_sequence BIGINT;

-- Backfill existing rows based on created_at order
UPDATE eventcore_events
SET global_sequence = subquery.seq
FROM (
    SELECT event_id, ROW_NUMBER() OVER (ORDER BY created_at, event_id) AS seq
    FROM eventcore_events
    WHERE global_sequence IS NULL
) AS subquery
WHERE eventcore_events.event_id = subquery.event_id;

-- Set sequence to continue from max existing value (minimum 1 for empty tables)
SELECT setval('eventcore_global_sequence',
    GREATEST(COALESCE((SELECT MAX(global_sequence) FROM eventcore_events), 0), 1),
    COALESCE((SELECT MAX(global_sequence) FROM eventcore_events), 0) > 0);

-- Make column NOT NULL and set default to use sequence
ALTER TABLE eventcore_events
    ALTER COLUMN global_sequence SET NOT NULL,
    ALTER COLUMN global_sequence SET DEFAULT nextval('eventcore_global_sequence');

-- Create index for efficient subscription queries ordered by global_sequence
CREATE INDEX IF NOT EXISTS eventcore_events_global_sequence_idx
    ON eventcore_events (global_sequence);
