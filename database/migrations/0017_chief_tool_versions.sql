CREATE TABLE chief_tool_versions (
 work_id TEXT PRIMARY KEY REFERENCES chief_work_items(id),
 version INTEGER NOT NULL CHECK(version > 0)
) STRICT;
CREATE TABLE chief_thread_revisions (
 work_id TEXT NOT NULL REFERENCES chief_work_items(id),
 old_thread_id TEXT NOT NULL,
 new_thread_id TEXT NOT NULL UNIQUE,
 created_at_micros INTEGER NOT NULL,
 PRIMARY KEY(work_id,old_thread_id)
) STRICT;
