-- Reusable display slots. Persistent work and native thread identities never change.
CREATE TABLE agent_slots (
    agent_key TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES agent_work_items(id) ON DELETE CASCADE,
    thread_id TEXT,
    slot INTEGER NOT NULL UNIQUE CHECK(slot >= 0)
) STRICT;

-- Positive native closure observations outlive their slots. A new native turn
-- clears the observation; an in-flight catalog page cannot reopen an agent.
CREATE TABLE agent_closed_native_threads (
    thread_id TEXT PRIMARY KEY
) STRICT;
