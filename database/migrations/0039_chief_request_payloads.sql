-- Keep large native approval details outside bounded inbox and snapshot rows.
CREATE TABLE chief_request_payloads (
    event_id INTEGER PRIMARY KEY REFERENCES chief_inbox_events(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK (json_valid(payload) AND length(CAST(payload AS BLOB)) <= 8388608)
) STRICT;

CREATE TRIGGER chief_request_payload_immutable
BEFORE UPDATE ON chief_request_payloads
BEGIN
    SELECT RAISE(ABORT, 'request source is immutable');
END;
