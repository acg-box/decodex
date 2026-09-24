-- Keep original request identity and content while allowing native reasoning inheritance.
CREATE TABLE quick_task_requests_next (
  conversation_id TEXT PRIMARY KEY REFERENCES conversations(conversation_id),
  operation_key TEXT NOT NULL UNIQUE CHECK (length(CAST(operation_key AS BLOB)) BETWEEN 1 AND 256),
  correlation_id TEXT NOT NULL CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 256),
  causation_id TEXT CHECK (length(CAST(causation_id AS BLOB)) BETWEEN 1 AND 256),
  initial_turn_id TEXT NOT NULL UNIQUE CHECK (length(initial_turn_id) = 36),
  message TEXT NOT NULL CHECK (length(CAST(message AS BLOB)) BETWEEN 1 AND 1048576),
  working_directory TEXT NOT NULL CHECK (length(CAST(working_directory AS BLOB)) BETWEEN 1 AND 4096),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  model TEXT NOT NULL DEFAULT 'gpt-5.6-sol' CHECK (length(CAST(model AS BLOB)) BETWEEN 1 AND 128),
  reasoning_effort TEXT CHECK (reasoning_effort IS NULL OR length(CAST(reasoning_effort AS BLOB)) BETWEEN 1 AND 128),
  fast INTEGER NOT NULL DEFAULT 0 CHECK (fast IN (0, 1)),
  service_tier TEXT CHECK (service_tier IS NULL OR (
    length(CAST(service_tier AS BLOB)) BETWEEN 1 AND 64
    AND service_tier NOT GLOB '*[^a-zA-Z0-9_.-]*'
  ))
) STRICT;
INSERT INTO quick_task_requests_next
SELECT conversation_id, operation_key, correlation_id, causation_id, initial_turn_id,
       message, working_directory, created_at_micros, model, reasoning_effort, fast, service_tier
FROM quick_task_requests;
DROP TABLE quick_task_requests;
ALTER TABLE quick_task_requests_next RENAME TO quick_task_requests;
