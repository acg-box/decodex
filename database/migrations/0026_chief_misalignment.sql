CREATE TABLE chief_misalignment (
    work_id TEXT PRIMARY KEY REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    details_json TEXT CHECK(details_json IS NULL OR json_valid(details_json)),
    created_at_micros INTEGER NOT NULL
) STRICT;
