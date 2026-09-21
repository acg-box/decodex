ALTER TABLE desktop_settings ADD COLUMN auto_activate_quota INTEGER NOT NULL DEFAULT 1
  CHECK (auto_activate_quota IN (0, 1));

CREATE TABLE account_quota_activation (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  observed_reset_at_micros INTEGER NOT NULL CHECK (observed_reset_at_micros > 0),
  next_due_at_micros INTEGER NOT NULL CHECK (next_due_at_micros > 0),
  attempted_at_micros INTEGER CHECK (attempted_at_micros > 0),
  outcome TEXT NOT NULL CHECK (outcome IN ('idle', 'unknown', 'completed', 'rejected'))
) STRICT;
