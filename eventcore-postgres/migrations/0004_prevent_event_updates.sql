-- Prevent UPDATE operations on the event log to enforce immutability.
-- Events are append-only; once written, they must never change.

CREATE OR REPLACE FUNCTION eventcore_prevent_update()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'Events are immutable and cannot be updated'
        USING ERRCODE = 'P0002';
END;
$$ LANGUAGE plpgsql;

-- Create the trigger (drop first if exists for idempotency)
DROP TRIGGER IF EXISTS eventcore_prevent_update_trigger ON eventcore_events;

CREATE TRIGGER eventcore_prevent_update_trigger
    BEFORE UPDATE ON eventcore_events
    FOR EACH ROW
    EXECUTE FUNCTION eventcore_prevent_update();