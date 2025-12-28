-- Prevent DELETE operations on the event log to enforce immutability.
-- Events are append-only; once written, they must never be removed.

CREATE OR REPLACE FUNCTION eventcore_prevent_delete()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'Events are immutable and cannot be deleted'
        USING ERRCODE = 'P0002';
END;
$$ LANGUAGE plpgsql;

-- Create the trigger (drop first if exists for idempotency)
DROP TRIGGER IF EXISTS eventcore_prevent_delete_trigger ON eventcore_events;

CREATE TRIGGER eventcore_prevent_delete_trigger
    BEFORE DELETE ON eventcore_events
    FOR EACH ROW
    EXECUTE FUNCTION eventcore_prevent_delete();
