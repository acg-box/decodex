-- Complete edited inputs are immutable data, not wake events or execution authority.
-- Queue entries reference this owner so large native input cannot starve inbox batches.
CREATE TABLE chief_prompt_inputs (
    id INTEGER PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL CHECK (length(thread_id) BETWEEN 1 AND 512),
    edit_receipt_id INTEGER NOT NULL REFERENCES chief_inbox_events(id),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    content TEXT NOT NULL CHECK (
        json_valid(content) AND json_type(content) = 'array'
        AND json_array_length(content) > 0
        AND length(CAST(content AS BLOB)) <= 8388608
    ),
    created_at_micros INTEGER NOT NULL,
    UNIQUE (edit_receipt_id, sha256)
) STRICT;

CREATE TRIGGER chief_prompt_input_immutable
BEFORE UPDATE ON chief_prompt_inputs
BEGIN
    SELECT RAISE(ABORT, 'prompt input is immutable');
END;

-- Durable acknowledgements must survive service restart.
CREATE TABLE chief_prompt_input_chunks (
    upload_id TEXT NOT NULL CHECK (length(upload_id) BETWEEN 1 AND 128),
    byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),
    work_item_id TEXT NOT NULL REFERENCES chief_work_items(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    edit_receipt_id INTEGER NOT NULL REFERENCES chief_inbox_events(id),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    total_bytes INTEGER NOT NULL CHECK (total_bytes BETWEEN 1 AND 8388608),
    fragment TEXT NOT NULL CHECK (length(CAST(fragment AS BLOB)) BETWEEN 1 AND 65536),
    PRIMARY KEY (upload_id, byte_offset)
) STRICT;

CREATE TRIGGER chief_prompt_chunk_immutable
BEFORE UPDATE ON chief_prompt_input_chunks
BEGIN
    SELECT RAISE(ABORT, 'prompt input chunk is immutable');
END;
