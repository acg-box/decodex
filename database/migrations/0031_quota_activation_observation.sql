ALTER TABLE account_quota_activation ADD COLUMN observed_at_micros INTEGER
  CHECK (observed_at_micros > 0);
