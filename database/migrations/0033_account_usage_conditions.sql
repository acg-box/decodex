ALTER TABLE account_usage_observations ADD COLUMN has_credits INTEGER CHECK (has_credits IN (0,1));
ALTER TABLE account_usage_observations ADD COLUMN unlimited_credits INTEGER CHECK (unlimited_credits IN (0,1));
ALTER TABLE account_usage_observations ADD COLUMN spend_control_reached INTEGER CHECK (spend_control_reached IN (0,1));
ALTER TABLE account_usage_observations ADD COLUMN rate_limit_reached INTEGER CHECK (rate_limit_reached IN (0,1));
