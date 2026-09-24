CREATE TABLE account_usage_observations (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  ordinary_usage_allowed INTEGER CHECK (ordinary_usage_allowed IN (0, 1))
) STRICT;
