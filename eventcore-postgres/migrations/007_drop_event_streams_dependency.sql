-- Migration: Remove dependency on event_streams table
-- Since we now derive stream versions directly from the events table,
-- we no longer need the event_streams table or its foreign key constraint

-- Drop the foreign key constraint if it exists
ALTER TABLE events DROP CONSTRAINT IF EXISTS fk_events_stream_id;

-- Drop the event_streams table if it exists (optional - we can keep it for backward compatibility)
-- DROP TABLE IF EXISTS event_streams;