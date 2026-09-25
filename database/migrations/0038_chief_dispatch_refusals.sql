-- A resolved, exact refusal can cancel a claimed continuation without replay.
DROP TRIGGER chief_capacity_transition;
CREATE TRIGGER chief_capacity_transition
BEFORE UPDATE ON chief_capacity_retries
WHEN NOT (
    (OLD.state = 'pending' AND NEW.state IN ('claimed', 'cancelled'))
    OR (OLD.state = 'claimed' AND NEW.state = 'submitted')
    OR (OLD.state = 'claimed' AND NEW.state = 'cancelled' AND EXISTS (
        SELECT 1 FROM chief_inbox_events e
        WHERE e.work_item_id = OLD.work_item_id
          AND e.event_kind = 'capacity_retry_rejected'
          AND e.disposition = 'resolved'
          AND json_extract(e.payload, '$.retryEventId') = OLD.event_id
          AND json_extract(e.payload, '$.reason') IN (
              'serverDraining', 'managedProviderChanged', 'settingsChanged',
              'requestTooLarge', 'requestQueueFull'
          )
    ))
)
BEGIN
    SELECT RAISE(ABORT, 'capacity retry cannot be replayed');
END;
