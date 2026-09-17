CREATE TABLE chief_capacity_retries (
    event_id INTEGER PRIMARY KEY REFERENCES chief_inbox_events(id),
    work_item_id TEXT NOT NULL REFERENCES chief_work_items(id),
    failed_turn_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt BETWEEN 1 AND 3),
    due_at_micros INTEGER NOT NULL CHECK (due_at_micros >= 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'claimed', 'submitted', 'cancelled')),
    retry_turn_id TEXT,
    UNIQUE (work_item_id, failed_turn_id),
    UNIQUE (work_item_id, retry_turn_id),
    CHECK (retry_turn_id IS NULL OR retry_turn_id != failed_turn_id),
    CHECK ((state = 'submitted' AND retry_turn_id IS NOT NULL)
        OR (state != 'submitted' AND retry_turn_id IS NULL))
) STRICT;

CREATE UNIQUE INDEX chief_capacity_pending_work ON chief_capacity_retries(work_item_id)
WHERE state = 'pending';

CREATE TRIGGER chief_capacity_identity_immutable
BEFORE UPDATE OF event_id, work_item_id, failed_turn_id, attempt, due_at_micros ON chief_capacity_retries
BEGIN
    SELECT RAISE(ABORT, 'capacity retry identity is immutable');
END;

CREATE TRIGGER chief_capacity_transition
BEFORE UPDATE ON chief_capacity_retries
WHEN NOT ((OLD.state = 'pending' AND NEW.state IN ('claimed', 'cancelled'))
    OR (OLD.state = 'claimed' AND NEW.state = 'submitted'))
BEGIN
    SELECT RAISE(ABORT, 'capacity retry cannot be replayed');
END;

CREATE TRIGGER chief_capacity_disposition_cancel
AFTER UPDATE OF disposition ON chief_inbox_events
WHEN NEW.disposition IS NOT NULL
BEGIN
    UPDATE chief_capacity_retries SET state = 'cancelled'
    WHERE event_id = NEW.id AND state = 'pending';
END;

CREATE TRIGGER chief_capacity_work_judgment_cancel
AFTER UPDATE OF status ON chief_work_items
WHEN NEW.status != 'open'
BEGIN
    UPDATE chief_inbox_events SET disposition = 'resolved',
        disposition_note = 'Capacity retry cancelled because the work decision changed.',
        disposed_at_micros = max(created_at_micros, NEW.updated_at_micros)
    WHERE disposition IS NULL AND id IN (
        SELECT event_id FROM chief_capacity_retries
        WHERE work_item_id = NEW.id AND state = 'pending'
    );
END;
CREATE INDEX chief_capacity_due ON chief_capacity_retries(due_at_micros)
WHERE state = 'pending';
