-- Native review observations are not inbox work and never wake a model.
CREATE TABLE chief_guardian_reviews (
    id INTEGER PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    review_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    generation_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('inProgress','approved','denied','timedOut','aborted')),
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),
    conflicted INTEGER NOT NULL DEFAULT 0 CHECK (conflicted IN (0,1)),
    created_at_micros INTEGER NOT NULL,
    updated_at_micros INTEGER NOT NULL,
    UNIQUE(work_id,thread_id,turn_id,review_id)
) STRICT;

-- Claim before sending. A missing reply stays pending and cannot be resent.
CREATE TABLE chief_guardian_approvals (
    id INTEGER PRIMARY KEY,
    review_row_id INTEGER NOT NULL REFERENCES chief_guardian_reviews(id) ON DELETE CASCADE,
    command_key TEXT NOT NULL UNIQUE,
    review_digest TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    generation_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending','submitted','rejected')),
    created_at_micros INTEGER NOT NULL,
    finished_at_micros INTEGER
) STRICT;
CREATE UNIQUE INDEX chief_guardian_one_submission
ON chief_guardian_approvals(review_row_id) WHERE state IN ('pending','submitted');
