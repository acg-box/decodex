ALTER TABLE command_receipts ADD COLUMN progress_json TEXT;

CREATE UNIQUE INDEX one_pending_account_route
  ON command_receipts(operation)
  WHERE operation = 'route_account' AND state = 'reserved';

CREATE TRIGGER route_command_progress_shape_insert
BEFORE INSERT ON command_receipts
WHEN NEW.progress_json IS NOT NULL
 AND (NEW.operation <> 'route_account' OR NEW.state <> 'reserved')
BEGIN
  SELECT RAISE(ABORT, 'route command progress is invalid');
END;

CREATE TRIGGER route_command_progress_shape_update
BEFORE UPDATE OF operation, state, progress_json ON command_receipts
WHEN NEW.progress_json IS NOT NULL
 AND (NEW.operation <> 'route_account' OR NEW.state <> 'reserved')
BEGIN
  SELECT RAISE(ABORT, 'route command progress is invalid');
END;
