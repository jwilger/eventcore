LOCK TABLE eventcore_events IN ACCESS EXCLUSIVE MODE;

CREATE TABLE IF NOT EXISTS eventcore_projection_delivery_frontier (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    last_position BIGINT NOT NULL CHECK (last_position >= 0)
);

INSERT INTO eventcore_projection_delivery_frontier (singleton, last_position)
VALUES (TRUE, 0)
ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS eventcore_projection_delivery (
    delivery_position BIGINT PRIMARY KEY CHECK (delivery_position > 0),
    event_id UUID NOT NULL UNIQUE REFERENCES eventcore_events (event_id)
);

DO $$
DECLARE
    current_frontier BIGINT;
    backfilled_count BIGINT;
BEGIN
    SELECT last_position
      INTO current_frontier
      FROM eventcore_projection_delivery_frontier
     WHERE singleton = TRUE
     FOR UPDATE;

    WITH unmapped_events AS (
        SELECT
            events.event_id,
            row_number() OVER (
                ORDER BY events.stream_id, events.stream_version, events.event_id
            ) AS ordinal
        FROM eventcore_events AS events
        LEFT JOIN eventcore_projection_delivery AS delivery
          ON delivery.event_id = events.event_id
        WHERE delivery.event_id IS NULL
    )
    INSERT INTO eventcore_projection_delivery (delivery_position, event_id)
    SELECT current_frontier + ordinal, event_id
      FROM unmapped_events
     ORDER BY ordinal;

    GET DIAGNOSTICS backfilled_count = ROW_COUNT;

    UPDATE eventcore_projection_delivery_frontier
       SET last_position = current_frontier + backfilled_count
     WHERE singleton = TRUE;
END;
$$;

CREATE OR REPLACE FUNCTION eventcore_projection_assign_delivery_positions()
RETURNS TRIGGER AS $$
DECLARE
    current_frontier BIGINT;
    inserted_count BIGINT;
BEGIN
    SELECT last_position
      INTO current_frontier
      FROM eventcore_projection_delivery_frontier
     WHERE singleton = TRUE
     FOR UPDATE;

    WITH ordered_events AS (
        SELECT
            event_id,
            row_number() OVER (
                ORDER BY stream_id, stream_version, event_id
            ) AS ordinal
        FROM inserted_events
    )
    INSERT INTO eventcore_projection_delivery (delivery_position, event_id)
    SELECT current_frontier + ordinal, event_id
      FROM ordered_events
     ORDER BY ordinal;

    GET DIAGNOSTICS inserted_count = ROW_COUNT;

    UPDATE eventcore_projection_delivery_frontier
       SET last_position = current_frontier + inserted_count
     WHERE singleton = TRUE;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS eventcore_projection_delivery_trigger ON eventcore_events;

CREATE TRIGGER eventcore_projection_delivery_trigger
AFTER INSERT ON eventcore_events
REFERENCING NEW TABLE AS inserted_events
FOR EACH STATEMENT
EXECUTE FUNCTION eventcore_projection_assign_delivery_positions();
