-- Input can arrive before parent activity. Retain it independently so a restart
-- can reconcile both native records without replaying an instruction or spawn.
CREATE TABLE native_work_deliveries (
 contribution_id TEXT PRIMARY KEY REFERENCES native_work_contributions(id),
 parent_thread_id TEXT NOT NULL,
 parent_turn_id TEXT NOT NULL,
 child_thread_id TEXT NOT NULL,
 child_turn_id TEXT NOT NULL,
 message_id TEXT NOT NULL,
 UNIQUE(child_thread_id,child_turn_id,message_id)
) STRICT;
