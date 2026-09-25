-- A file approval contains two bounded native messages plus local routing metadata.
DROP TRIGGER chief_request_payload_immutable;
ALTER TABLE chief_request_payloads RENAME TO chief_request_payloads_v39;
CREATE TABLE chief_request_payloads (
    event_id INTEGER PRIMARY KEY REFERENCES chief_inbox_events(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK (json_valid(payload) AND length(CAST(payload AS BLOB)) <= 16842752)
) STRICT;
INSERT INTO chief_request_payloads SELECT event_id, payload FROM chief_request_payloads_v39;
DROP TABLE chief_request_payloads_v39;
CREATE TRIGGER chief_request_payload_immutable
BEFORE UPDATE ON chief_request_payloads
BEGIN
    SELECT RAISE(ABORT, 'request source is immutable');
END;
