-- A live call authorizes native voice input on one existing Chief thread.
-- Media, SDP, credentials and provisional captions are never stored here.
CREATE TABLE chief_voice_calls (
    session_id TEXT PRIMARY KEY NOT NULL CHECK(length(session_id) BETWEEN 1 AND 512),
    work_id TEXT NOT NULL REFERENCES chief_work_items(id),
    thread_id TEXT NOT NULL CHECK(length(thread_id) BETWEEN 1 AND 512),
    generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
    baseline_turn_id TEXT,
    created_at_micros INTEGER NOT NULL,
    closed_at_micros INTEGER,
    CHECK(closed_at_micros IS NULL OR closed_at_micros >= created_at_micros)
) STRICT;
CREATE UNIQUE INDEX chief_voice_one_open_call ON chief_voice_calls((1)) WHERE closed_at_micros IS NULL;
CREATE INDEX chief_voice_thread_generation ON chief_voice_calls(thread_id,generation_id);
CREATE TABLE chief_voice_observed_turns (
    generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    PRIMARY KEY(generation_id,thread_id,turn_id)
) STRICT;
