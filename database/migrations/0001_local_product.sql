CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY CHECK (version > 0),
  name TEXT NOT NULL UNIQUE CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 128),
  sha256 TEXT NOT NULL CHECK (
    length(sha256) = 64 AND
    sha256 = lower(sha256) AND
    sha256 NOT GLOB '*[^0-9a-f]*'
  ),
  applied_at_micros INTEGER NOT NULL CHECK (applied_at_micros > 0)
) STRICT;

CREATE TABLE account_identities (
  account_id TEXT PRIMARY KEY CHECK (
    length(account_id) = 36 AND
    account_id = lower(account_id) AND
    account_id NOT GLOB '*[^0-9a-f-]*'
  ),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE account_operations (
  operation_id TEXT PRIMARY KEY CHECK (length(operation_id) = 36),
  account_id TEXT NOT NULL REFERENCES account_identities(account_id),
  kind TEXT NOT NULL CHECK (kind IN ('enroll', 'import', 'refresh', 'logout')),
  phase TEXT NOT NULL CHECK (
    phase IN (
      'prepared',
      'provider_effect_pending',
      'store_applied',
      'committed',
      'cancelled',
      'recovery_required'
    )
  ),
  expected_account_revision INTEGER CHECK (expected_account_revision > 0),
  expected_credential_json TEXT,
  target_credential_json TEXT,
  provider TEXT NOT NULL CHECK (provider = 'chatgpt'),
  provider_account_id TEXT NOT NULL CHECK (
    length(CAST(provider_account_id AS BLOB)) BETWEEN 1 AND 512
  ),
  requested_display_label TEXT CHECK (
    length(CAST(requested_display_label AS BLOB)) BETWEEN 1 AND 128
  ),
  requested_enabled INTEGER CHECK (requested_enabled IN (0, 1)),
  recovery_code TEXT CHECK (
    length(CAST(recovery_code AS BLOB)) BETWEEN 1 AND 128
  ),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  completed_at_micros INTEGER CHECK (completed_at_micros >= created_at_micros),
  CHECK (
    (kind IN ('enroll', 'import') AND requested_display_label IS NOT NULL AND requested_enabled IS NOT NULL) OR
    (kind IN ('refresh', 'logout') AND requested_display_label IS NULL AND requested_enabled IS NULL)
  ),
  CHECK ((phase = 'recovery_required') = (recovery_code IS NOT NULL)),
  CHECK (
    (phase IN ('committed', 'cancelled') AND completed_at_micros IS NOT NULL) OR
    (phase NOT IN ('committed', 'cancelled') AND completed_at_micros IS NULL)
  )
) STRICT;

CREATE UNIQUE INDEX one_unsettled_account_operation
  ON account_operations(account_id)
  WHERE phase NOT IN ('committed', 'cancelled');

CREATE TABLE accounts (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  display_label TEXT NOT NULL CHECK (
    length(CAST(display_label AS BLOB)) BETWEEN 1 AND 128
  ),
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  state TEXT NOT NULL CHECK (
    state IN ('unavailable', 'unknown', 'available', 'depleted', 'auth_failed', 'plugin_unready')
  ),
  revision INTEGER NOT NULL CHECK (revision > 0),
  provider TEXT NOT NULL CHECK (provider = 'chatgpt'),
  provider_account_id TEXT NOT NULL UNIQUE CHECK (
    length(CAST(provider_account_id AS BLOB)) BETWEEN 1 AND 512
  ),
  credential_store_observation TEXT NOT NULL DEFAULT 'unknown' CHECK (
    credential_store_observation IN (
      'unknown', 'exact', 'missing', 'unavailable', 'mismatch', 'provider_mismatch'
    )
  ),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  tombstoned_at_micros INTEGER CHECK (tombstoned_at_micros >= created_at_micros)
) STRICT;

CREATE TABLE account_credentials (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
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

CREATE TABLE local_account_transfers (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  source_sha256 TEXT NOT NULL CHECK (
    length(source_sha256) = 64 AND
    source_sha256 = lower(source_sha256) AND
    source_sha256 NOT GLOB '*[^0-9a-f]*'
  ),
  account_count INTEGER NOT NULL CHECK (account_count BETWEEN 1 AND 512),
  imported_at_micros INTEGER NOT NULL CHECK (imported_at_micros > 0)
) STRICT;

CREATE TABLE account_routing_control (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  mode TEXT NOT NULL CHECK (mode IN ('fixed', 'balanced')),
  fixed_account_id TEXT REFERENCES account_identities(account_id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0),
  CHECK ((mode = 'fixed') = (fixed_account_id IS NOT NULL))
) STRICT;

INSERT INTO account_routing_control (
  singleton,
  mode,
  fixed_account_id,
  revision,
  updated_at_micros
) VALUES (1, 'balanced', NULL, 1, 1);

CREATE TABLE account_routing_order (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  position INTEGER NOT NULL UNIQUE CHECK (position >= 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
) STRICT;

CREATE TABLE account_quota_facts (
  account_id TEXT NOT NULL REFERENCES account_identities(account_id),
  duration_minutes INTEGER NOT NULL CHECK (duration_minutes IN (300, 10080)),
  used_percent INTEGER CHECK (used_percent BETWEEN 0 AND 100),
  resets_at_micros INTEGER,
  error_code TEXT CHECK (
    error_code IN (
      'provider_unavailable',
      'protocol_unavailable',
      'account_mismatch',
      'unsupported_window'
    )
  ),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros >= 0),
  PRIMARY KEY (account_id, duration_minutes),
  CHECK (
    (error_code IS NULL AND used_percent IS NOT NULL AND resets_at_micros > observed_at_micros) OR
    (error_code IS NOT NULL AND used_percent IS NULL AND resets_at_micros IS NULL)
  )
) STRICT;

CREATE TABLE account_profile_snapshots (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  provider TEXT NOT NULL CHECK (provider = 'chatgpt'),
  provider_account_id TEXT NOT NULL CHECK (
    length(CAST(provider_account_id AS BLOB)) BETWEEN 1 AND 512
  ),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  display_name TEXT,
  username TEXT,
  lifetime_tokens INTEGER CHECK (lifetime_tokens >= 0),
  peak_daily_tokens INTEGER CHECK (peak_daily_tokens >= 0),
  longest_task_seconds INTEGER CHECK (longest_task_seconds >= 0),
  current_streak_days INTEGER CHECK (current_streak_days >= 0),
  longest_streak_days INTEGER CHECK (longest_streak_days >= 0),
  CHECK (
    display_name IS NOT NULL OR username IS NOT NULL OR lifetime_tokens IS NOT NULL OR
    peak_daily_tokens IS NOT NULL OR longest_task_seconds IS NOT NULL OR
    current_streak_days IS NOT NULL OR longest_streak_days IS NOT NULL
  )
) STRICT;

CREATE TABLE account_profile_daily_usage (
  account_id TEXT NOT NULL REFERENCES account_profile_snapshots(account_id) ON DELETE CASCADE,
  start_date TEXT NOT NULL CHECK (
    length(start_date) = 10 AND substr(start_date, 5, 1) = '-' AND substr(start_date, 8, 1) = '-'
  ),
  tokens INTEGER NOT NULL CHECK (tokens >= 0),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  PRIMARY KEY (account_id, start_date)
) STRICT;

CREATE TABLE codex_account_capability (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  build_identity TEXT NOT NULL CHECK (
    length(CAST(build_identity AS BLOB)) BETWEEN 1 AND 256
  ),
  executable_sha256 TEXT NOT NULL CHECK (length(executable_sha256) = 64),
  schema_sha256 TEXT NOT NULL CHECK (length(schema_sha256) = 64),
  callback_profile_sha256 TEXT NOT NULL CHECK (length(callback_profile_sha256) = 64),
  login_chatgpt_auth_tokens INTEGER NOT NULL CHECK (login_chatgpt_auth_tokens IN (0, 1)),
  refresh_callback INTEGER NOT NULL CHECK (refresh_callback IN (0, 1)),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0)
) STRICT;

CREATE TABLE command_receipts (
  protocol TEXT NOT NULL CHECK (length(CAST(protocol AS BLOB)) BETWEEN 1 AND 128),
  idempotency_key TEXT NOT NULL CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  operation TEXT NOT NULL CHECK (length(CAST(operation AS BLOB)) BETWEEN 1 AND 128),
  entity_id TEXT NOT NULL CHECK (length(CAST(entity_id AS BLOB)) BETWEEN 1 AND 256),
  expected_revision INTEGER CHECK (expected_revision > 0),
  state TEXT NOT NULL CHECK (state IN ('reserved', 'completed_success', 'completed_error')),
  response_json TEXT,
  claim_token TEXT CHECK (length(claim_token) = 36),
  claim_expires_at_micros INTEGER,
  reserved_at_micros INTEGER NOT NULL CHECK (reserved_at_micros > 0),
  completed_at_micros INTEGER CHECK (completed_at_micros >= reserved_at_micros),
  PRIMARY KEY (protocol, idempotency_key),
  CHECK ((state = 'reserved') = (response_json IS NULL)),
  CHECK ((state = 'reserved') = (completed_at_micros IS NULL)),
  CHECK (
    (state = 'reserved' AND claim_token IS NOT NULL AND claim_expires_at_micros > reserved_at_micros) OR
    (state <> 'reserved' AND claim_token IS NULL AND claim_expires_at_micros IS NULL)
  )
) STRICT;

CREATE TABLE role_profiles (
  role TEXT PRIMARY KEY CHECK (role = 'task'),
  revision INTEGER NOT NULL CHECK (revision > 0),
  model TEXT NOT NULL CHECK (length(CAST(model AS BLOB)) BETWEEN 1 AND 128),
  reasoning_effort TEXT NOT NULL CHECK (
    reasoning_effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh')
  ),
  service_tier TEXT NOT NULL DEFAULT 'default' CHECK (
    length(CAST(service_tier AS BLOB)) BETWEEN 1 AND 32
  ),
  instructions TEXT NOT NULL CHECK (length(CAST(instructions AS BLOB)) <= 65536),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
) STRICT;

INSERT INTO role_profiles (
  role,
  revision,
  model,
  reasoning_effort,
  instructions,
  updated_at_micros
) VALUES ('task', 1, 'gpt-5.4', 'high', '', 1);

CREATE TABLE conversations (
  conversation_id TEXT PRIMARY KEY CHECK (length(conversation_id) = 36),
  kind TEXT NOT NULL CHECK (kind = 'ordinary_task'),
  state TEXT NOT NULL CHECK (state IN ('active', 'completed', 'failed', 'archived')),
  title TEXT CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 512),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros)
) STRICT;

CREATE TABLE quick_task_requests (
  conversation_id TEXT PRIMARY KEY REFERENCES conversations(conversation_id),
  operation_key TEXT NOT NULL UNIQUE CHECK (
    length(CAST(operation_key AS BLOB)) BETWEEN 1 AND 256
  ),
  correlation_id TEXT NOT NULL CHECK (
    length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 256
  ),
  causation_id TEXT CHECK (length(CAST(causation_id AS BLOB)) BETWEEN 1 AND 256),
  initial_turn_id TEXT NOT NULL UNIQUE CHECK (length(initial_turn_id) = 36),
  message TEXT NOT NULL CHECK (length(CAST(message AS BLOB)) BETWEEN 1 AND 1048576),
  working_directory TEXT NOT NULL CHECK (
    length(CAST(working_directory AS BLOB)) BETWEEN 1 AND 4096
  ),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE routing_decisions (
  routing_decision_id TEXT PRIMARY KEY CHECK (length(routing_decision_id) = 36),
  operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) = 36),
  idempotency_key TEXT NOT NULL UNIQUE CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  authority_shape TEXT NOT NULL CHECK (
    authority_shape IN ('conversation_account_registry', 'conversation_continuation')
  ),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  turn_id TEXT NOT NULL CHECK (length(turn_id) = 36),
  conversation_revision INTEGER NOT NULL CHECK (conversation_revision > 0),
  source_runtime_session_id TEXT REFERENCES runtime_sessions(runtime_session_id)
    DEFERRABLE INITIALLY DEFERRED,
  source_runtime_session_revision INTEGER CHECK (source_runtime_session_revision > 0),
  account_snapshot_id TEXT CHECK (length(account_snapshot_id) = 36),
  profile_snapshot_id TEXT CHECK (length(profile_snapshot_id) = 36),
  snapshot_id TEXT CHECK (length(snapshot_id) = 36),
  snapshot_json TEXT,
  decision_kind TEXT NOT NULL CHECK (decision_kind IN ('selected', 'waiting', 'no_route')),
  account_id TEXT REFERENCES accounts(account_id),
  account_revision INTEGER CHECK (account_revision > 0),
  routing_revision INTEGER NOT NULL CHECK (routing_revision > 0),
  quota_classification TEXT NOT NULL CHECK (
    quota_classification IN ('known_available', 'unknown', 'known_depleted')
  ),
  causes_json TEXT NOT NULL,
  exclusions_json TEXT NOT NULL,
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  UNIQUE (conversation_id, turn_id),
  CHECK ((decision_kind = 'selected') = (account_id IS NOT NULL)),
  CHECK ((account_id IS NULL) = (account_revision IS NULL)),
  CHECK (
    (authority_shape = 'conversation_account_registry' AND
      source_runtime_session_id IS NULL AND source_runtime_session_revision IS NULL AND
      account_snapshot_id IS NULL AND profile_snapshot_id IS NULL AND
      snapshot_id IS NOT NULL AND snapshot_json IS NOT NULL) OR
    (authority_shape = 'conversation_continuation' AND
      source_runtime_session_id IS NOT NULL AND source_runtime_session_revision IS NOT NULL AND
      account_snapshot_id IS NOT NULL AND profile_snapshot_id IS NOT NULL AND
      snapshot_id IS NULL AND snapshot_json IS NULL AND decision_kind = 'selected')
  )
) STRICT;

CREATE UNIQUE INDEX one_initial_routing_decision_per_conversation
  ON routing_decisions(conversation_id)
  WHERE authority_shape = 'conversation_account_registry';

CREATE TABLE continuation_plans (
  continuation_plan_id TEXT PRIMARY KEY CHECK (length(continuation_plan_id) = 36),
  operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) = 36),
  idempotency_key TEXT NOT NULL UNIQUE CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  turn_id TEXT NOT NULL CHECK (length(turn_id) = 36),
  routing_decision_id TEXT NOT NULL REFERENCES routing_decisions(routing_decision_id),
  source_runtime_session_id TEXT NOT NULL REFERENCES runtime_sessions(runtime_session_id)
    DEFERRABLE INITIALLY DEFERRED,
  source_runtime_session_revision INTEGER NOT NULL CHECK (source_runtime_session_revision > 0),
  selected_account_id TEXT NOT NULL REFERENCES accounts(account_id),
  runtime_session_id TEXT REFERENCES runtime_sessions(runtime_session_id)
    DEFERRABLE INITIALLY DEFERRED,
  kind TEXT NOT NULL CHECK (kind IN ('initial_thread', 'same_thread', 'context_pack_fallback')),
  codex_thread_id TEXT,
  fallback_context_pack_id TEXT CHECK (length(fallback_context_pack_id) = 36),
  same_thread_attempt_id TEXT,
  same_thread_evidence_id TEXT,
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  UNIQUE (conversation_id, turn_id),
  CHECK (
    (kind = 'initial_thread' AND runtime_session_id = source_runtime_session_id AND
      codex_thread_id IS NULL AND fallback_context_pack_id IS NULL AND
      same_thread_attempt_id IS NULL AND same_thread_evidence_id IS NULL) OR
    (kind = 'same_thread' AND runtime_session_id IS NULL AND codex_thread_id IS NOT NULL AND
      fallback_context_pack_id IS NULL AND same_thread_attempt_id IS NOT NULL AND
      same_thread_evidence_id IS NOT NULL) OR
    (kind = 'context_pack_fallback' AND runtime_session_id IS NOT NULL AND
      fallback_context_pack_id IS NOT NULL AND codex_thread_id IS NULL AND
      same_thread_attempt_id IS NULL AND same_thread_evidence_id IS NULL)
  )
) STRICT;

CREATE TABLE runtime_sessions (
  runtime_session_id TEXT PRIMARY KEY CHECK (length(runtime_session_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  account_id TEXT NOT NULL REFERENCES accounts(account_id),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  account_snapshot_id TEXT NOT NULL UNIQUE CHECK (length(account_snapshot_id) = 36),
  account_display_label TEXT NOT NULL CHECK (
    length(CAST(account_display_label AS BLOB)) BETWEEN 1 AND 128
  ),
  account_observed_state TEXT NOT NULL CHECK (
    account_observed_state IN ('unavailable', 'unknown', 'available', 'depleted', 'auth_failed', 'plugin_unready')
  ),
  credential_binding_json TEXT NOT NULL,
  profile_snapshot_id TEXT NOT NULL UNIQUE CHECK (length(profile_snapshot_id) = 36),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_role TEXT NOT NULL CHECK (profile_role = 'task'),
  model TEXT NOT NULL CHECK (length(CAST(model AS BLOB)) BETWEEN 1 AND 128),
  reasoning_effort TEXT NOT NULL CHECK (
    reasoning_effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh')
  ),
  instructions TEXT NOT NULL CHECK (length(CAST(instructions AS BLOB)) <= 65536),
  service_tier TEXT NOT NULL CHECK (length(CAST(service_tier AS BLOB)) BETWEEN 1 AND 32),
  instructions_sha256 TEXT NOT NULL CHECK (length(instructions_sha256) = 64),
  profile_provenance TEXT,
  codex_thread_id TEXT CHECK (length(CAST(codex_thread_id AS BLOB)) BETWEEN 1 AND 512),
  state TEXT NOT NULL CHECK (state IN ('starting', 'active', 'ended', 'diverged')),
  last_known_turn_id TEXT CHECK (
    length(CAST(last_known_turn_id AS BLOB)) BETWEEN 1 AND 256
  ),
  thread_start_request_id INTEGER CHECK (thread_start_request_id > 0),
  thread_start_request_sha256 TEXT CHECK (length(thread_start_request_sha256) = 64),
  thread_start_response_id INTEGER CHECK (thread_start_response_id > 0),
  thread_start_response_sha256 TEXT CHECK (length(thread_start_response_sha256) = 64),
  thread_start_fence_key TEXT UNIQUE CHECK (
    length(CAST(thread_start_fence_key AS BLOB)) BETWEEN 1 AND 256
  ),
  thread_start_binding_key TEXT UNIQUE CHECK (
    length(CAST(thread_start_binding_key AS BLOB)) BETWEEN 1 AND 256
  ),
  thread_start_turn_id TEXT CHECK (length(thread_start_turn_id) = 36),
  thread_start_continuation_plan_id TEXT CHECK (length(thread_start_continuation_plan_id) = 36),
  thread_start_routing_decision_id TEXT CHECK (length(thread_start_routing_decision_id) = 36),
  thread_start_process_generation_id TEXT CHECK (length(thread_start_process_generation_id) = 36),
  thread_start_process_generation_revision INTEGER CHECK (
    thread_start_process_generation_revision > 0
  ),
  thread_start_execution_epoch_id TEXT CHECK (length(thread_start_execution_epoch_id) = 36),
  has_acknowledged_turn INTEGER NOT NULL DEFAULT 0 CHECK (has_acknowledged_turn IN (0, 1)),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  ended_at_micros INTEGER CHECK (ended_at_micros >= created_at_micros),
  CHECK (
    (state = 'starting' AND codex_thread_id IS NULL AND thread_start_response_id IS NULL) OR
    (state = 'active' AND codex_thread_id IS NOT NULL AND thread_start_request_id IS NOT NULL AND
      thread_start_response_id = thread_start_request_id) OR
    state IN ('ended', 'diverged')
  ),
  CHECK (
    (thread_start_fence_key IS NULL AND thread_start_turn_id IS NULL AND
      thread_start_continuation_plan_id IS NULL AND thread_start_routing_decision_id IS NULL AND
      thread_start_process_generation_id IS NULL AND
      thread_start_process_generation_revision IS NULL AND
      thread_start_execution_epoch_id IS NULL) OR
    (thread_start_fence_key IS NOT NULL AND thread_start_turn_id IS NOT NULL AND
      thread_start_continuation_plan_id IS NOT NULL AND thread_start_routing_decision_id IS NOT NULL AND
      thread_start_process_generation_id IS NOT NULL AND
      thread_start_process_generation_revision IS NOT NULL AND
      thread_start_execution_epoch_id IS NOT NULL)
  )
) STRICT;

CREATE UNIQUE INDEX one_live_runtime_session_per_conversation
  ON runtime_sessions(conversation_id)
  WHERE state IN ('starting', 'active');

CREATE TABLE turns (
  turn_id TEXT PRIMARY KEY CHECK (length(turn_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  runtime_session_id TEXT REFERENCES runtime_sessions(runtime_session_id),
  sequence INTEGER NOT NULL CHECK (sequence > 0),
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  possible_side_effects TEXT NOT NULL CHECK (
    possible_side_effects IN ('none', 'possible', 'unknown')
  ),
  status TEXT NOT NULL CHECK (status IN ('active', 'completed', 'failed')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  completed_at_micros INTEGER CHECK (completed_at_micros >= created_at_micros),
  UNIQUE (conversation_id, sequence)
) STRICT;

CREATE TABLE history_items (
  history_item_id TEXT PRIMARY KEY CHECK (length(history_item_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  turn_id TEXT NOT NULL REFERENCES turns(turn_id),
  sequence INTEGER NOT NULL CHECK (sequence > 0),
  kind TEXT NOT NULL CHECK (
    kind IN ('message', 'reasoning', 'tool_call', 'tool_result', 'artifact', 'status')
  ),
  role TEXT CHECK (role IN ('user', 'assistant')),
  status TEXT NOT NULL CHECK (status IN ('streaming', 'completed', 'failed')),
  media_type TEXT NOT NULL CHECK (length(CAST(media_type AS BLOB)) BETWEEN 1 AND 128),
  inline_text TEXT,
  blob_sha256 TEXT CHECK (length(blob_sha256) = 64),
  metadata_json TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  CHECK ((inline_text IS NULL) <> (blob_sha256 IS NULL)),
  UNIQUE (conversation_id, sequence)
) STRICT;

CREATE TABLE runtime_command_receipts (
  idempotency_key TEXT PRIMARY KEY CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  operation TEXT NOT NULL CHECK (length(CAST(operation AS BLOB)) BETWEEN 1 AND 128),
  entity_id TEXT NOT NULL CHECK (length(CAST(entity_id AS BLOB)) BETWEEN 1 AND 256),
  response_json TEXT NOT NULL,
  completed_at_micros INTEGER NOT NULL CHECK (completed_at_micros > 0)
) STRICT;

CREATE TABLE conversation_routing_successors (
  source_conversation_id TEXT PRIMARY KEY REFERENCES conversations(conversation_id),
  successor_conversation_id TEXT NOT NULL UNIQUE REFERENCES conversations(conversation_id),
  source_routing_decision_id TEXT NOT NULL REFERENCES routing_decisions(routing_decision_id),
  idempotency_key TEXT NOT NULL UNIQUE CHECK (
    length(CAST(idempotency_key AS BLOB)) BETWEEN 1 AND 256
  ),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE process_execution_epochs (
  execution_epoch_id TEXT PRIMARY KEY CHECK (length(execution_epoch_id) = 36),
  authorization_sha256 TEXT NOT NULL CHECK (length(authorization_sha256) = 64),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE process_generations (
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
  credential_schema_version INTEGER NOT NULL CHECK (credential_schema_version = 1),
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

CREATE UNIQUE INDEX one_quarantining_process_generation_per_account
  ON process_generations(account_id)
  WHERE state <> 'dead';

CREATE TABLE process_generation_death_evidence (
  evidence_id TEXT PRIMARY KEY CHECK (length(evidence_id) = 36),
  generation_id TEXT NOT NULL UNIQUE REFERENCES process_generations(generation_id),
  kind TEXT NOT NULL CHECK (
    kind IN (
      'spawn_not_created',
      'owned_child_exit',
      'linux_pidfd_exit',
      'macos_kqueue_exit_and_group_quiescence',
      'exact_termination_exit',
      'prior_boot_ended'
    )
  ),
  observed_boot_id TEXT NOT NULL CHECK (
    length(CAST(observed_boot_id AS BLOB)) BETWEEN 1 AND 256
  ),
  bound_boot_id TEXT,
  process_id INTEGER CHECK (process_id > 0),
  process_start_id TEXT,
  process_group_id INTEGER CHECK (process_group_id > 0),
  session_id INTEGER CHECK (session_id > 0),
  witness_sha256 TEXT NOT NULL CHECK (length(witness_sha256) = 64),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  CHECK (
    (bound_boot_id IS NULL AND process_id IS NULL AND process_start_id IS NULL AND
      process_group_id IS NULL AND session_id IS NULL) OR
    (bound_boot_id IS NOT NULL AND process_id IS NOT NULL AND process_start_id IS NOT NULL AND
      process_group_id = process_id AND session_id = process_id)
  )
) STRICT;

CREATE TABLE provider_attempts (
  attempt_id TEXT PRIMARY KEY CHECK (length(attempt_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  turn_id TEXT NOT NULL REFERENCES turns(turn_id),
  continuation_plan_id TEXT NOT NULL REFERENCES continuation_plans(continuation_plan_id),
  routing_decision_id TEXT NOT NULL REFERENCES routing_decisions(routing_decision_id),
  runtime_session_id TEXT NOT NULL REFERENCES runtime_sessions(runtime_session_id),
  runtime_session_revision INTEGER NOT NULL CHECK (runtime_session_revision > 0),
  account_id TEXT NOT NULL REFERENCES accounts(account_id),
  process_generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
  process_generation_revision INTEGER NOT NULL CHECK (process_generation_revision > 0),
  execution_epoch_id TEXT NOT NULL REFERENCES process_execution_epochs(execution_epoch_id),
  request_id TEXT NOT NULL UNIQUE CHECK (length(request_id) = 36),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  provider_idempotency_key TEXT,
  provider_correlation_key TEXT,
  predecessor_attempt_id TEXT REFERENCES provider_attempts(attempt_id),
  duplicate_risk_ack_sha256 TEXT CHECK (length(duplicate_risk_ack_sha256) = 64),
  state TEXT NOT NULL CHECK (
    state IN (
      'prepared',
      'canceled',
      'dispatch_authorized',
      'succeeded',
      'failed_definitive',
      'not_submitted',
      'unknown'
    )
  ),
  unknown_reason TEXT CHECK (
    unknown_reason IN ('supervision_lost', 'dispatch_outcome_unavailable', 'restore_projection')
  ),
  terminal_evidence_id TEXT,
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  CHECK (provider_idempotency_key IS NOT NULL OR provider_correlation_key IS NOT NULL),
  CHECK ((state = 'unknown') = (unknown_reason IS NOT NULL)),
  CHECK (
    (state IN ('succeeded', 'failed_definitive', 'not_submitted')) =
    (terminal_evidence_id IS NOT NULL)
  )
) STRICT;

CREATE UNIQUE INDEX one_nonterminal_provider_attempt_per_turn
  ON provider_attempts(turn_id)
  WHERE state IN ('prepared', 'dispatch_authorized', 'unknown');

CREATE TABLE provider_attempt_positive_evidence (
  evidence_id TEXT PRIMARY KEY CHECK (length(evidence_id) = 36),
  attempt_id TEXT NOT NULL UNIQUE REFERENCES provider_attempts(attempt_id),
  request_id TEXT NOT NULL CHECK (length(request_id) = 36),
  source TEXT NOT NULL CHECK (
    source IN (
      'provider_receipt',
      'positive_idempotency_lookup',
      'exact_turn_readback',
      'exact_thread_readback',
      'positive_non_submission_receipt'
    )
  ),
  outcome TEXT NOT NULL CHECK (outcome IN ('succeeded', 'failed_definitive', 'not_submitted')),
  provider_key TEXT NOT NULL CHECK (
    length(CAST(provider_key AS BLOB)) BETWEEN 1 AND 512
  ),
  provider_receipt_id TEXT,
  provider_thread_id TEXT,
  provider_turn_id TEXT,
  witness_sha256 TEXT NOT NULL CHECK (length(witness_sha256) = 64),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0)
) STRICT;

CREATE INDEX account_registry_order ON account_routing_order(position, account_id);
CREATE INDEX account_quota_by_account ON account_quota_facts(account_id, duration_minutes);
CREATE INDEX conversation_recent ON conversations(updated_at_micros DESC, conversation_id);
CREATE INDEX history_by_conversation ON history_items(conversation_id, sequence);
CREATE INDEX process_generation_by_session ON process_generations(runtime_session_id);
CREATE INDEX provider_attempt_by_session ON provider_attempts(runtime_session_id, created_at_micros);
