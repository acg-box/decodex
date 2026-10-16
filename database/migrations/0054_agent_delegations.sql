-- Ownership does not imply a standing request to report every conversation turn.
CREATE TABLE agent_delegations (
    work_id TEXT PRIMARY KEY REFERENCES agent_work_items(id) ON DELETE CASCADE,
    instruction_event_id INTEGER NOT NULL REFERENCES agent_inbox_events(id)
) STRICT;

-- Preserve outstanding assignments, including work waiting for a user or another agent.
-- Accepted historical work stays complete until its owner explicitly assigns more work.
INSERT INTO agent_delegations(work_id, instruction_event_id)
SELECT work.id, max(event.id)
FROM agent_work_items AS work
JOIN agent_inbox_events AS event ON event.work_item_id = work.id
WHERE work.parent_goal_id IS NOT NULL
  AND work.status <> 'resolved'
  AND event.event_kind = 'work_instruction'
  AND NOT EXISTS (
      SELECT 1 FROM agent_inbox_events AS result
      WHERE result.work_item_id = work.id AND result.id > event.id
        AND result.event_kind = 'worker_turn_completed' AND result.disposition = 'resolved'
  )
GROUP BY work.id;
