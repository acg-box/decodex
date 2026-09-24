-- Local dismissal is not evidence of a native user reply.
CREATE TABLE chief_async_skips (
    work_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    created_at_micros INTEGER NOT NULL,
    PRIMARY KEY(work_id, thread_id, question_id),
    FOREIGN KEY(work_id, thread_id, question_id)
      REFERENCES chief_async_questions(work_id, thread_id, question_id) ON DELETE CASCADE
) STRICT;
