-- Migration: Partitioning strategy for large-scale event storage
-- Implements time-based partitioning for the events table to improve performance at scale

-- First, we need to convert the existing events table to a partitioned table
-- This migration should be run when the table is empty or during maintenance windows

-- Drop existing events table and recreate as partitioned (for new installations)
-- For existing data, this would require a more complex migration strategy

DROP TABLE IF EXISTS events CASCADE;

-- Recreate events table as partitioned by created_at (monthly partitions)
CREATE TABLE events (
    event_id UUID NOT NULL,
    stream_id VARCHAR(255) NOT NULL,
    event_version BIGINT NOT NULL CHECK (event_version >= 0),
    event_type VARCHAR(255) NOT NULL,
    event_data JSONB NOT NULL,
    metadata JSONB,
    causation_id UUID,
    correlation_id UUID,
    user_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure unique combination of stream_id and event_version for data integrity
    CONSTRAINT unique_stream_version UNIQUE (stream_id, event_version),
    
    -- Foreign key to event_streams table
    CONSTRAINT fk_events_stream_id FOREIGN KEY (stream_id) REFERENCES event_streams(stream_id) ON DELETE CASCADE,
    
    -- Primary key must include partition key (created_at)
    PRIMARY KEY (event_id, created_at)
) PARTITION BY RANGE (created_at);

-- Create initial partitions for the current and next few months
DO $$
DECLARE
    start_date DATE;
    end_date DATE;
    partition_name TEXT;
    i INTEGER;
BEGIN
    -- Start from the beginning of current month
    start_date := DATE_TRUNC('month', CURRENT_DATE);
    
    -- Create partitions for current month plus next 12 months
    FOR i IN 0..12 LOOP
        end_date := start_date + INTERVAL '1 month';
        partition_name := 'events_' || TO_CHAR(start_date, 'YYYY_MM');
        
        EXECUTE FORMAT('
            CREATE TABLE %I PARTITION OF events
            FOR VALUES FROM (%L) TO (%L)',
            partition_name, start_date, end_date);
            
        start_date := end_date;
    END LOOP;
END $$;

-- Recreate indexes on the partitioned table
CREATE INDEX idx_events_stream_id_version 
ON events (stream_id, event_version);

CREATE INDEX idx_events_event_id_timestamp 
ON events (event_id, created_at);

CREATE INDEX idx_events_event_type 
ON events (event_type, created_at);

CREATE INDEX idx_events_causation_correlation 
ON events (causation_id, correlation_id) WHERE causation_id IS NOT NULL;

CREATE INDEX idx_events_user_id 
ON events (user_id, created_at) WHERE user_id IS NOT NULL;

CREATE INDEX idx_events_metadata_gin 
ON events USING GIN (metadata) WHERE metadata IS NOT NULL;

CREATE INDEX idx_events_data_gin 
ON events USING GIN (event_data);

CREATE INDEX idx_events_multistream_read 
ON events (stream_id, event_version, created_at);

CREATE INDEX idx_events_saga_correlation 
ON events (correlation_id, created_at, stream_id) 
WHERE correlation_id IS NOT NULL;

CREATE INDEX idx_events_causation_chain 
ON events (causation_id, created_at) 
WHERE causation_id IS NOT NULL;

-- Function to automatically create new partitions
CREATE OR REPLACE FUNCTION create_monthly_partition(target_date DATE)
RETURNS TEXT AS $$
DECLARE
    start_date DATE;
    end_date DATE;
    partition_name TEXT;
BEGIN
    start_date := DATE_TRUNC('month', target_date);
    end_date := start_date + INTERVAL '1 month';
    partition_name := 'events_' || TO_CHAR(start_date, 'YYYY_MM');
    
    -- Check if partition already exists
    IF NOT EXISTS (
        SELECT 1 FROM pg_class 
        WHERE relname = partition_name
    ) THEN
        EXECUTE FORMAT('
            CREATE TABLE %I PARTITION OF events
            FOR VALUES FROM (%L) TO (%L)',
            partition_name, start_date, end_date);
            
        RETURN 'Created partition: ' || partition_name;
    ELSE
        RETURN 'Partition already exists: ' || partition_name;
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Function to drop old partitions (for data retention)
CREATE OR REPLACE FUNCTION drop_old_partitions(retention_months INTEGER DEFAULT 24)
RETURNS TEXT AS $$
DECLARE
    cutoff_date DATE;
    partition_name TEXT;
    dropped_count INTEGER := 0;
BEGIN
    cutoff_date := DATE_TRUNC('month', CURRENT_DATE) - (retention_months || ' months')::INTERVAL;
    
    FOR partition_name IN
        SELECT schemaname||'.'||tablename
        FROM pg_tables 
        WHERE tablename LIKE 'events_____' 
        AND tablename < 'events_' || TO_CHAR(cutoff_date, 'YYYY_MM')
    LOOP
        EXECUTE 'DROP TABLE ' || partition_name;
        dropped_count := dropped_count + 1;
    END LOOP;
    
    RETURN 'Dropped ' || dropped_count || ' old partitions';
END;
$$ LANGUAGE plpgsql;

-- Automatic partition creation trigger
CREATE OR REPLACE FUNCTION auto_create_partition()
RETURNS TRIGGER AS $$
DECLARE
    partition_result TEXT;
BEGIN
    -- Try to create partition for the target month
    SELECT create_monthly_partition(NEW.created_at) INTO partition_result;
    
    -- If we're near month end, also create next month's partition
    IF EXTRACT(DAY FROM NEW.created_at) > 25 THEN
        SELECT create_monthly_partition(NEW.created_at + INTERVAL '1 month') INTO partition_result;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Note: Trigger is commented out as it can impact performance
-- Consider using a cron job instead for partition management
-- CREATE TRIGGER trigger_auto_create_partition
--     BEFORE INSERT ON events
--     FOR EACH ROW
--     EXECUTE FUNCTION auto_create_partition();

-- Comments for documentation
COMMENT ON TABLE events IS 'Partitioned events table using monthly range partitioning for improved performance at scale';
COMMENT ON FUNCTION create_monthly_partition(DATE) IS 'Creates a new monthly partition for the events table';
COMMENT ON FUNCTION drop_old_partitions(INTEGER) IS 'Drops old partitions beyond the retention period (default 24 months)';
COMMENT ON FUNCTION auto_create_partition() IS 'Automatically creates partitions when new events are inserted';