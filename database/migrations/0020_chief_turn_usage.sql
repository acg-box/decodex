ALTER TABLE chief_usage ADD COLUMN baseline_input_tokens INTEGER CHECK (baseline_input_tokens >= 0);
ALTER TABLE chief_usage ADD COLUMN baseline_output_tokens INTEGER CHECK (baseline_output_tokens >= 0);
ALTER TABLE chief_usage ADD COLUMN turn_input_tokens INTEGER CHECK (turn_input_tokens >= 0);
ALTER TABLE chief_usage ADD COLUMN turn_output_tokens INTEGER CHECK (turn_output_tokens >= 0);
