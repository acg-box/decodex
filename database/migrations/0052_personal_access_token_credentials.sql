-- Extend credential schemas without changing existing account, process, or payload values.
-- The migration owner disables foreign keys before its transaction, checks them before
-- commit, and restores enforcement before it returns.

CREATE TABLE account_credentials_pat (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  schema_version INTEGER NOT NULL CHECK (schema_version IN (1, 2)),
  credential_version INTEGER NOT NULL CHECK (credential_version > 0),
  fingerprint TEXT NOT NULL CHECK (
    length(fingerprint) = 64 AND
    fingerprint = lower(fingerprint) AND
    fingerprint NOT GLOB '*[^0-9a-f]*'
  ),
  writer_operation_id TEXT NOT NULL REFERENCES account_operations(operation_id),
  provider TEXT NOT NULL CHECK (provider = 'chatgpt'),
  provider_account_id TEXT NOT NULL UNIQUE CHECK (
    length(CAST(provider_account_id AS BLOB)) BETWEEN 1 AND 512
  ),
  payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 1048576),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
) STRICT;
INSERT INTO account_credentials_pat SELECT * FROM account_credentials ORDER BY rowid;
DROP TABLE account_credentials;
ALTER TABLE account_credentials_pat RENAME TO account_credentials;
DROP TRIGGER agent_process_binding_authority;
CREATE TABLE process_generations_pat (
  generation_id TEXT PRIMARY KEY CHECK (length(generation_id) = 36),
  account_id TEXT NOT NULL REFERENCES accounts(account_id),
  runtime_session_id TEXT REFERENCES runtime_sessions(runtime_session_id),
  execution_epoch_id TEXT NOT NULL REFERENCES process_execution_epochs(execution_epoch_id),
  runner_identity TEXT NOT NULL CHECK (
    length(CAST(runner_identity AS BLOB)) BETWEEN 1 AND 128
  ),
  intended_boot_id TEXT NOT NULL CHECK (
    length(CAST(intended_boot_id AS BLOB)) BETWEEN 1 AND 256
  ),
  control_kind TEXT NOT NULL CHECK (
    control_kind IN ('stdio_only_best_effort_eof', 'parent_death_signal_and_stdio_eof')
  ),
  isolation_kind TEXT NOT NULL CHECK (isolation_kind = 'session'),
  bound_boot_id TEXT,
  process_id INTEGER CHECK (process_id > 0),
  process_start_id TEXT,
  process_group_id INTEGER CHECK (process_group_id > 0),
  session_id INTEGER CHECK (session_id > 0),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  credential_schema_version INTEGER NOT NULL CHECK (credential_schema_version IN (1, 2)),
  credential_version INTEGER NOT NULL CHECK (credential_version > 0),
  credential_fingerprint TEXT NOT NULL CHECK (length(credential_fingerprint) = 64),
  credential_writer_operation_id TEXT NOT NULL CHECK (
    length(credential_writer_operation_id) = 36
  ) REFERENCES account_operations(operation_id),
  provider TEXT NOT NULL CHECK (provider = 'chatgpt'),
  provider_account_id TEXT NOT NULL CHECK (
    length(CAST(provider_account_id AS BLOB)) BETWEEN 1 AND 512
  ),
  refresh_callback_profile_sha256 TEXT NOT NULL CHECK (
    length(refresh_callback_profile_sha256) = 64
  ),
  quick_task_admission_key TEXT UNIQUE CHECK (
    length(CAST(quick_task_admission_key AS BLOB)) BETWEEN 1 AND 256
  ),
  state TEXT NOT NULL CHECK (
    state IN ('starting', 'ready', 'stopping', 'dead', 'death_unknown')
  ),
  authority_loss_reason TEXT,
  death_evidence_id TEXT,
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  CHECK (
    (bound_boot_id IS NULL AND process_id IS NULL AND process_start_id IS NULL AND
      process_group_id IS NULL AND session_id IS NULL) OR
    (bound_boot_id IS NOT NULL AND process_id IS NOT NULL AND process_start_id IS NOT NULL AND
      process_group_id = process_id AND session_id = process_id)
  ),
  CHECK (state NOT IN ('ready', 'stopping') OR process_id IS NOT NULL),
  CHECK ((state = 'death_unknown') = (authority_loss_reason IS NOT NULL)),
  CHECK ((state = 'dead') = (death_evidence_id IS NOT NULL))
) STRICT;
INSERT INTO process_generations_pat SELECT * FROM process_generations ORDER BY rowid;
DROP TABLE process_generations;
ALTER TABLE process_generations_pat RENAME TO process_generations;
CREATE UNIQUE INDEX one_quarantining_process_generation_per_account
  ON process_generations(account_id) WHERE state <> 'dead';
CREATE INDEX process_generation_by_session ON process_generations(runtime_session_id);

CREATE TRIGGER agent_process_binding_authority BEFORE INSERT ON agent_process_bindings
BEGIN
	SELECT RAISE(ABORT, 'Agent root must be a root goal') WHERE NOT EXISTS (
		SELECT 1 FROM agent_work_items WHERE id = NEW.root_id AND kind = 'goal' AND parent_goal_id IS NULL
	);
	SELECT RAISE(ABORT, 'Previous Agent process must be dead before account rotation') WHERE EXISTS (
        SELECT 1 FROM agent_process_bindings b JOIN process_generations g ON g.generation_id = b.generation_id
        WHERE b.root_id = NEW.root_id AND b.account_id <> NEW.account_id AND g.state <> 'dead'
    );
    SELECT RAISE(ABORT, 'Agent work must be idle before account rotation') WHERE
        coalesce((SELECT account_id <> NEW.account_id FROM agent_process_bindings
            WHERE root_id = NEW.root_id ORDER BY created_at_micros DESC, rowid DESC LIMIT 1), 0)
        AND EXISTS (
            WITH RECURSIVE family(id) AS (
                SELECT NEW.root_id
                UNION SELECT w.id FROM agent_work_items w JOIN family f ON w.parent_goal_id = f.id
            )
            SELECT 1 FROM agent_work_items WHERE id IN (SELECT id FROM family) AND dispatch_state <> 'idle'
        );
	SELECT RAISE(ABORT, 'Agent generation ownership differs') WHERE NOT EXISTS (
		SELECT 1 FROM process_generations WHERE generation_id = NEW.generation_id
			AND account_id = NEW.account_id AND runtime_session_id IS NULL AND quick_task_admission_key IS NULL
	);
END;
