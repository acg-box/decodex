CREATE INDEX chief_inbox_work_history ON chief_inbox_events(work_item_id, id);
CREATE INDEX chief_inbox_work_kind ON chief_inbox_events(work_item_id, event_kind, id);
