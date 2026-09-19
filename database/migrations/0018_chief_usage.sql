CREATE TABLE chief_usage (
    thread_id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES chief_work_items(id),
    turn_id TEXT NOT NULL,
    usage_json TEXT NOT NULL CHECK(length(usage_json) <= 1024)
) STRICT;
