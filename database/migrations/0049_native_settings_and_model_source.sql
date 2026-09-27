-- Retain the source of initial execution choices across routing and restart.
-- NULL pairs preserve requests created before account-bound discovery.
ALTER TABLE quick_task_requests ADD COLUMN model_source_account_id TEXT;
ALTER TABLE quick_task_requests ADD COLUMN model_source_account_revision INTEGER
CHECK (
  (model_source_account_id IS NULL AND model_source_account_revision IS NULL)
  OR (model_source_account_id IS NOT NULL AND model_source_account_revision IS NOT NULL
      AND model_source_account_revision > 0)
);

ALTER TABLE quick_task_requests ADD COLUMN model_source_review_required INTEGER NOT NULL DEFAULT 0
CHECK (model_source_review_required IN (0, 1));

-- Last observed native session settings. These do not replace execution intent.
CREATE TABLE conversation_native_settings (
  runtime_session_id TEXT PRIMARY KEY REFERENCES runtime_sessions(runtime_session_id),
  codex_thread_id TEXT NOT NULL CHECK (length(CAST(codex_thread_id AS BLOB)) BETWEEN 1 AND 512),
  process_generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
  process_generation_revision INTEGER NOT NULL CHECK (process_generation_revision > 0),
  account_id TEXT NOT NULL REFERENCES accounts(account_id),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  response_id INTEGER NOT NULL CHECK (response_id > 0),
  response_sha256 TEXT NOT NULL CHECK (length(response_sha256) = 64),
  settings_json TEXT NOT NULL CHECK (json_valid(settings_json) AND length(CAST(settings_json AS BLOB)) <= 32768),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0)
) STRICT;

ALTER TABLE agent_misalignment ADD COLUMN retired_voice INTEGER NOT NULL DEFAULT 0 CHECK(retired_voice IN (0,1));
-- Earlier records do not prove whether voice was retired. Require explicit acknowledgment.
UPDATE agent_misalignment SET retired_voice = 1;
