-- Migration: Add trigger to enforce atomic version checking on event insertion
-- This ensures that events can only be inserted if they have the correct sequential version

-- Create a function that validates event versions before insertion
CREATE OR REPLACE FUNCTION check_event_version() RETURNS TRIGGER AS $$
DECLARE
    current_max_version BIGINT;
    expected_next_version BIGINT;
BEGIN
    -- Get the current maximum version for this stream
    SELECT COALESCE(MAX(event_version), -1) INTO current_max_version
    FROM events
    WHERE stream_id = NEW.stream_id;
    
    -- Calculate what the next version should be
    expected_next_version := current_max_version + 1;
    
    -- Check if the new event has the correct version
    IF NEW.event_version != expected_next_version THEN
        RAISE EXCEPTION 'Version conflict for stream %: expected version %, got %', 
            NEW.stream_id, expected_next_version, NEW.event_version
            USING ERRCODE = '40001'; -- Use serialization_failure error code
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create the trigger on the events table
DROP TRIGGER IF EXISTS enforce_event_version ON events;
CREATE TRIGGER enforce_event_version
    BEFORE INSERT ON events
    FOR EACH ROW
    EXECUTE FUNCTION check_event_version();

-- Add comment for documentation
COMMENT ON FUNCTION check_event_version() IS 'Enforces sequential event versioning within each stream to prevent concurrent write conflicts';