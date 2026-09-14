-- Preserve facts and errors. Only new positive observations can mark absence.
CREATE TABLE account_quota_facts_optional (
  account_id TEXT NOT NULL REFERENCES account_identities(account_id),
  duration_minutes INTEGER NOT NULL CHECK (duration_minutes IN (300, 10080)),
  used_percent INTEGER CHECK (used_percent BETWEEN 0 AND 100),
  resets_at_micros INTEGER,
  error_code TEXT CHECK (error_code IN (
    'provider_unavailable', 'protocol_unavailable', 'account_mismatch', 'unsupported_window'
  )),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros >= 0),
  not_applicable INTEGER NOT NULL DEFAULT 0 CHECK (not_applicable IN (0, 1)),
  PRIMARY KEY (account_id, duration_minutes),
  CHECK (
    (not_applicable = 0 AND error_code IS NULL AND used_percent IS NOT NULL
      AND resets_at_micros IS NOT NULL AND resets_at_micros > observed_at_micros) OR
    (not_applicable = 0 AND error_code IS NOT NULL AND used_percent IS NULL
      AND resets_at_micros IS NULL) OR
    (not_applicable = 1 AND duration_minutes = 300 AND observed_at_micros > 0
      AND error_code IS NULL AND used_percent IS NULL AND resets_at_micros IS NULL)
  )
) STRICT;

INSERT INTO account_quota_facts_optional (
  account_id, duration_minutes, used_percent, resets_at_micros, error_code, observed_at_micros
)
SELECT account_id, duration_minutes, used_percent, resets_at_micros, error_code, observed_at_micros
FROM account_quota_facts;

DROP TABLE account_quota_facts;
ALTER TABLE account_quota_facts_optional RENAME TO account_quota_facts;
CREATE INDEX account_quota_by_account ON account_quota_facts(account_id, duration_minutes);
