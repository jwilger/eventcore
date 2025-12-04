-- Add trigger-based version management for gap-free sequential versions
-- and database-enforced optimistic concurrency control.
--
-- Usage: Before INSERT, set expected versions via session config:
--   SELECT set_config('eventcore.expected_versions', '{"stream_id": expected_version, ...}', true);
--
-- The trigger will:
-- 1. Validate that each stream's current max version matches the expected version
-- 2. Auto-assign stream_version as max + 1 (gap-free)
-- 3. Raise an exception if validation fails

CREATE OR REPLACE FUNCTION eventcore_assign_stream_version()
RETURNS TRIGGER AS $$
DECLARE
    current_max_version BIGINT;
    expected_versions JSONB;
    expected_version BIGINT;
BEGIN
    -- Get current max version for this stream
    -- Use a subquery to get max, avoiding FOR UPDATE with aggregate
    SELECT COALESCE(
        (SELECT stream_version FROM eventcore_events
         WHERE stream_id = NEW.stream_id
         ORDER BY stream_version DESC
         LIMIT 1
         FOR UPDATE),
        0
    ) INTO current_max_version;

    -- Check if expected versions are set in session config
    expected_versions := NULLIF(current_setting('eventcore.expected_versions', true), '')::JSONB;

    IF expected_versions IS NOT NULL THEN
        -- Get expected version for this stream
        expected_version := (expected_versions ->> NEW.stream_id)::BIGINT;

        IF expected_version IS NOT NULL AND expected_version != current_max_version THEN
            RAISE EXCEPTION 'version_conflict: stream "%" expected version %, actual %',
                NEW.stream_id, expected_version, current_max_version
                USING ERRCODE = 'P0001';  -- Custom error code for version conflict
        END IF;

        -- Update expected version for subsequent events in same stream
        -- This handles multiple events to the same stream in one INSERT
        expected_versions := jsonb_set(
            expected_versions,
            ARRAY[NEW.stream_id],
            to_jsonb(current_max_version + 1)
        );
        PERFORM set_config('eventcore.expected_versions', expected_versions::TEXT, true);
    END IF;

    -- Auto-assign the next sequential version
    NEW.stream_version := current_max_version + 1;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create the trigger (drop first if exists for idempotency)
DROP TRIGGER IF EXISTS eventcore_version_trigger ON eventcore_events;

CREATE TRIGGER eventcore_version_trigger
    BEFORE INSERT ON eventcore_events
    FOR EACH ROW
    EXECUTE FUNCTION eventcore_assign_stream_version();

-- Make stream_version optional (trigger will assign it)
-- We keep the column NOT NULL but allow the trigger to set it
ALTER TABLE eventcore_events
    ALTER COLUMN stream_version SET DEFAULT 0;
