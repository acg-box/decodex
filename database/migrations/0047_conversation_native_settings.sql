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
