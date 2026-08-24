ALTER TABLE command_receipts ADD COLUMN request_json TEXT;

CREATE INDEX pending_account_route_commands
  ON command_receipts(operation, state, claim_expires_at_micros)
  WHERE operation = 'route_account' AND state = 'reserved';

CREATE TRIGGER route_command_request_required_insert
BEFORE INSERT ON command_receipts
WHEN NEW.operation = 'route_account' AND NEW.request_json IS NULL
BEGIN
  SELECT RAISE(ABORT, 'route command request is required');
END;

CREATE TRIGGER route_command_request_required_update
BEFORE UPDATE OF operation, request_json ON command_receipts
WHEN NEW.operation = 'route_account' AND NEW.request_json IS NULL
BEGIN
  SELECT RAISE(ABORT, 'route command request is required');
END;
