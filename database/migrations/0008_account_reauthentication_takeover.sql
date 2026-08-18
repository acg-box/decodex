ALTER TABLE account_operations
  ADD COLUMN recovery_operation_id TEXT
    REFERENCES account_operations(operation_id)
    DEFERRABLE INITIALLY DEFERRED
    CHECK (recovery_operation_id IS NULL OR recovery_operation_id <> operation_id);

ALTER TABLE account_operations
  ADD COLUMN superseded_by_operation_id TEXT
    REFERENCES account_operations(operation_id)
    DEFERRABLE INITIALLY DEFERRED
    CHECK (
      superseded_by_operation_id IS NULL OR
      (superseded_by_operation_id <> operation_id AND recovery_operation_id IS NULL)
    );

DROP INDEX one_unsettled_account_operation;

CREATE UNIQUE INDEX one_unsettled_account_operation
  ON account_operations(account_id)
  WHERE phase NOT IN ('committed', 'cancelled')
    AND recovery_operation_id IS NULL
    AND superseded_by_operation_id IS NULL;

CREATE UNIQUE INDEX one_active_account_reauthentication_takeover
  ON account_operations(recovery_operation_id)
  WHERE recovery_operation_id IS NOT NULL
    AND phase NOT IN ('committed', 'cancelled');

CREATE UNIQUE INDEX one_account_operation_supersession
  ON account_operations(superseded_by_operation_id)
  WHERE superseded_by_operation_id IS NOT NULL;
