PRAGMA foreign_keys = ON;
INSERT INTO account_identities VALUES ('46000000-0000-4000-8000-000000000001', 1);
INSERT INTO account_operations (
  operation_id, account_id, kind, phase, provider, provider_account_id,
  requested_display_label, requested_enabled, created_at_micros, updated_at_micros,
  completed_at_micros
) VALUES (
  '47000000-0000-4000-8000-000000000001',
  '46000000-0000-4000-8000-000000000001', 'import', 'committed', 'chatgpt',
  'opaque-restart-provider', 'Opaque restart account', 1, 1, 1, 1
);
INSERT INTO accounts (
  account_id, display_label, enabled, state, revision, provider, provider_account_id,
  credential_store_observation, created_at_micros, updated_at_micros
) VALUES (
  '46000000-0000-4000-8000-000000000001', 'Opaque restart account', 1, 'available',
  1, 'chatgpt', 'opaque-restart-provider', 'exact', 1, 1
);
INSERT INTO conversations VALUES (
  '44000000-0000-4000-8000-000000000001', 'ordinary_task', 'active',
  'Opaque restart conversation', 1, 1, 1
);
INSERT INTO runtime_sessions (
  runtime_session_id, conversation_id, account_id, account_revision, account_snapshot_id,
  account_display_label, account_observed_state, credential_binding_json,
  profile_snapshot_id, profile_revision, profile_role, model, reasoning_effort,
  instructions, service_tier, instructions_sha256, codex_thread_id, state,
  thread_start_request_id, thread_start_request_sha256, thread_start_response_id,
  thread_start_response_sha256, thread_start_fence_key, thread_start_binding_key,
  thread_start_turn_id, thread_start_continuation_plan_id, thread_start_routing_decision_id,
  thread_start_process_generation_id, thread_start_process_generation_revision,
  thread_start_execution_epoch_id, has_acknowledged_turn, revision,
  created_at_micros, updated_at_micros
) VALUES (
  '41000000-0000-4000-8000-000000000001',
  '44000000-0000-4000-8000-000000000001',
  '46000000-0000-4000-8000-000000000001', 1,
  '48000000-0000-4000-8000-000000000001', 'Opaque restart account', 'available', '{}',
  '49000000-0000-4000-8000-000000000001', 1, 'task', 'gpt-5.6-sol', 'high',
  'Follow the request.', 'default',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'provider/thread?after#restart%opaque', 'active', 1,
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1,
  'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  'thread-fence', 'thread-binding', '45000000-0000-4000-8000-000000000000',
  '4a000000-0000-4000-8000-000000000000', '4b000000-0000-4000-8000-000000000000',
  '42000000-0000-4000-8000-000000000001', 3,
  '43000000-0000-4000-8000-000000000001', 1, 4, 1, 1
);
INSERT INTO turns VALUES (
  '45000000-0000-4000-8000-000000000001',
  '44000000-0000-4000-8000-000000000001',
  '41000000-0000-4000-8000-000000000001', 2, 'user', 'unknown', 'active', 1, 1, 1, NULL
);
INSERT INTO routing_decisions (
  routing_decision_id, operation_id, idempotency_key, request_sha256, authority_shape,
  conversation_id, turn_id, conversation_revision, source_runtime_session_id,
  source_runtime_session_revision, account_snapshot_id, profile_snapshot_id,
  decision_kind, account_id, account_revision, routing_revision, quota_classification,
  causes_json, exclusions_json, created_at_micros
) VALUES (
  '4b000000-0000-4000-8000-000000000001',
  '4c000000-0000-4000-8000-000000000001', 'restart-route',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'conversation_continuation', '44000000-0000-4000-8000-000000000001',
  '45000000-0000-4000-8000-000000000001', 1,
  '41000000-0000-4000-8000-000000000001', 4,
  '48000000-0000-4000-8000-000000000001', '49000000-0000-4000-8000-000000000001',
  'selected', '46000000-0000-4000-8000-000000000001', 1, 1, 'unknown', '[]', '[]', 1
);
INSERT INTO continuation_plans (
  continuation_plan_id, operation_id, idempotency_key, request_sha256, conversation_id,
  turn_id, routing_decision_id, source_runtime_session_id, source_runtime_session_revision,
  selected_account_id, runtime_session_id, kind, codex_thread_id,
  same_thread_attempt_id, same_thread_evidence_id, created_at_micros
) VALUES (
  '4a000000-0000-4000-8000-000000000001',
  '4d000000-0000-4000-8000-000000000001', 'restart-plan',
  'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  '44000000-0000-4000-8000-000000000001',
  '45000000-0000-4000-8000-000000000001',
  '4b000000-0000-4000-8000-000000000001',
  '41000000-0000-4000-8000-000000000001', 4,
  '46000000-0000-4000-8000-000000000001', NULL, 'same_thread',
  'provider/thread?after#restart%opaque',
  '4e000000-0000-4000-8000-000000000001',
  '4f000000-0000-4000-8000-000000000001', 1
);
INSERT INTO process_execution_epochs VALUES (
  '43000000-0000-4000-8000-000000000001',
  'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 1
);
INSERT INTO process_generations (
  generation_id, account_id, runtime_session_id, execution_epoch_id, runner_identity,
  intended_boot_id, control_kind, isolation_kind, bound_boot_id, process_id,
  process_start_id, process_group_id, session_id, account_revision,
  credential_schema_version, credential_version, credential_fingerprint,
  credential_writer_operation_id, provider, provider_account_id,
  refresh_callback_profile_sha256, state, revision, created_at_micros, updated_at_micros
) VALUES (
  '42000000-0000-4000-8000-000000000001',
  '46000000-0000-4000-8000-000000000001',
  '41000000-0000-4000-8000-000000000001',
  '43000000-0000-4000-8000-000000000001', 'sha256:fixture', 'runtime-resume-test',
  'stdio_only_best_effort_eof', 'session', 'runtime-resume-test', 42001,
  'runtime-resume-start', 42001, 42001, 1, 1, 1,
  'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
  '47000000-0000-4000-8000-000000000001', 'chatgpt', 'opaque-restart-provider',
  'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
  'ready', 3, 1, 1
);
