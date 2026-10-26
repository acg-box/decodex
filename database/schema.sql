-- Current Decodex product schema. Initialize an empty database; do not upgrade older layouts.

CREATE TABLE "account_credentials" (
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
  completed_at_micros INTEGER CHECK (completed_at_micros >= created_at_micros), recovery_operation_id TEXT
    REFERENCES account_operations(operation_id)
    DEFERRABLE INITIALLY DEFERRED
    CHECK (recovery_operation_id IS NULL OR recovery_operation_id <> operation_id), superseded_by_operation_id TEXT
    REFERENCES account_operations(operation_id)
    DEFERRABLE INITIALLY DEFERRED
    CHECK (
      superseded_by_operation_id IS NULL OR
      (superseded_by_operation_id <> operation_id AND recovery_operation_id IS NULL)
    ),
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

CREATE TABLE account_profile_daily_usage (
  account_id TEXT NOT NULL REFERENCES account_profile_snapshots(account_id) ON DELETE CASCADE,
  start_date TEXT NOT NULL CHECK (
    length(start_date) = 10 AND substr(start_date, 5, 1) = '-' AND substr(start_date, 8, 1) = '-'
  ),
  tokens INTEGER NOT NULL CHECK (tokens >= 0),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  PRIMARY KEY (account_id, start_date)
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

CREATE TABLE account_quota_activation (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  observed_reset_at_micros INTEGER NOT NULL CHECK (observed_reset_at_micros > 0),
  next_due_at_micros INTEGER NOT NULL CHECK (next_due_at_micros > 0),
  attempted_at_micros INTEGER CHECK (attempted_at_micros > 0),
  outcome TEXT NOT NULL CHECK (outcome IN ('idle', 'unknown', 'completed', 'rejected'))
, observed_at_micros INTEGER
  CHECK (observed_at_micros > 0)) STRICT;

CREATE TABLE "account_quota_facts" (
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

CREATE TABLE account_routing_control (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  mode TEXT NOT NULL CHECK (mode IN ('fixed', 'balanced')),
  fixed_account_id TEXT REFERENCES account_identities(account_id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0),
  CHECK ((mode = 'fixed') = (fixed_account_id IS NOT NULL))
) STRICT;

CREATE TABLE account_routing_order (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  position INTEGER NOT NULL UNIQUE CHECK (position >= 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
) STRICT;

CREATE TABLE account_usage_observations (
  account_id TEXT PRIMARY KEY REFERENCES account_identities(account_id),
  account_revision INTEGER NOT NULL CHECK (account_revision > 0),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  ordinary_usage_allowed INTEGER CHECK (ordinary_usage_allowed IN (0, 1))
, has_credits INTEGER CHECK (has_credits IN (0,1)), unlimited_credits INTEGER CHECK (unlimited_credits IN (0,1)), spend_control_reached INTEGER CHECK (spend_control_reached IN (0,1)), rate_limit_reached INTEGER CHECK (rate_limit_reached IN (0,1))) STRICT;

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

CREATE TABLE "agent_async_answers" (
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    created_at_micros INTEGER NOT NULL,
    PRIMARY KEY(work_id, thread_id, question_id)
) STRICT;

CREATE TABLE "agent_async_questions" (
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    question_json TEXT NOT NULL CHECK(json_valid(question_json)),
    created_at_micros INTEGER NOT NULL, arrived_live INTEGER NOT NULL DEFAULT 0 CHECK(arrived_live IN (0, 1)),
    PRIMARY KEY(work_id, thread_id, question_id)
) STRICT;

CREATE TABLE "agent_async_recovery" (
    required_item_id TEXT,
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    PRIMARY KEY(work_id, thread_id)
) STRICT;

CREATE TABLE "agent_async_skips" (
    work_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    created_at_micros INTEGER NOT NULL,
    PRIMARY KEY(work_id, thread_id, question_id),
    FOREIGN KEY(work_id, thread_id, question_id)
      REFERENCES "agent_async_questions"(work_id, thread_id, question_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE "agent_capacity_retries" (
    event_id INTEGER PRIMARY KEY REFERENCES "agent_inbox_events"(id),
    work_item_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
    failed_turn_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt BETWEEN 1 AND 3),
    due_at_micros INTEGER NOT NULL CHECK (due_at_micros >= 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'claimed', 'submitted', 'cancelled')),
    retry_turn_id TEXT,
    UNIQUE (work_item_id, failed_turn_id),
    UNIQUE (work_item_id, retry_turn_id),
    CHECK (retry_turn_id IS NULL OR retry_turn_id != failed_turn_id),
    CHECK ((state = 'submitted' AND retry_turn_id IS NOT NULL)
        OR (state != 'submitted' AND retry_turn_id IS NULL))
) STRICT;

CREATE TABLE agent_closed_native_threads (
    thread_id TEXT PRIMARY KEY
) STRICT;

CREATE TABLE agent_delegations (
    work_id TEXT PRIMARY KEY REFERENCES agent_work_items(id) ON DELETE CASCADE,
    instruction_event_id INTEGER NOT NULL REFERENCES agent_inbox_events(id)
) STRICT;

CREATE TABLE "agent_dependencies" (
	work_item_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
	depends_on_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
	PRIMARY KEY (work_item_id, depends_on_id),
	CHECK (work_item_id <> depends_on_id)
) STRICT;

CREATE TABLE "agent_guardian_approvals" (
    id INTEGER PRIMARY KEY,
    review_row_id INTEGER NOT NULL REFERENCES "agent_guardian_reviews"(id) ON DELETE CASCADE,
    command_key TEXT NOT NULL UNIQUE,
    review_digest TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    generation_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending','submitted','rejected')),
    created_at_micros INTEGER NOT NULL,
    finished_at_micros INTEGER
) STRICT;

CREATE TABLE "agent_guardian_reviews" (
    id INTEGER PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    review_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    generation_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('inProgress','approved','denied','timedOut','aborted')),
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),
    conflicted INTEGER NOT NULL DEFAULT 0 CHECK (conflicted IN (0,1)),
    created_at_micros INTEGER NOT NULL,
    updated_at_micros INTEGER NOT NULL,
    UNIQUE(work_id,thread_id,turn_id,review_id)
) STRICT;

CREATE TABLE "agent_inbox_events" (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	source_event_id TEXT NOT NULL UNIQUE CHECK (length(source_event_id) BETWEEN 1 AND 2048),
	work_item_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
	event_kind TEXT NOT NULL CHECK (length(event_kind) BETWEEN 1 AND 128),
	payload TEXT NOT NULL CHECK (length(payload) <= 65536),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= 0),
	disposition TEXT CHECK (disposition IN ('resolved', 'follow_up', 'wait', 'user_decision')),
	disposition_note TEXT CHECK (length(disposition_note) BETWEEN 1 AND 65536),
	disposed_at_micros INTEGER CHECK (disposed_at_micros >= created_at_micros),
	delivery_work_item_id TEXT REFERENCES "agent_work_items"(id),
	delivered_turn_id TEXT,
	CHECK ((delivery_work_item_id IS NULL AND delivered_turn_id IS NULL)
		OR (delivery_work_item_id IS NOT NULL AND delivered_turn_id IS NOT NULL)),
	CHECK ((disposition IS NULL AND disposition_note IS NULL AND disposed_at_micros IS NULL)
		OR (disposition IS NOT NULL AND disposition_note IS NOT NULL AND disposed_at_micros IS NOT NULL))
) STRICT;

CREATE TABLE "agent_live_output" (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 work_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
 turn_id TEXT NOT NULL,
 item_id TEXT NOT NULL,
 text TEXT NOT NULL DEFAULT '',
 truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0,1)),
 kind TEXT NOT NULL DEFAULT 'agentMessage' CHECK(kind IN ('agentMessage','plan','reasoningSummary')),
 completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1)),
 summary_parts TEXT,
 UNIQUE(work_id,turn_id,item_id),
 CHECK(length(CAST(text AS BLOB)) <= 65536),
 CHECK(summary_parts IS NULL OR (kind='reasoningSummary' AND length(CAST(summary_parts AS BLOB)) <= 524288))
) STRICT;

CREATE TABLE "agent_managers" (
 work_id TEXT PRIMARY KEY REFERENCES "agent_work_items"(id)
) STRICT;

CREATE TABLE "agent_misalignment" (
    work_id TEXT PRIMARY KEY REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    details_json TEXT CHECK(details_json IS NULL OR json_valid(details_json)),
    created_at_micros INTEGER NOT NULL
, retired_voice INTEGER NOT NULL DEFAULT 0 CHECK(retired_voice IN (0,1))) STRICT;

CREATE TABLE "agent_process_bindings" (
 operation_key TEXT PRIMARY KEY CHECK(length(CAST(operation_key AS BLOB)) BETWEEN 1 AND 256),
 root_id TEXT NOT NULL REFERENCES agent_work_items(id),
 account_id TEXT NOT NULL REFERENCES accounts(account_id),
 generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
 request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
 created_at_micros INTEGER NOT NULL CHECK(created_at_micros>0),
 UNIQUE(root_id,generation_id)
) STRICT;

CREATE TABLE "agent_prompt_input_chunks" (
    upload_id TEXT NOT NULL CHECK (length(upload_id) BETWEEN 1 AND 128),
    byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),
    work_item_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    edit_receipt_id INTEGER NOT NULL REFERENCES "agent_inbox_events"(id),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    total_bytes INTEGER NOT NULL CHECK (total_bytes BETWEEN 1 AND 8388608),
    fragment TEXT NOT NULL CHECK (length(CAST(fragment AS BLOB)) BETWEEN 1 AND 65536),
    PRIMARY KEY (upload_id, byte_offset)
) STRICT;

CREATE TABLE "agent_prompt_inputs" (
    id INTEGER PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES "agent_work_items"(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL CHECK (length(thread_id) BETWEEN 1 AND 512),
    edit_receipt_id INTEGER NOT NULL REFERENCES "agent_inbox_events"(id),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    content TEXT NOT NULL CHECK (
        json_valid(content) AND json_type(content) = 'array'
        AND json_array_length(content) > 0
        AND length(CAST(content AS BLOB)) <= 8388608
    ),
    created_at_micros INTEGER NOT NULL,
    UNIQUE (edit_receipt_id, sha256)
) STRICT;

CREATE TABLE "agent_request_payloads" (
    event_id INTEGER PRIMARY KEY REFERENCES "agent_inbox_events"(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK (json_valid(payload) AND length(CAST(payload AS BLOB)) <= 16842752)
) STRICT;

CREATE TABLE "agent_root_settings" (
	root_id TEXT PRIMARY KEY REFERENCES "agent_work_items"(id),
	config_json TEXT NOT NULL CHECK (length(CAST(config_json AS BLOB)) BETWEEN 2 AND 16384),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE agent_slots (
    agent_key TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES agent_work_items(id) ON DELETE CASCADE,
    thread_id TEXT,
    slot INTEGER NOT NULL UNIQUE CHECK(slot >= 0)
) STRICT;

CREATE TABLE "agent_thread_revisions" (
 work_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
 old_thread_id TEXT NOT NULL,
 new_thread_id TEXT NOT NULL UNIQUE,
 created_at_micros INTEGER NOT NULL,
 PRIMARY KEY(work_id,old_thread_id)
) STRICT;

CREATE TABLE "agent_tool_versions" (
 work_id TEXT PRIMARY KEY REFERENCES "agent_work_items"(id),
 version INTEGER NOT NULL CHECK(version > 0)
) STRICT;

CREATE TABLE "agent_usage" (
    thread_id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
    turn_id TEXT NOT NULL,
    usage_json TEXT NOT NULL CHECK(length(usage_json) <= 1024)
, baseline_input_tokens INTEGER CHECK (baseline_input_tokens >= 0), baseline_output_tokens INTEGER CHECK (baseline_output_tokens >= 0), turn_input_tokens INTEGER CHECK (turn_input_tokens >= 0), turn_output_tokens INTEGER CHECK (turn_output_tokens >= 0)) STRICT;

CREATE TABLE "agent_voice_calls" (
    session_id TEXT PRIMARY KEY NOT NULL CHECK(length(session_id) BETWEEN 1 AND 512),
    work_id TEXT NOT NULL REFERENCES "agent_work_items"(id),
    thread_id TEXT NOT NULL CHECK(length(thread_id) BETWEEN 1 AND 512),
    generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
    baseline_turn_id TEXT,
    created_at_micros INTEGER NOT NULL,
    closed_at_micros INTEGER,
    CHECK(closed_at_micros IS NULL OR closed_at_micros >= created_at_micros)
) STRICT;

CREATE TABLE "agent_voice_observed_turns" (
    generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    PRIMARY KEY(generation_id,thread_id,turn_id)
) STRICT;

CREATE TABLE "agent_work_items" (
	id TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 512),
	parent_goal_id TEXT REFERENCES "agent_work_items"(id),
	kind TEXT NOT NULL CHECK (kind IN ('goal', 'task')),
	title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 1024),
	instructions TEXT NOT NULL CHECK (length(instructions) BETWEEN 1 AND 65536),
	codex_thread_id TEXT UNIQUE CHECK (length(CAST(codex_thread_id AS BLOB)) BETWEEN 1 AND 512),
	dispatch_state TEXT NOT NULL DEFAULT 'idle' CHECK (dispatch_state IN ('idle', 'dispatching', 'running', 'unknown')),
	active_turn_id TEXT CHECK (length(CAST(active_turn_id AS BLOB)) BETWEEN 1 AND 512),
	status TEXT NOT NULL CHECK (status IN ('open', 'resolved', 'follow_up', 'wait', 'user_decision')),
	next_check_at_micros INTEGER CHECK (next_check_at_micros >= 0),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= 0),
	updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
	CHECK (parent_goal_id IS NULL OR parent_goal_id <> id),
	CHECK ((dispatch_state = 'running' AND active_turn_id IS NOT NULL)
		OR dispatch_state = 'unknown'
		OR (dispatch_state IN ('idle', 'dispatching') AND active_turn_id IS NULL))
) STRICT;

CREATE TABLE agents (
 id TEXT PRIMARY KEY REFERENCES agent_work_items(id),
 assistant INTEGER NOT NULL DEFAULT 0 CHECK(assistant IN(0,1))
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

CREATE TABLE collaboration_instructions (
 request_id TEXT NOT NULL REFERENCES collaboration_requests(id),
 id TEXT NOT NULL UNIQUE,
 requirements_id TEXT NOT NULL,
 text TEXT NOT NULL,
 material_ids_json TEXT NOT NULL CHECK(json_valid(material_ids_json)),
 event_id INTEGER NOT NULL UNIQUE REFERENCES agent_inbox_events(id),
 PRIMARY KEY(request_id,id)
) STRICT;

CREATE TABLE collaboration_requests (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL REFERENCES works(id),
 recipient_agent_id TEXT NOT NULL REFERENCES agents(id),
 instruction_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('pending','accepted','returned','closed')),
 required INTEGER NOT NULL CHECK(required IN(0,1)),
 outcome TEXT CHECK(outcome IN('used','not_used','declined','cancelled','failed')),
 note TEXT,
 CHECK((state='closed')=(outcome IS NOT NULL)),
 FOREIGN KEY(id,instruction_id) REFERENCES collaboration_instructions(request_id,id) DEFERRABLE INITIALLY DEFERRED
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
  completed_at_micros INTEGER CHECK (completed_at_micros >= reserved_at_micros), request_json TEXT, progress_json TEXT,
  PRIMARY KEY (protocol, idempotency_key),
  CHECK ((state = 'reserved') = (response_json IS NULL)),
  CHECK ((state = 'reserved') = (completed_at_micros IS NULL)),
  CHECK (
    (state = 'reserved' AND claim_token IS NOT NULL AND claim_expires_at_micros > reserved_at_micros) OR
    (state <> 'reserved' AND claim_token IS NULL AND claim_expires_at_micros IS NULL)
  )
) STRICT;

CREATE TABLE context_packs (
  context_pack_id TEXT PRIMARY KEY CHECK (length(context_pack_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  pack_revision INTEGER NOT NULL CHECK (pack_revision = 1),
  possible_side_effects TEXT NOT NULL CHECK (
    possible_side_effects IN ('none', 'possible', 'unknown')
  ),
  policy_max_bytes INTEGER NOT NULL CHECK (policy_max_bytes BETWEEN 1024 AND 262144),
  policy_recent_item_limit INTEGER NOT NULL CHECK (policy_recent_item_limit BETWEEN 1 AND 256),
  manifest_json TEXT NOT NULL CHECK (
    length(CAST(manifest_json AS BLOB)) BETWEEN 2 AND 1048576
  ),
  manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
  compiled_sha256 TEXT NOT NULL CHECK (length(compiled_sha256) = 64),
  byte_length INTEGER NOT NULL CHECK (byte_length BETWEEN 1 AND 262144),
  truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
  omitted_source_count INTEGER NOT NULL CHECK (omitted_source_count BETWEEN 0 AND 512),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

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

CREATE TABLE conversations (
  conversation_id TEXT PRIMARY KEY CHECK (length(conversation_id) = 36),
  kind TEXT NOT NULL CHECK (kind = 'ordinary_task'),
  state TEXT NOT NULL CHECK (state IN ('active', 'completed', 'failed', 'archived')),
  title TEXT CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 512),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros)
) STRICT;

CREATE TABLE desktop_settings (
	singleton        INTEGER PRIMARY KEY CHECK (singleton = 1),
	show_in_menu_bar INTEGER NOT NULL CHECK (show_in_menu_bar IN (0, 1)),
	revision         INTEGER NOT NULL CHECK (revision > 0)
, auto_activate_quota INTEGER NOT NULL DEFAULT 1
  CHECK (auto_activate_quota IN (0, 1)), auto_recap INTEGER NOT NULL DEFAULT 0 CHECK (auto_recap IN (0, 1))) STRICT;

CREATE TABLE guardian_review_details (
 id INTEGER PRIMARY KEY,
 parent_thread_id TEXT NOT NULL,
 reviewer_thread_id TEXT NOT NULL,
 reviewer_turn_id TEXT NOT NULL,
 item_id TEXT NOT NULL,
 review_id TEXT,
 record_json TEXT NOT NULL CHECK(length(record_json)<=65536 AND json_valid(record_json)),
 UNIQUE(reviewer_thread_id,reviewer_turn_id,item_id)
) STRICT;

CREATE TABLE guardian_settings (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 reviewer TEXT NOT NULL CHECK(reviewer IN('user','auto_review')),
 review_details INTEGER NOT NULL CHECK(review_details IN(0,1)),
 revision INTEGER NOT NULL CHECK(revision>0)
) STRICT;

CREATE TABLE guardian_sources (
 source_id TEXT NOT NULL,
 agent_id TEXT PRIMARY KEY REFERENCES agent_work_items(id),
 thread_id TEXT NOT NULL,
 revision INTEGER NOT NULL,
 outcome TEXT NOT NULL CHECK(outcome IN('pending','applied','overridden','unsupported','rejected','unknown')),
 effective_reviewer TEXT,
 detail TEXT NOT NULL
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

CREATE TABLE native_work_contributions (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 instruction TEXT NOT NULL,
 child_thread_id TEXT,
 parent_turn_id TEXT,
 parent_item_id TEXT,
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id),
 CHECK((child_thread_id IS NULL)=(parent_turn_id IS NULL)),
 CHECK((child_thread_id IS NULL)=(parent_item_id IS NULL)),
 UNIQUE(parent_turn_id,parent_item_id)
) STRICT;

CREATE TABLE native_work_deliveries (
 contribution_id TEXT PRIMARY KEY REFERENCES native_work_contributions(id),
 parent_thread_id TEXT NOT NULL,
 parent_turn_id TEXT NOT NULL,
 child_thread_id TEXT NOT NULL,
 child_turn_id TEXT NOT NULL,
 message_id TEXT NOT NULL, assignment_proof TEXT NOT NULL
 DEFAULT 'plaintext' CHECK(assignment_proof IN('plaintext','native_task_name')),
 UNIQUE(child_thread_id,child_turn_id,message_id)
) STRICT;

CREATE TABLE native_work_dispatches (
 contribution_id TEXT PRIMARY KEY REFERENCES native_work_contributions(id),
 thread_id TEXT NOT NULL,
 turn_id TEXT NOT NULL,
 call_id TEXT NOT NULL,
 UNIQUE(thread_id,turn_id,call_id)
) STRICT;

CREATE TABLE process_execution_epochs (
  execution_epoch_id TEXT PRIMARY KEY CHECK (length(execution_epoch_id) = 36),
  authorization_sha256 TEXT NOT NULL CHECK (length(authorization_sha256) = 64),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TABLE "process_generation_death_evidence" (
  evidence_id TEXT PRIMARY KEY CHECK (length(evidence_id) = 36),
  generation_id TEXT NOT NULL UNIQUE REFERENCES process_generations(generation_id),
  kind TEXT NOT NULL CHECK (
    kind IN (
      'spawn_not_created',
      'owned_child_exit',
      'linux_pidfd_exit',
      'macos_kqueue_exit_and_group_quiescence',
      'macos_kernel_confirmed_gone',
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

CREATE TABLE "process_generations" (
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

CREATE TABLE "project_agents" (
 agent_id TEXT PRIMARY KEY REFERENCES agent_work_items(id) ON DELETE CASCADE,
 project_id TEXT NOT NULL REFERENCES "projects"(id)
) STRICT;

CREATE TABLE "projects" (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 256),
 directory TEXT NOT NULL UNIQUE CHECK(length(directory) BETWEEN 1 AND 4096)
, native_project_id TEXT, lead_agent_id TEXT REFERENCES agent_work_items(id)) STRICT;

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

CREATE TABLE "quick_task_requests" (
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
, model_source_account_id TEXT, model_source_account_revision INTEGER
CHECK (
  (model_source_account_id IS NULL AND model_source_account_revision IS NULL)
  OR (model_source_account_id IS NOT NULL AND model_source_account_revision IS NOT NULL
      AND model_source_account_revision > 0)
), model_source_review_required INTEGER NOT NULL DEFAULT 0
CHECK (model_source_review_required IN (0, 1))) STRICT;

CREATE TABLE "reset_card_operations" (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL,
    account_revision INTEGER NOT NULL CHECK (account_revision > 0),
    granted_at INTEGER NOT NULL CHECK (granted_at >= 0),
    expires_at INTEGER CHECK (expires_at > granted_at),
    exact_credit_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('prepared', 'sending', 'completed', 'failed')),
    outcome TEXT CHECK (outcome IN ('reset', 'nothing_to_reset', 'no_credit', 'already_redeemed')),
    failure TEXT CHECK (failure IN ('account_changed', 'inventory_changed', 'provider_unavailable')),
    CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (exact_credit_id IS NULL OR length(exact_credit_id) BETWEEN 1 AND 1024),
    CHECK ((state IN ('prepared', 'sending') AND exact_credit_id IS NOT NULL AND failure IS NULL AND (state = 'sending' OR outcome IS NULL))
        OR (state = 'completed' AND exact_credit_id IS NULL AND outcome IS NOT NULL AND failure IS NULL)
        OR (state = 'failed' AND exact_credit_id IS NULL AND outcome IS NULL AND failure IS NOT NULL))
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
  instructions TEXT NOT NULL CHECK (
    length(CAST(instructions AS BLOB)) BETWEEN 1 AND 65536
  ),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
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

CREATE TABLE work_commands (
 id TEXT PRIMARY KEY,
 agent_id TEXT NOT NULL REFERENCES agents(id),
 command_json TEXT NOT NULL CHECK(json_valid(command_json))
) STRICT;

CREATE TABLE work_materials (
 id TEXT NOT NULL,
 work_id TEXT NOT NULL REFERENCES works(id),
 record_json TEXT NOT NULL CHECK(json_valid(record_json)),
 PRIMARY KEY(work_id,id)
) STRICT;

CREATE TABLE work_requirements (
 work_id TEXT NOT NULL REFERENCES works(id),
 id TEXT NOT NULL,
 goal TEXT NOT NULL,
 criteria TEXT NOT NULL,
 source TEXT NOT NULL,
 PRIMARY KEY(work_id,id)
) STRICT;

CREATE TABLE work_result_reuse (
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 result_id TEXT NOT NULL REFERENCES work_results(id),
 assessment TEXT NOT NULL,
 PRIMARY KEY(work_id,requirements_id,result_id),
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id)
) STRICT;

CREATE TABLE work_results (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 instruction_id TEXT REFERENCES collaboration_instructions(id),
 agent_id TEXT NOT NULL REFERENCES agents(id),
 record_json TEXT NOT NULL CHECK(json_valid(record_json)),
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id)
) STRICT;

CREATE TABLE works (
 id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 512),
 project_id TEXT REFERENCES projects(id),
 owner_agent_id TEXT NOT NULL REFERENCES agents(id),
 title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 120),
 requirements_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('open','blocked','completed','cancelled')),
 reason TEXT,
 resume_condition TEXT,
 user_hold INTEGER NOT NULL DEFAULT 0 CHECK(user_hold IN(0,1)),
 assessment_json TEXT CHECK(assessment_json IS NULL OR json_valid(assessment_json)),
 CHECK(state<>'blocked' OR (length(reason)>0 AND length(resume_condition)>0)),
 CHECK(state<>'completed' OR assessment_json IS NOT NULL),
 FOREIGN KEY(id,requirements_id) REFERENCES work_requirements(work_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE INDEX account_quota_by_account ON account_quota_facts(account_id, duration_minutes);

CREATE INDEX account_registry_order ON account_routing_order(position, account_id);

CREATE INDEX agent_capacity_due ON agent_capacity_retries(due_at_micros)
WHERE state = 'pending';

CREATE UNIQUE INDEX agent_capacity_pending_work ON agent_capacity_retries(work_item_id)
WHERE state = 'pending';

CREATE UNIQUE INDEX agent_guardian_one_submission
ON agent_guardian_approvals(review_row_id) WHERE state IN ('pending','submitted');

CREATE INDEX agent_inbox_pending ON agent_inbox_events(id) WHERE disposition IS NULL;

CREATE INDEX agent_inbox_work_history ON agent_inbox_events(work_item_id, id);

CREATE INDEX agent_inbox_work_kind ON agent_inbox_events(work_item_id, event_kind, id);

CREATE INDEX agent_process_generation ON agent_process_bindings(generation_id);

CREATE INDEX agent_process_root ON agent_process_bindings(root_id,created_at_micros);

CREATE UNIQUE INDEX agent_voice_one_open_call ON agent_voice_calls((1)) WHERE closed_at_micros IS NULL;

CREATE INDEX agent_voice_thread_generation ON agent_voice_calls(thread_id,generation_id);

CREATE INDEX agent_work_due ON agent_work_items(next_check_at_micros)
WHERE next_check_at_micros IS NOT NULL;

CREATE INDEX collaboration_recipient ON collaboration_requests(recipient_agent_id,state);

CREATE INDEX context_pack_by_conversation
  ON context_packs(conversation_id, created_at_micros, context_pack_id);

CREATE INDEX conversation_recent ON conversations(updated_at_micros DESC, conversation_id);

CREATE INDEX history_by_conversation ON history_items(conversation_id, sequence);

CREATE UNIQUE INDEX one_account_operation_supersession
  ON account_operations(superseded_by_operation_id)
  WHERE superseded_by_operation_id IS NOT NULL;

CREATE UNIQUE INDEX one_active_account_reauthentication_takeover
  ON account_operations(recovery_operation_id)
  WHERE recovery_operation_id IS NOT NULL
    AND phase NOT IN ('committed', 'cancelled');

CREATE UNIQUE INDEX one_assistant ON agents(assistant) WHERE assistant=1;

CREATE UNIQUE INDEX one_initial_routing_decision_per_conversation
  ON routing_decisions(conversation_id)
  WHERE authority_shape = 'conversation_account_registry';

CREATE UNIQUE INDEX one_live_runtime_session_per_conversation
  ON runtime_sessions(conversation_id)
  WHERE state IN ('starting', 'active');

CREATE UNIQUE INDEX one_nonterminal_provider_attempt_per_turn
  ON provider_attempts(turn_id)
  WHERE state IN ('prepared', 'dispatch_authorized', 'unknown');

CREATE UNIQUE INDEX one_quarantining_process_generation_per_account
  ON process_generations(account_id) WHERE state <> 'dead';

CREATE UNIQUE INDEX one_unsettled_account_operation
  ON account_operations(account_id)
  WHERE phase NOT IN ('committed', 'cancelled')
    AND recovery_operation_id IS NULL
    AND superseded_by_operation_id IS NULL;

CREATE INDEX process_generation_by_session ON process_generations(runtime_session_id);

CREATE UNIQUE INDEX project_native_identity ON projects(native_project_id) WHERE native_project_id IS NOT NULL;

CREATE INDEX provider_attempt_by_session ON provider_attempts(runtime_session_id, created_at_micros);

CREATE UNIQUE INDEX reset_card_active_account ON reset_card_operations(account_id)
WHERE state IN ('prepared', 'sending');

CREATE INDEX result_work ON work_results(work_id);

CREATE INDEX work_owner ON works(owner_agent_id,state);

CREATE INDEX work_project ON works(project_id,state);

CREATE INDEX work_workspace_scope ON "project_agents"(project_id);

CREATE TRIGGER agent_capacity_disposition_cancel
AFTER UPDATE OF disposition ON agent_inbox_events
WHEN NEW.disposition IS NOT NULL
BEGIN
    UPDATE agent_capacity_retries SET state = 'cancelled'
    WHERE event_id = NEW.id AND state = 'pending';
END;

CREATE TRIGGER agent_capacity_identity_immutable
BEFORE UPDATE OF event_id, work_item_id, failed_turn_id, attempt, due_at_micros ON agent_capacity_retries
BEGIN
    SELECT RAISE(ABORT, 'capacity retry identity is immutable');
END;

CREATE TRIGGER agent_capacity_transition
BEFORE UPDATE ON agent_capacity_retries
WHEN NOT (
    (OLD.state = 'pending' AND NEW.state IN ('claimed', 'cancelled'))
    OR (OLD.state = 'claimed' AND NEW.state = 'submitted')
    OR (OLD.state = 'claimed' AND NEW.state = 'cancelled' AND EXISTS (
        SELECT 1 FROM agent_inbox_events e
        WHERE e.work_item_id = OLD.work_item_id
          AND e.event_kind = 'capacity_retry_rejected'
          AND e.disposition = 'resolved'
          AND json_extract(e.payload, '$.retryEventId') = OLD.event_id
          AND json_extract(e.payload, '$.reason') IN (
              'serverDraining', 'managedProviderChanged', 'settingsChanged',
              'requestTooLarge', 'requestQueueFull'
          )
    ))
)
BEGIN
    SELECT RAISE(ABORT, 'capacity retry cannot be replayed');
END;

CREATE TRIGGER agent_capacity_work_judgment_cancel
AFTER UPDATE OF status ON agent_work_items
WHEN NEW.status != 'open'
BEGIN
    UPDATE agent_inbox_events SET disposition = 'resolved',
        disposition_note = 'Capacity retry cancelled because the work decision changed.',
        disposed_at_micros = max(created_at_micros, NEW.updated_at_micros)
    WHERE disposition IS NULL AND id IN (
        SELECT event_id FROM agent_capacity_retries
        WHERE work_item_id = NEW.id AND state = 'pending'
    );
END;

CREATE TRIGGER agent_dependency_acyclic BEFORE INSERT ON agent_dependencies
BEGIN
	SELECT RAISE(ABORT, 'dependency cycle') WHERE EXISTS (
		WITH RECURSIVE ancestors(id) AS (
			SELECT NEW.depends_on_id
			UNION
			SELECT depends_on_id FROM agent_dependencies JOIN ancestors ON work_item_id = ancestors.id
		)
		SELECT 1 FROM ancestors WHERE id = NEW.work_item_id
	);
END;

CREATE TRIGGER agent_dependency_immutable BEFORE UPDATE ON agent_dependencies
BEGIN
	SELECT RAISE(ABORT, 'dependency is immutable');
END;

CREATE TRIGGER agent_inbox_disposition_once BEFORE UPDATE ON agent_inbox_events
WHEN OLD.disposition IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'event already disposed');
END;

CREATE TRIGGER agent_inbox_source_immutable
BEFORE UPDATE OF source_event_id, work_item_id, event_kind, payload, created_at_micros ON agent_inbox_events
BEGIN
	SELECT RAISE(ABORT, 'event source is immutable');
END;

CREATE TRIGGER agent_manager_goal BEFORE INSERT ON agent_managers BEGIN
 SELECT RAISE(ABORT,'manager must be a goal') WHERE NOT EXISTS(SELECT 1 FROM agent_work_items WHERE id=NEW.work_id AND kind='goal');
END;

CREATE TRIGGER agent_parent_goal BEFORE INSERT ON agent_work_items
WHEN NEW.parent_goal_id IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'parent must be a goal')
	WHERE NOT EXISTS (SELECT 1 FROM agent_work_items WHERE id = NEW.parent_goal_id AND kind = 'goal');
END;

CREATE TRIGGER agent_process_binding_authority BEFORE INSERT ON agent_process_bindings
BEGIN
 SELECT RAISE(ABORT,'Agent root must be a root goal') WHERE NOT EXISTS(
  SELECT 1 FROM agent_work_items WHERE id=NEW.root_id AND kind='goal' AND parent_goal_id IS NULL
 );
 SELECT RAISE(ABORT,'Previous Agent process must be dead before account rotation') WHERE EXISTS(
  SELECT 1 FROM agent_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id
  WHERE b.root_id=NEW.root_id AND b.account_id<>NEW.account_id AND g.state<>'dead'
 );
 SELECT RAISE(ABORT,'Agent work must be idle before account rotation') WHERE
  coalesce((SELECT account_id<>NEW.account_id FROM agent_process_bindings WHERE root_id=NEW.root_id ORDER BY created_at_micros DESC,rowid DESC LIMIT 1),0)
  AND EXISTS(
   WITH RECURSIVE family(id) AS (
    SELECT NEW.root_id UNION SELECT w.id FROM agent_work_items w JOIN family f ON w.parent_goal_id=f.id
   )
   SELECT 1 FROM agent_work_items WHERE id IN(SELECT id FROM family) AND dispatch_state<>'idle'
  );
 SELECT RAISE(ABORT,'Agent generation ownership differs') WHERE NOT EXISTS(
  SELECT 1 FROM process_generations WHERE generation_id=NEW.generation_id
   AND account_id=NEW.account_id AND runtime_session_id IS NULL AND quick_task_admission_key IS NULL
 );
END;

CREATE TRIGGER agent_process_binding_immutable BEFORE UPDATE ON agent_process_bindings
BEGIN
 SELECT RAISE(ABORT,'Agent process admission is immutable');
END;

CREATE TRIGGER agent_prompt_chunk_immutable
BEFORE UPDATE ON agent_prompt_input_chunks
BEGIN
    SELECT RAISE(ABORT, 'prompt input chunk is immutable');
END;

CREATE TRIGGER agent_prompt_input_immutable
BEFORE UPDATE ON agent_prompt_inputs
BEGIN
    SELECT RAISE(ABORT, 'prompt input is immutable');
END;

CREATE TRIGGER agent_request_payload_immutable
BEFORE UPDATE ON agent_request_payloads
BEGIN
    SELECT RAISE(ABORT, 'request source is immutable');
END;

CREATE TRIGGER agent_root_settings_authority BEFORE INSERT ON agent_root_settings
BEGIN
	SELECT RAISE(ABORT, 'Agent settings require a root goal') WHERE NOT EXISTS (
		SELECT 1 FROM agent_work_items WHERE id = NEW.root_id AND kind = 'goal' AND parent_goal_id IS NULL
	);
END;

CREATE TRIGGER agent_root_settings_immutable BEFORE UPDATE ON agent_root_settings
BEGIN
	SELECT RAISE(ABORT, 'Agent root settings are immutable');
END;

CREATE TRIGGER agent_work_identity_immutable
BEFORE UPDATE OF id, parent_goal_id, kind ON agent_work_items
BEGIN
	SELECT RAISE(ABORT, 'work identity is immutable');
END;

INSERT INTO "account_routing_control" VALUES(1,'balanced',NULL,1,1);

INSERT INTO "desktop_settings" VALUES(1,1,1,1,0);

INSERT INTO "guardian_settings" VALUES(1,'user',0,1);

INSERT INTO "role_profiles" VALUES('task',2,'gpt-5.4','high','default','Follow the user request for this task.',2);
