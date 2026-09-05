CREATE TABLE legacy_account_route_interruptions (
  idempotency_key TEXT PRIMARY KEY CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  terminal_reason TEXT NOT NULL CHECK (terminal_reason = 'interrupted_by_upgrade'),
  interrupted_at_micros INTEGER NOT NULL CHECK (interrupted_at_micros > 0)
) STRICT;

INSERT INTO legacy_account_route_interruptions (
  idempotency_key,
  request_sha256,
  terminal_reason,
  interrupted_at_micros
)
SELECT
  idempotency_key,
  request_sha256,
  'interrupted_by_upgrade',
  MAX(reserved_at_micros, 1)
FROM command_receipts
WHERE protocol = 'decodex/account-command/1'
  AND operation = 'route_account'
  AND state = 'reserved';

-- A legacy pending Route is audit-only after this migration. Removing its command receipt
-- clears request/progress/lease authority and makes provider refresh or auth projection replay
-- impossible while preserving all account, credential, and routing rows.
DELETE FROM command_receipts
WHERE protocol = 'decodex/account-command/1'
  AND operation = 'route_account'
  AND state = 'reserved';

DROP TRIGGER route_command_request_required_insert;
DROP TRIGGER route_command_request_required_update;
DROP TRIGGER route_command_progress_shape_insert;
DROP TRIGGER route_command_progress_shape_update;
DROP INDEX pending_account_route_commands;
DROP INDEX one_pending_account_route;
