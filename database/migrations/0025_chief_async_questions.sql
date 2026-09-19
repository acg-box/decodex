CREATE TABLE chief_async_questions (
    work_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    question_json TEXT NOT NULL CHECK(json_valid(question_json)),
    created_at_micros INTEGER NOT NULL,
    PRIMARY KEY(work_id, thread_id, question_id)
) STRICT;
CREATE TABLE chief_async_answers (
    work_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    created_at_micros INTEGER NOT NULL,
    PRIMARY KEY(work_id, thread_id, question_id)
) STRICT;
-- Existing threads need native history, including replies from other clients.
CREATE TABLE chief_async_recovery (
    required_item_id TEXT,
    work_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    PRIMARY KEY(work_id, thread_id)
) STRICT;
INSERT INTO chief_async_recovery(work_id, thread_id)
SELECT id, codex_thread_id FROM chief_work_items WHERE codex_thread_id IS NOT NULL;
