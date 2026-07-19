#!/usr/bin/env python3
"""Run XY-1267 integration tests in a disposable PostgreSQL 18 cluster."""

from __future__ import annotations

from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass
from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import select
import shutil
import stat
import subprocess
import sys
import tempfile
import time


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1267"
COLLATION_DATABASE = "decodex_xy1267_tr"
RESTORE_DATABASE = "decodex_xy1267_restore"
AUTHORITY_CAPTURE_DATABASE = "decodex_xy1300_authority_capture"
AUTHORITY_CAPTURE_RESTORE_DATABASE = "decodex_xy1300_authority_capture_restore"
AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE = "decodex_xy1300_authority_capture_restore_2"
AUTHORITY_CAPTURE_UPGRADE_DATABASE = "decodex_xy1300_authority_upgrade"
AUTHORITY_CAPTURE_RESTORE_EDGES = (
	("source_to_restored_once", "source", "restored_once"),
	("restored_once_to_restored_twice", "restored_once", "restored_twice"),
)
AUTHORITY_CANDIDATE_SCHEMA = "decodex/postgres-authority-candidate/2"
AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES = 128 * 1024
GIT_READ_TIMEOUT_SECONDS = 5.0
GIT_METADATA_MAX_BYTES = 4 * 1024
GIT_COMMIT_MAX_BYTES = 64 * 1024
GIT_STATUS_MAX_BYTES = 64 * 1024
GIT_PATH_LIST_MAX_BYTES = 64 * 1024
GIT_AUTHORITY_SOURCE_MAX_BYTES = 512 * 1024
AUTHORITY_DIGEST_CONSTANTS = (
	("schema", "SCHEMA_CONTRACT_SHA256"),
	("configured_authority", "CONFIGURED_AUTHORITY_SHA256"),
)
DEFAULT_ACL_TAMPER_DATABASE = "decodex_xy1315_default_acl_tamper"
DEFAULT_ACL_RESTORE_DATABASE = "decodex_xy1315_default_acl_restore"
AUTHORITY_DATABASE = "decodex_xy1307_authority"
TRIGGER_DATABASE = "decodex_xy1307_trigger_contract"
FUNCTION_DATABASE = "decodex_xy1307_function_contract"
PRIVILEGED_FUNCTION_DATABASE = "decodex_xy1307_privileged_function"
TRIGGER_ESCAPE_DATABASE = "decodex_xy1307_trigger_escape"
EXTENSION_CONTROL_DATABASE = "decodex_xy1307_extension_control"
HOSTILE_SEARCH_DATABASE = "decodex_xy1307_hostile_search"
CONSTRAINT_DRIFT_DATABASE = "decodex_xy1307_constraint_drift"
IDENTITY_CAST_DATABASE = "decodex_xy1315_identity_cast"
EXTERNAL_CASCADE_DATABASE = "decodex_xy1307_external_cascade"
LEDGER_TAMPER_DATABASE = "decodex_xy1307_ledger_tamper"
MISSING_EXTENSION_DATABASE = "decodex_xy1307_missing_extension"
V8_EMPTY_DATABASE = "decodex_xy1274_v8_empty"
V8_LOCK_DATABASE = "decodex_xy1274_v8_lock"
ROLE_PROFILE_CONCURRENCY_DATABASE = "decodex_xy1346_concurrency"
ROLE_PROFILE_UPGRADE_DATABASE = "decodex_xy1346_upgrade"
ROLE_PROFILE_ROLLBACK_DATABASE = "decodex_xy1346_rollback"
ROLE_PROFILE_RETRY_DATABASE = "decodex_xy1346_retry"
ROLE_PROFILE_CRASH_DATABASE = "decodex_xy1346_crash"
ROLE_PROFILE_RESTORE_SOURCE_DATABASE = "decodex_xy1346_restore_source"
ROLE_PROFILE_RESTORE_DATABASE = "decodex_xy1346_restore"
RUNTIME_SESSION_COMMAND_DATABASE = "decodex_xy1337_commands"
RUNTIME_SESSION_ROLLBACK_DATABASE = "decodex_xy1337_rollback"
RUNTIME_SESSION_RETRY_DATABASE = "decodex_xy1337_retry"
RUNTIME_SESSION_UPGRADE_DATABASE = "decodex_xy1337_upgrade"
RUNTIME_SESSION_FENCE_DATABASE = "decodex_xy1337_fence"
RUNTIME_SESSION_CRASH_DATABASE = "decodex_xy1337_crash"
RUNTIME_SESSION_RESTORE_SOURCE_DATABASE = "decodex_xy1337_restore_source"
RUNTIME_SESSION_RESTORE_DATABASE = "decodex_xy1337_restore"
WORK_ITEM_DATABASE = "decodex_xy1343_work_items"
WORK_ITEM_RESTORE_DATABASE = "decodex_xy1343_work_items_restore"
MANAGED_RUN_DATABASE = "decodex_xy1338_managed_runs"
MANAGED_RUN_RESTORE_DATABASE = "decodex_xy1338_managed_runs_restore"
MANAGED_REPOSITORY_DATABASE = "decodex_xy1364_managed_repositories"
MIGRATION_ROLE = "decodex_migration"
RUNTIME_ROLE = "decodex_runtime_xy1300"
FUNCTION_OWNER_ROLE = "decodex_function_owner"
SET_BYPASS_ROLE = "decodex_set_bypass"
SET_LEDGER_WRITE_ROLE = "decodex_set_ledger_write"
SET_SEQUENCE_UPDATE_ROLE = "decodex_set_sequence_update"
MEMBERSHIP_ADMIN_ROLE = "decodex_membership_admin_target"
MISSING_SELECT_ROLE = "decodex_incompatible_missing_history_select"
HOSTILE_SEARCH_ROLE = "decodex_hostile_search_runtime"
UNSAFE_ROLES = {
	"table-owner": "decodex_unsafe_table_owner",
	"truncate": "decodex_unsafe_truncate",
	"bypassrls": "decodex_unsafe_bypassrls",
	"schema-create": "decodex_unsafe_schema_create",
	"trigger-bypass": "decodex_unsafe_trigger_bypass",
	"alter-system-bypass": "decodex_unsafe_alter_system_bypass",
	"login-default-replica": "decodex_unsafe_login_default_replica",
	"function-owner-membership": "decodex_unsafe_function_owner",
	"migration-history-write": "decodex_unsafe_migration_history_write",
	"set-role-bypass": "decodex_unsafe_set_role_bypass",
	"migration-history-column-grant": "decodex_unsafe_history_column_grant",
	"migration-history-set-write": "decodex_unsafe_history_set_write",
	"sequence-update": "decodex_unsafe_sequence_update",
	"sequence-set-update": "decodex_unsafe_sequence_set_update",
	"sequence-grant-option": "decodex_unsafe_sequence_grant",
	"table-grant-option": "decodex_unsafe_table_grant",
	"function-grant-option": "decodex_unsafe_function_grant",
	"collation-owner": "decodex_unsafe_collation_owner",
	"conversion-owner": "decodex_unsafe_conversion_owner",
	"operator-owner": "decodex_unsafe_operator_owner",
	"text-search-owner": "decodex_unsafe_text_search_owner",
	"membership-admin": "decodex_unsafe_membership_admin",
	"superuser": "decodex_unsafe_superuser",
}
RUNTIME_EXECUTE_SIGNATURES = (
	"decodex.is_canonical_media_type(pg_catalog.text)",
	"decodex.is_history_metadata_projection(pg_catalog.jsonb)",
	"decodex.normalize_unicode_whitespace(pg_catalog.text)",
	"decodex.ascii_lower(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.text)",
	"decodex.has_credential_material(pg_catalog.jsonb)",
	"decodex.is_meaningful_evidence(pg_catalog.jsonb)",
	"decodex.rfc3339_utc(pg_catalog.timestamptz)",
	"decodex.is_valid_operation_duration(pg_catalog.interval)",
	"decodex.lease_ttl_milliseconds(pg_catalog.interval)",
	"decodex.try_acquire_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.interval)",
	"decodex.renew_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.interval)",
	"decodex.release_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.prune_history_snapshots()",
	"decodex.issue_history_cursor(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int4)",
	"decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text)",
	"decodex.create_project(decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text)",
	"decodex.transition_project(decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.project_status)",
	"decodex.create_policy(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text)",
	"decodex.accept_policy_revision(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.int8)",
	"decodex.create_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.update_program_context(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.jsonb,pg_catalog.jsonb,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.transition_program(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.program_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.create_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog._text,pg_catalog._text,pg_catalog.int8,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.transition_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.objective_state,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.text)",
	"decodex.achieve_objective(decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,decodex.canonical_uuid_v4_text,pg_catalog.int8,pg_catalog.text,decodex.canonical_uuid_v4_text)",
	"decodex.bootstrap_role_profiles_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.update_role_profile_exact(pg_catalog.text,pg_catalog.text,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.create_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,decodex.role_profile_role,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex.account_state,pg_catalog.int8,pg_catalog.uuid,decodex.runtime_session_state)",
	"decodex.transition_runtime_session_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.runtime_session_state)",
	"decodex.create_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.update_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog._uuid,pg_catalog.text,pg_catalog.text,decodex.work_item_priority,pg_catalog._text,pg_catalog._text,decodex.work_item_state,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.assess_work_item_readiness_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text)",
	"decodex.accept_work_item_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.guard_work_item_running_resume(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.replace_routing_policy_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog._uuid,pg_catalog._int8,decodex._routing_member_disposition,decodex._codex_capability)",
	"decodex.publish_routing_evidence_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex._codex_capability,decodex._capability_evidence_state)",
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_codex_experiment_thread_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool)",
	"decodex.record_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_continuation_plan_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_waiting_usage_wake_transition_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.register_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.claim_due_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.fire_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.cancel_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
)
TRIGGER_ONLY_SIGNATURES = (
	"decodex.enforce_lease_operation_time()",
	"decodex.enforce_outbox_operation_time()",
	"decodex.enforce_quota_observation_monotonicity()",
	"decodex.forbid_mutation_of_activity()",
	"decodex.enforce_outbox_terminal_retention()",
	"decodex.forbid_outbox_truncate()",
	"decodex.enforce_command_receipt_state()",
	"decodex.acquire_hierarchy_coordinator()",
	"decodex.canonicalize_created_at()",
	"decodex.enforce_blob_object_state()",
	"decodex.enforce_conversation_state()",
	"decodex.enforce_runtime_session_state()",
	"decodex.enforce_turn_state()",
	"decodex.enforce_history_item_state()",
	"decodex.capture_history_item_version()",
	"decodex.enforce_artifact_state()",
	"decodex.enforce_artifact_revision_state()",
	"decodex.enforce_context_pack_state()",
	"decodex.enforce_context_pack_source_state()",
	"decodex.enforce_history_cursor_state()",
	"decodex.enforce_policy_identity_state()",
	"decodex.forbid_policy_revision_mutation()",
	"decodex.enforce_program_state()",
	"decodex.enforce_objective_state()",
	"decodex.forbid_objective_evidence_mutation()",
	"decodex.enforce_objective_completion_coherence()",
	"decodex.enforce_exact_receipt_completion()",
	"decodex.forbid_exact_receipt_rewrite()",
	"decodex.forbid_exact_receipt_truncate()",
	"decodex.enforce_complete_role_profile_set()",
	"decodex.forbid_role_profile_identity_rewrite()",
	"decodex.forbid_role_profile_revision_mutation()",
	"decodex.forbid_role_profile_truncate()",
	"decodex.enforce_role_profile_event_namespace()",
	"decodex.enforce_runtime_session_command_owner()",
	"decodex.forbid_runtime_snapshot_mutation()",
	"decodex.enforce_runtime_session_event_namespace()",
	"decodex.enforce_work_item_state()",
	"decodex.enforce_work_item_command_owner()",
	"decodex.forbid_work_item_acceptance_mutation()",
	"decodex.enforce_work_item_acceptance_coherence()",
	"decodex.enforce_work_item_event_namespace()",
	"decodex.enforce_managed_run_command_owner()",
	"decodex.forbid_managed_run_immutable_mutation()",
	"decodex.enforce_managed_run_assignment_scope()",
	"decodex.enforce_managed_run_state()",
	"decodex.enforce_effect_barrier_state()",
	"decodex.enforce_managed_run_event_namespace()",
	"decodex.forbid_managed_repository_history_mutation()",
	"decodex.enforce_managed_repository_projection()",
	"decodex.enforce_repository_operation_scope()",
	"decodex.enforce_repository_history_completeness()",
	"decodex.forbid_routing_history_mutation()",
	"decodex.enforce_routing_completeness()",
	"decodex.enforce_routing_command_owner()",
	"decodex.forbid_codex_experiment_history_mutation()",
	"decodex.enforce_codex_experiment_command_owner()",
	"decodex.forbid_routing_decision_mutation()",
	"decodex.enforce_routing_decision_completeness()",
	"decodex.forbid_continuation_plan_mutation()",
	"decodex.enforce_continuation_plan_completeness()",
	"decodex.enforce_continuation_event_namespace()",
	"decodex.enforce_waiting_usage_wake_command_owner()",
	"decodex.forbid_waiting_usage_wake_transition_mutation()",
	"decodex.enforce_waiting_usage_wake_transition_complete()",
	"decodex.enforce_waiting_usage_wake_head_projection()",
	"decodex.enforce_waiting_usage_wake_event_namespace()",
)
RUNTIME_TYPE_NAMES = (
	"decodex.account_state",
	"decodex.outbox_state",
	"decodex.effect_state",
	"decodex.conversation_status",
	"decodex.runtime_session_state",
	"decodex.turn_role",
	"decodex.side_effect_state",
	"decodex.history_item_kind",
	"decodex.history_item_status",
	"decodex.turn_status",
	"decodex.artifact_status",
	"decodex.context_source_kind",
	"decodex.transition_kind",
	"decodex.context_source_disposition",
	"decodex.command_receipt_state",
	"decodex.canonical_uuid_v4_text",
	"decodex.project_status",
	"decodex.agent_role",
	"decodex.agent_status",
	"decodex.program_state",
	"decodex.objective_state",
	"decodex.quota_window_class",
	"decodex.observation_confidence",
	"decodex.role_profile_role",
	"decodex.work_item_priority",
	"decodex.work_item_state",
	"decodex.work_item_edge_kind",
	"decodex.work_item_blocker_kind",
	"decodex.managed_run_lifecycle",
	"decodex.managed_run_phase",
	"decodex.managed_run_wait_reason",
	"decodex.execution_assignment_role",
	"decodex.effect_barrier_state",
	"decodex.managed_run_effect_kind",
	"decodex.managed_run_effect_state",
	"decodex.managed_run_safety_input_kind",
	"decodex.managed_repository_phase",
	"decodex.repository_operation_kind",
	"decodex.repository_operation_state",
	"decodex.repository_ambiguity",
	"decodex.repository_authority_transition_kind",
	"decodex.repository_evidence_kind",
	"decodex.routing_member_disposition",
	"decodex.codex_capability",
	"decodex.capability_evidence_state",
	"decodex.routing_blocker",
	"decodex.codex_experiment_observation_kind",
)
AUTHORITY_ANCHOR_SIGNATURE = (
	"decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,"
	"pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,"
	"decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)"
)
UPGRADE_RUNTIME_EXECUTE_SIGNATURES = (
	"decodex.replace_routing_policy_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog._uuid,pg_catalog._int8,decodex._routing_member_disposition,decodex._codex_capability)",
	"decodex.publish_routing_evidence_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex._codex_capability,decodex._capability_evidence_state)",
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_codex_experiment_thread_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool)",
	"decodex.record_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_continuation_plan_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_waiting_usage_wake_transition_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.register_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.claim_due_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.fire_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.cancel_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
)
UPGRADE_RUNTIME_TYPE_NAMES = (
	"decodex.routing_member_disposition",
	"decodex.codex_capability",
	"decodex.capability_evidence_state",
	"decodex.routing_blocker",
	"decodex.codex_experiment_observation_kind",
)
V19_INTERNAL_SIGNATURES = (
	"decodex.register_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.timestamptz)",
	"decodex.claim_due_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.timestamptz)",
	"decodex.fire_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.timestamptz)",
	"decodex.cancel_waiting_usage_wake_exact_internal(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.timestamptz)",
)

MANIFEST_DIAGNOSTIC_ERROR_LIMIT = 512
MANIFEST_DIAGNOSTIC_IDENTITY_LIMIT = 256
MANIFEST_DIAGNOSTIC_EVIDENCE_LIMIT = 8
MANIFEST_DIAGNOSTIC_SCHEMA = "decodex/postgres-manifest-component-diagnostic/1"
MANIFEST_QUERY_ERROR_SCHEMA = "decodex/postgres-manifest-query-error/1"
RESTORE_PARITY_DIAGNOSTIC_SCHEMA = "decodex/postgres-restore-parity-diagnostic/1"
SEMANTIC_AUTHORITY_SCHEMA = "decodex/postgres-semantic-authority/1"
SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA = "decodex/postgres-semantic-authority-diagnostic/1"
SEMANTIC_AUTHORITY_PREDICATES = (
	"configured_runtime_session",
	"no_forbidden_role_attributes",
	"no_database_create",
	"no_schema_create",
	"no_effective_object_ownership",
	"no_function_grant_option",
	"no_trigger_bypass",
	"no_alter_system_bypass",
	"session_replication_role_origin",
	"no_membership_admin",
	"exact_table_authority",
	"no_unsafe_table_authority",
	"migration_history_exists",
	"migration_history_select",
	"no_unsafe_migration_history_authority",
	"exact_sequence_contract",
	"sequence_usage",
	"no_unsafe_sequence_authority",
	"no_extension_control",
	"schema_usage",
	"identity_cast_closed",
	"exact_trigger_inventory",
	"no_relation_rules",
	"no_relation_policies",
	"closed_function_dependencies",
	"exact_function_inventory",
	"function_metadata",
	"function_semantics",
	"function_execute_authority",
	"retention_inventory",
	"retention_trigger_bindings",
	"retention_function_metadata",
	"retention_function_semantics",
)
CONSTRAINT_CONTRACT_FIELDS = (
	"constraint_type",
	"definition",
	"deferrable",
	"deferred",
	"validated",
	"enforced",
	"update_action",
	"delete_action",
	"match_type",
	"is_local",
	"inheritance_count",
	"no_inherit",
	"source_columns",
	"referenced_columns",
	"referenced_namespace",
	"referenced_relation",
)
MANIFEST_CLIENT_ERROR_MESSAGE = "manifest query failed without PostgreSQL database error"


class TestFailure(RuntimeError):
	"""Raised when isolated PostgreSQL setup or the integration test fails."""


class HarnessCorruption(RuntimeError):
	"""Raised when the harness cannot truthfully continue or report ordinary failure."""


@dataclass
class StageOrchestrator:
	"""Local aggregate scheduler for meaningful PostgreSQL harness stages."""

	stages: dict[str, dict[str, object]]
	outputs: list[str]
	primary_failure: Exception | None = None
	corruption: Exception | None = None
	scheduling_stopped: bool = False
	cluster_started: bool = False


def require_stage_report(orchestrator: StageOrchestrator) -> None:
	if (
		not isinstance(orchestrator.stages, dict)
		or not isinstance(orchestrator.outputs, list)
		or any(
			not isinstance(name, str)
			or not isinstance(result, dict)
			or result.get("status") not in {"passed", "failed", "blocked"}
			for name, result in orchestrator.stages.items()
		)
	):
		raise HarnessCorruption("aggregate stage report state is invalid")


def corruption_failure(error: Exception) -> HarnessCorruption | None:
	if isinstance(error, HarnessCorruption):
		return error
	if isinstance(error, (AssertionError, KeyError, TypeError)):
		return HarnessCorruption(
			f"{type(error).__name__}: {error or 'no diagnostic'}"
		)
	message = str(error).lower()
	if any(classification in message for classification in (
		"source binding",
		"source commit",
		"git source lineage",
		"frozen postgresql gate",
		"redact",
		"secret marker",
		"disclosed secret-bearing",
		"serialized a role-setting canary",
		"forbidden operational",
	)):
		return HarnessCorruption(str(error))
	return None


def run_stage(
	orchestrator: StageOrchestrator,
	name: str,
	action: Callable[[], object],
	*,
	depends_on: tuple[str, ...] = (),
	fatal: bool = False,
	always_run: bool = False,
) -> object | None:
	"""Run one semantic stage, preserving ordinary failure and dependency truth."""
	require_stage_report(orchestrator)
	if name in orchestrator.stages:
		raise HarnessCorruption(f"aggregate stage {name} was scheduled more than once")
	blocked_by = [
		dependency for dependency in depends_on
		if orchestrator.stages.get(dependency, {}).get("status") != "passed"
	]
	if not always_run and (orchestrator.scheduling_stopped or blocked_by):
		if orchestrator.scheduling_stopped:
			blocked_by.append("harness_scheduling_stopped")
		orchestrator.stages[name] = {
			"status": "blocked",
			"blocked_by": list(dict.fromkeys(blocked_by)),
		}
		return None
	try:
		result = action()
	except Exception as error:
		corruption = corruption_failure(error)
		failure = corruption or error
		orchestrator.stages[name] = {
			"status": "failed",
			"classification": "harness_corruption" if corruption else "test_failure",
			"error": str(failure),
		}
		if orchestrator.primary_failure is None:
			orchestrator.primary_failure = failure
		if corruption is not None:
			orchestrator.corruption = corruption
			orchestrator.scheduling_stopped = True
		elif not isinstance(error, TestFailure):
			orchestrator.corruption = HarnessCorruption(
				f"unexpected {type(error).__name__}: {error}"
			)
			orchestrator.stages[name]["classification"] = "harness_corruption"
			if orchestrator.primary_failure is None:
				orchestrator.primary_failure = orchestrator.corruption
			orchestrator.scheduling_stopped = True
		elif fatal:
			orchestrator.scheduling_stopped = True
		return None
	orchestrator.stages[name] = {"status": "passed"}
	if isinstance(result, str) and result:
		orchestrator.outputs.append(result)
	return result


@dataclass(frozen=True)
class AuthorityCandidatePublication:
	"""An exception-free capture awaiting post-cleanup atomic publication."""

	output_path: Path
	receipt: dict[str, object]


@dataclass(frozen=True)
class PhaseAAuthorityReceipt:
	"""Validated immutable derivation evidence consumed only by Phase B."""

	document: dict[str, object]
	sha256: str


class ClusterStatus(Enum):
	"""Tri-state result from pg_ctl status."""

	RUNNING = "running"
	STOPPED = "stopped"
	UNKNOWN = "unknown"


def run(command: list[str], env: dict[str, str]) -> str:
	completed = subprocess.run(
		command,
		check=False,
		text=True,
		capture_output=True,
		env=env,
		cwd=REPO_ROOT,
	)
	if completed.returncode != 0:
		raise TestFailure(
			f"command failed ({completed.returncode}): {' '.join(command)}\n"
			f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
		)
	return completed.stdout.strip() or completed.stderr.strip()


def frozen_source_binding() -> dict[str, str]:
	status = git_read_text(
		"status", "--porcelain=v1", "--untracked-files=all",
		byte_limit=GIT_STATUS_MAX_BYTES,
	).strip()
	if status:
		raise TestFailure("frozen PostgreSQL gate requires a clean exact committed worktree")
	binding = {
		"head": git_read_text(
			"rev-parse", "--verify", "HEAD", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
		"tree": git_read_text(
			"rev-parse", "--verify", "HEAD^{tree}", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
	}
	require_commit_tree_binding(binding)
	return binding


def run_blob_session_restart_contract(
	data_dir: Path,
	log_path: Path,
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
) -> str:
	"""Restart PostgreSQL under an in-flight Rust BlobSession and prove fenced recovery."""
	sync = work / "blob-session-restart"
	sync.mkdir()
	test_env = env.copy()
	test_env["DECODEX_TEST_BLOB_RESTART_SYNC"] = str(sync)
	process = subprocess.Popen(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
			"postgres_store", "--run-ignored", "all", "--",
			"postgres_blob_session_restart_contract", "--exact",
		],
		text=True,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=test_env,
		cwd=REPO_ROOT,
	)
	try:
		deadline = time.monotonic() + 30
		while time.monotonic() < deadline:
			if (sync / "ready").exists():
				break
			if process.poll() is not None:
				stdout, stderr = process.communicate()
				raise TestFailure(f"BlobSession restart fixture exited early\n{stdout}\n{stderr}")
			time.sleep(0.02)
		else:
			raise TestFailure("BlobSession restart fixture did not reach publication barrier")

		run(["pg_ctl", "-D", str(data_dir), "-m", "immediate", "-w", "stop"], env)
		run(
			[
				"pg_ctl", "-D", str(data_dir), "-l", str(log_path), "-o",
				f"-k {socket_dir} -p {port} -h '' -F", "-w", "start",
			],
			env,
		)
		psql(
			DATABASE,
			"ALTER TABLE decodex.command_receipts DISABLE TRIGGER command_receipts_state_guard; "
			"UPDATE decodex.command_receipts SET claim_expires_at=created_at+interval '1 microsecond' "
			"WHERE idempotency_key='restart-artifact' AND receipt_state='pending'; "
			"ALTER TABLE decodex.command_receipts ENABLE TRIGGER command_receipts_state_guard",
			env,
		)
		(sync / "restarted").write_text("restarted", encoding="utf-8")
		stdout, stderr = process.communicate(timeout=60)
		if process.returncode != 0:
			raise TestFailure(f"BlobSession restart contract failed\n{stdout}\n{stderr}")
		return stdout.strip() or stderr.strip()
	finally:
		if process.poll() is None:
			process.terminate()
			process.wait(timeout=10)


def run_live_doctor_mutation(
	root: Path,
	database: str,
	sql: str,
	case: str,
	work: Path,
	env: dict[str, str],
	*,
	unsafe_authority: bool = False,
	cluster_authority: bool = False,
	secret_sql: bool = False,
	mutation_probe: str | None = None,
) -> str:
	"""Coordinate a real daemon query around an adapter-owned database mutation."""
	sync = work / f"live-doctor-{case}"
	sync.mkdir()
	test_env = env.copy()
	test_env["DECODEX_TEST_LIVE_INCOMPATIBLE_ROOT"] = str(root)
	test_env["DECODEX_TEST_LIVE_INCOMPATIBLE_SYNC"] = str(sync)
	if unsafe_authority:
		test_env["DECODEX_TEST_LIVE_EXPECTED_UNSAFE"] = "1"
	command = [
		"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
		"bootstrap_doctor", "--run-ignored", "all", "--",
		"isolated_postgres_live_doctor_detects_database_drift", "--exact",
	]
	process = subprocess.Popen(
		command,
		text=True,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=test_env,
		cwd=REPO_ROOT,
	)
	deadline = time.monotonic() + 20

	while not (sync / "ready").exists():
		if process.poll() is not None:
			stdout, stderr = process.communicate()
			if secret_sql:
				raise TestFailure("live doctor exited before secret-bearing mutation")
			raise TestFailure(
				f"live doctor exited before {case} mutation\nstdout:\n{stdout}\nstderr:\n{stderr}"
			)
		if time.monotonic() >= deadline:
			process.kill()
			stdout, stderr = process.communicate()
			if secret_sql:
				raise TestFailure("live doctor did not reach the secret-bearing mutation barrier")
			raise TestFailure(
				f"live doctor did not reach {case} barrier\nstdout:\n{stdout}\nstderr:\n{stderr}"
			)
		time.sleep(0.01)

	if cluster_authority:
		if secret_sql:
			psql_secret(database, sql, env)
		else:
			psql(database, sql, env)
	else:
		psql_as(MIGRATION_ROLE, database, sql, env)
	if mutation_probe is not None and psql(database, mutation_probe, env) != "t":
		process.terminate()
		process.communicate(timeout=10)
		raise TestFailure(f"{case} authority mutation probe is vacuous")
	(sync / "mutated").write_text("mutated", encoding="utf-8")
	try:
		stdout, stderr = process.communicate(timeout=30)
	except subprocess.TimeoutExpired as error:
		process.kill()
		stdout, stderr = process.communicate()
		if secret_sql:
			raise TestFailure("live doctor did not finish the secret-bearing drift check") from error
		raise TestFailure(
			f"live doctor did not finish {case}\nstdout:\n{stdout}\nstderr:\n{stderr}"
		) from error
	if process.returncode != 0:
		if secret_sql:
			raise TestFailure("live doctor failed after the secret-bearing mutation")
		raise TestFailure(
			f"live doctor failed after {case} mutation\nstdout:\n{stdout}\nstderr:\n{stderr}"
		)

	return stdout.strip() or stderr.strip()


def postgres_status(data_dir: Path, env: dict[str, str]) -> ClusterStatus:
	"""Preserve status errors instead of treating them as a stopped server."""
	completed = subprocess.run(
		["pg_ctl", "-D", str(data_dir), "status"],
		check=False,
		text=True,
		capture_output=True,
		env=env,
		cwd=REPO_ROOT,
	)
	if completed.returncode == 0:
		return ClusterStatus.RUNNING
	if completed.returncode == 3:
		return ClusterStatus.STOPPED
	return ClusterStatus.UNKNOWN


def database_url(socket_dir: Path, port: int, database: str, role: str) -> str:
	return f"postgresql://{role}@/{database}?host={socket_dir.as_posix()}&port={port}"


def psql(database: str, sql: str, env: dict[str, str]) -> str:
	return run(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database, "-c", sql],
		env,
	)


def psql_secret(
	database: str, sql: str, env: dict[str, str], *, expect_failure: bool = False
) -> str:
	"""Execute secret-bearing SQL only after one live session disables statement logging."""
	ready_marker = "XY1272_SECRET_LOGGING_READY"
	done_marker = "XY1272_SECRET_SQL_DONE"
	process = subprocess.Popen(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database],
		text=True,
		bufsize=1,
		stdin=subprocess.PIPE,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=env,
		cwd=REPO_ROOT,
	)
	if process.stdin is None or process.stdout is None or process.stderr is None:
		raise TestFailure("secret-bearing PostgreSQL fixture pipes are unavailable")
	try:
		process.stdin.write(
			"SET log_min_error_statement=PANIC;\n"
			"SET log_min_messages=PANIC;\n"
			"SET log_statement=none;\n"
			"SET log_duration=off;\n"
			"SET log_min_duration_statement=-1;\n"
			"SET log_min_duration_sample=-1;\n"
			"SET log_statement_sample_rate=0;\n"
			"SET log_transaction_sample_rate=0;\n"
			"SET log_parameter_max_length=0;\n"
			"SET log_parameter_max_length_on_error=0;\n"
			"SET debug_print_parse=off;\n"
			"SET debug_print_rewritten=off;\n"
			"SET debug_print_plan=off;\n"
			"SET log_parser_stats=off;\n"
			"SET log_planner_stats=off;\n"
			"SET log_executor_stats=off;\n"
			"SET log_statement_stats=off;\n"
			"SELECT pg_catalog.concat_ws('|',"
			"pg_catalog.current_setting('log_min_error_statement'),"
			"pg_catalog.current_setting('log_min_messages'),"
			"pg_catalog.current_setting('log_statement'),"
			"pg_catalog.current_setting('log_duration'),"
			"pg_catalog.current_setting('log_min_duration_statement'),"
			"pg_catalog.current_setting('log_min_duration_sample'),"
			"pg_catalog.current_setting('log_statement_sample_rate'),"
			"pg_catalog.current_setting('log_transaction_sample_rate'),"
			"pg_catalog.current_setting('log_parameter_max_length'),"
			"pg_catalog.current_setting('log_parameter_max_length_on_error'),"
			"pg_catalog.current_setting('debug_print_parse'),"
			"pg_catalog.current_setting('debug_print_rewritten'),"
			"pg_catalog.current_setting('debug_print_plan'),"
			"pg_catalog.current_setting('log_parser_stats'),"
			"pg_catalog.current_setting('log_planner_stats'),"
			"pg_catalog.current_setting('log_executor_stats'),"
			"pg_catalog.current_setting('log_statement_stats'),"
			"pg_catalog.current_setting('logging_collector'),"
			"pg_catalog.current_setting('log_destination'));\n"
			f"\\echo {ready_marker}\n"
		)
		process.stdin.flush()
		ready, _, _ = select.select([process.stdout], [], [], 10)
		if not ready:
			raise TestFailure("secret-bearing PostgreSQL fixture logging check timed out")
		settings = process.stdout.readline().strip()
		if process.stdout.readline().strip() != ready_marker:
			raise TestFailure("secret-bearing PostgreSQL fixture logging check did not complete")
		expected = "panic|panic|none|off|-1|-1|0|0|0|0|off|off|off|off|off|off|off|off|stderr"
		if settings != expected:
			raise TestFailure("secret-bearing PostgreSQL fixture logging is not fail-closed")

		process.stdin.write("\\set VERBOSITY terse\n")
		if expect_failure:
			process.stdin.write("\\set ON_ERROR_STOP off\n")
		process.stdin.write(sql)
		if not sql.rstrip().endswith(";"):
			process.stdin.write(";")
		process.stdin.write(f"\n\\echo {done_marker}\n\\quit\n")
		process.stdin.flush()
		stdout, stderr = process.communicate(timeout=10)
		lines = stdout.splitlines()
		if process.returncode != 0 or done_marker not in lines:
			raise TestFailure("secret-bearing PostgreSQL fixture command failed")
		if expect_failure:
			if "ERROR:" not in stderr:
				raise TestFailure("secret-bearing PostgreSQL failure probe unexpectedly succeeded")
			return ""
		if stderr:
			raise TestFailure("secret-bearing PostgreSQL fixture emitted diagnostics")
		return "\n".join(line for line in lines if line != done_marker).strip()
	except subprocess.TimeoutExpired as error:
		process.kill()
		process.communicate()
		raise TestFailure("secret-bearing PostgreSQL fixture command timed out") from error
	finally:
		if process.poll() is None:
			process.terminate()
			process.wait(timeout=10)


def psql_as(role: str, database: str, sql: str, env: dict[str, str]) -> str:
	role_env = env.copy()
	role_env["PGUSER"] = role
	return psql(database, sql, role_env)


def assert_postgres_logs_redact(log_paths: tuple[Path, ...], markers: tuple[str, ...]) -> None:
	for log_path in log_paths:
		contents = log_path.read_bytes()
		if any(marker.encode("utf-8") in contents for marker in markers):
			raise TestFailure("PostgreSQL server log disclosed secret-bearing canary material")


def assert_psql_rejected(
	role: str, database: str, sql: str, env: dict[str, str], context: str
) -> None:
	role_env = env.copy()
	role_env["PGUSER"] = role
	completed = subprocess.run(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database, "-c", sql],
		check=False,
		text=True,
		capture_output=True,
		env=role_env,
		cwd=REPO_ROOT,
	)
	if completed.returncode == 0:
		raise TestFailure(f"{context}: forbidden SQL unexpectedly succeeded")


def set_contract_urls(
	env: dict[str, str], socket_dir: Path, port: int, database: str, runtime_role: str
) -> None:
	env["DECODEX_TEST_MIGRATION_DATABASE_URL"] = database_url(
		socket_dir, port, database, MIGRATION_ROLE
	)
	env["DECODEX_TEST_RUNTIME_DATABASE_URL"] = database_url(
		socket_dir, port, database, runtime_role
	)


def create_database(database: str, env: dict[str, str], *, locale: str | None = None) -> None:
	locale_clause = ""
	if locale is not None:
		locale_clause = f" LOCALE_PROVIDER icu ICU_LOCALE '{locale}'"
	psql(
		"postgres",
		f"CREATE DATABASE {database} WITH TEMPLATE template0 ENCODING 'UTF8' "
		f"OWNER {MIGRATION_ROLE}{locale_clause}",
		env,
	)
	psql(database, f"GRANT USAGE, CREATE ON SCHEMA public TO {MIGRATION_ROLE}", env)
	psql(
		"postgres",
		f"REVOKE CREATE ON DATABASE {database} FROM PUBLIC; "
		f"GRANT CONNECT, CREATE ON DATABASE {database} TO {MIGRATION_ROLE}; "
		f"GRANT CONNECT ON DATABASE {database} TO {RUNTIME_ROLE}",
		env,
	)


def run_migration(env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
			"postgres_store", "--run-ignored", "all", "--",
			"postgres_migration_contract", "--exact",
		],
		env,
	)


def run_migration_through_v13(env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			"postgres_migration_through_v13_fixture", "--exact",
		],
		env,
	)


def dump_schema_manifest(
	path: Path,
	database: str,
	env: dict[str, str],
	*,
	structured_errors: bool = False,
) -> str:
	manifest_env = env.copy()
	manifest_env["DECODEX_SCHEMA_MANIFEST_PATH"] = str(path)
	manifest_env["DECODEX_EXPECTED_MANIFEST_DATABASE"] = database
	manifest_env.pop("DECODEX_SCHEMA_MANIFEST_STRUCTURED_ERRORS", None)
	if structured_errors:
		manifest_env["DECODEX_SCHEMA_MANIFEST_STRUCTURED_ERRORS"] = "1"
	return run(
		["cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
		 "test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
		 "postgres_schema_manifest_dump_fixture", "--exact"],
		manifest_env,
	)


def run_role_profile_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			f"role_profiles::{test}", "--exact",
		],
		env,
	)


def prepare_role_profile_database(
	database: str, socket_dir: Path, port: int, env: dict[str, str]
) -> None:
	create_database(database, env)
	set_contract_urls(env, socket_dir, port, database, RUNTIME_ROLE)
	run_migration(env)
	provision_runtime(database, RUNTIME_ROLE, env)


def run_role_profile_crash_recovery(
	data_dir: Path,
	log_path: Path,
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
) -> str:
	sync = work / "role-profile-crash-recovery"
	sync.mkdir()
	test_env = env.copy()
	test_env["DECODEX_ROLE_PROFILE_RESTART_SYNC"] = str(sync)
	process = subprocess.Popen(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			"role_profiles::postgres_exact_role_profile_crash_recovery", "--exact",
		],
		text=True,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=test_env,
		cwd=REPO_ROOT,
	)
	try:
		deadline = time.monotonic() + 30
		while time.monotonic() < deadline:
			if (sync / "ready").exists():
				break
			if process.poll() is not None:
				stdout, stderr = process.communicate()
				raise TestFailure(
					f"RoleProfile crash fixture exited early\n{stdout}\n{stderr}"
				)
			time.sleep(0.02)
		else:
			raise TestFailure("RoleProfile crash fixture did not reach its lock barrier")

		run(["pg_ctl", "-D", str(data_dir), "-m", "immediate", "-w", "stop"], env)
		run(
			[
				"pg_ctl", "-D", str(data_dir), "-l", str(log_path), "-o",
				f"-k {socket_dir} -p {port} -h '' -F", "-w", "start",
			],
			env,
		)
		(sync / "restarted").write_text("restarted", encoding="utf-8")
		stdout, stderr = process.communicate(timeout=60)
		if process.returncode != 0:
			raise TestFailure(f"RoleProfile crash/recovery failed\n{stdout}\n{stderr}")
		return stdout.strip() or stderr.strip()
	finally:
		if process.poll() is None:
			process.terminate()
			process.wait(timeout=10)


def run_role_profile_final_gate_contracts(
	data_dir: Path,
	log_path: Path,
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
	restore_report: dict[str, object],
) -> str:
	outputs: list[str] = []
	for database, test in (
		(ROLE_PROFILE_CONCURRENCY_DATABASE, "postgres_exact_role_profile_concurrency"),
		(ROLE_PROFILE_ROLLBACK_DATABASE, "postgres_exact_role_profile_atomic_rollback"),
		(ROLE_PROFILE_RETRY_DATABASE, "postgres_exact_role_profile_retry_convergence"),
	):
		prepare_role_profile_database(database, socket_dir, port, env)
		outputs.append(run_role_profile_test(test, env))

	create_database(ROLE_PROFILE_UPGRADE_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, ROLE_PROFILE_UPGRADE_DATABASE, RUNTIME_ROLE
	)
	outputs.append(run_role_profile_test("postgres_v8_to_v9_role_profile_upgrade", env))

	prepare_role_profile_database(ROLE_PROFILE_CRASH_DATABASE, socket_dir, port, env)
	outputs.append(
		run_role_profile_crash_recovery(
			data_dir, log_path, socket_dir, port, work, env
		)
	)

	prepare_role_profile_database(
		ROLE_PROFILE_RESTORE_SOURCE_DATABASE, socket_dir, port, env
	)
	outputs.append(run_role_profile_test("postgres_exact_role_profile_commands", env))
	source_manifest_path = work / "xy1346-role-profiles-source-manifest.json"
	outputs.append(capture_restore_checkpoint(
		restore_report,
		"role_profile_post_command",
		source_manifest_path,
		ROLE_PROFILE_RESTORE_SOURCE_DATABASE,
		env,
	))
	restore_dump = work / "xy1346-role-profiles.dump"
	restore_ready = True
	try:
		run(
			[
				"pg_dump", "-Fc", "-f", str(restore_dump),
				ROLE_PROFILE_RESTORE_SOURCE_DATABASE,
			],
			env,
		)
		create_database(ROLE_PROFILE_RESTORE_DATABASE, env)
		run(
			[
				"pg_restore", "--exit-on-error", "-d", ROLE_PROFILE_RESTORE_DATABASE,
				str(restore_dump),
			],
			env,
		)
	except TestFailure as error:
		restore_ready = False
		record_restore_stage(
			restore_report, "role_profile_restore", "failed", error=str(error)
		)
	else:
		record_restore_stage(restore_report, "role_profile_restore", "passed")
	if restore_stage_ready(restore_report, "role_profile_restored_capture") and restore_ready:
		set_contract_urls(
			env, socket_dir, port, ROLE_PROFILE_RESTORE_DATABASE, RUNTIME_ROLE
		)
		restored_manifest_path = work / "xy1346-role-profiles-restored-manifest.json"
		outputs.append(capture_restore_checkpoint(
			restore_report,
			"role_profile_restored",
			restored_manifest_path,
			ROLE_PROFILE_RESTORE_DATABASE,
			env,
		))
		outputs.append(record_restore_production_check(
			restore_report,
			"role_profile_restored",
			lambda: run_role_profile_test("postgres_exact_role_profile_restore", env),
		))
	else:
		checkpoints = restore_report["checkpoints"]
		assert isinstance(checkpoints, dict)
		checkpoints["role_profile_restored"] = unavailable_checkpoint(
			"role_profile_restore prerequisite is unavailable"
		)
		record_restore_production_check(
			restore_report,
			"role_profile_restored",
			lambda: run_role_profile_test("postgres_exact_role_profile_restore", env),
		)
	set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)

	return "\n".join(outputs)


def run_runtime_session_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			f"runtime_sessions::{test}", "--exact",
		],
		env,
	)


def run_work_item_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			f"work_items::{test}", "--exact",
		],
		env,
	)


def run_managed_run_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			f"managed_runs::{test}", "--exact",
		],
		env,
	)


def run_managed_repository_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			f"managed_repositories::{test}", "--exact",
		],
		env,
	)


def run_managed_repository_focused_contracts(
	socket_dir: Path, port: int, env: dict[str, str]
) -> str:
	create_database(MANAGED_REPOSITORY_DATABASE, env)
	set_contract_urls(env, socket_dir, port, MANAGED_REPOSITORY_DATABASE, RUNTIME_ROLE)
	migration_output = run_migration(env)
	provision_runtime(MANAGED_REPOSITORY_DATABASE, RUNTIME_ROLE, env)
	contract_output = run_managed_repository_test(
		"postgres_managed_repository_authority_contract", env
	)
	return "\n".join((migration_output, contract_output))


def rust_digest_constant(name: str) -> str:
	authority = (
		REPO_ROOT / "crates/decodex-postgres/src/authority.rs"
	).read_text(encoding="utf-8")
	match = re.search(
		rf"const {re.escape(name)}: \[u8; 32\] = \[(.*?)\];",
		authority,
		flags=re.DOTALL,
	)
	if match is None:
		raise TestFailure(f"missing Rust digest constant {name}")
	values = re.findall(r"0x([0-9a-fA-F]{2})", match.group(1))
	if len(values) != 32:
		raise TestFailure(f"invalid Rust digest constant {name}")
	return "".join(value.lower() for value in values)


def unavailable_component(reason: str) -> dict[str, object]:
	return {
		"available": False,
		"complete": False,
		"error": reason,
		"manifest": None,
	}


def unavailable_checkpoint(reason: str) -> dict[str, object]:
	return {
		"authority": unavailable_component(reason),
		"binding": None,
		"capture_error": reason,
		"schema": unavailable_component(reason),
		"sequence_state": None,
	}


def validate_semantic_manifest(
	document: object, location: str, *, require_semantic_authority: bool = False
) -> dict[str, object]:
	if not isinstance(document, dict):
		raise TestFailure(f"invalid semantic manifest envelope at {location}")
	expected_fields = {"schema", "authority", "binding", "sequence_state"}
	if require_semantic_authority:
		expected_fields.add("semantic_authority")
	if set(document) != expected_fields:
		raise TestFailure(f"invalid semantic manifest envelope at {location}")
	if not isinstance(document["binding"], dict) or not isinstance(
		document["sequence_state"], list
	):
		raise TestFailure(f"invalid semantic manifest evidence at {location}")
	for component_name in ("schema", "authority"):
		component = document[component_name]
		if not isinstance(component, dict) or set(component) != {
			"available", "complete", "error", "manifest"
		}:
			raise TestFailure(f"invalid {component_name} component at {location}")
		available = component["available"]
		complete = component["complete"]
		manifest = component["manifest"]
		error = component["error"]
		if not isinstance(available, bool) or not isinstance(complete, bool):
			raise TestFailure(f"invalid {component_name} status at {location}")
		if error is not None and not isinstance(error, str):
			raise TestFailure(f"invalid {component_name} error at {location}")
		if available != isinstance(manifest, str) or complete and not available:
			raise TestFailure(f"incoherent {component_name} availability at {location}")
		if isinstance(manifest, str):
			rows = json.loads(manifest)
			if not isinstance(rows, list):
				raise TestFailure(
					f"invalid {component_name} row manifest at {location}"
				)
	if require_semantic_authority:
		validate_semantic_authority_evidence(document["semantic_authority"])
	return document


def validate_semantic_authority_evidence(evidence: object) -> list[dict[str, object]]:
	if (
		not isinstance(evidence, dict)
		or set(evidence) != {"predicates", "schema"}
		or evidence["schema"] != SEMANTIC_AUTHORITY_SCHEMA
		or not isinstance(evidence["predicates"], list)
		or len(evidence["predicates"]) != len(SEMANTIC_AUTHORITY_PREDICATES)
	):
		raise TestFailure("invalid semantic authority evidence")
	predicates = evidence["predicates"]
	assert isinstance(predicates, list)
	names: set[str] = set()
	for predicate in predicates:
		if (
			not isinstance(predicate, dict)
			or set(predicate) != {"name", "passed"}
			or not isinstance(predicate["name"], str)
			or re.fullmatch(r"[a-z][a-z0-9_]{0,63}", predicate["name"]) is None
			or not isinstance(predicate["passed"], bool)
			or predicate["name"] in names
		):
			raise TestFailure("invalid semantic authority predicate")
		names.add(predicate["name"])
	if tuple(predicate["name"] for predicate in predicates) != SEMANTIC_AUTHORITY_PREDICATES:
		raise TestFailure("semantic authority predicate contract differs")
	return predicates


def require_capture_semantic_authority(
	document: dict[str, object], checkpoint: str, *, secret_markers: tuple[str, ...]
) -> dict[str, object]:
	predicates = validate_semantic_authority_evidence(document.get("semantic_authority"))
	failed = sorted(
		predicate["name"] for predicate in predicates if predicate["passed"] is False
	)
	if failed:
		diagnostic = {
			"checkpoint": checkpoint,
			"failed_predicates": failed,
			"predicate_count": len(predicates),
			"schema": SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
		}
		serialized = json.dumps(diagnostic, sort_keys=True, separators=(",", ":"))
		if any(marker and marker in serialized for marker in secret_markers):
			raise TestFailure("semantic authority diagnostic redaction failed")
		raise TestFailure("authority candidate semantic diagnostic: " + serialized)
	passed = sorted(predicate["name"] for predicate in predicates)
	return {
		"all_passed": True,
		"passed_predicates": passed,
		"predicate_count": len(passed),
		"schema": SEMANTIC_AUTHORITY_SCHEMA,
	}


def load_semantic_manifest(path: Path) -> dict[str, object]:
	return validate_semantic_manifest(
		json.loads(path.read_text(encoding="utf-8")), str(path)
	)


def validate_capture_manifest_rows(document: dict[str, object]) -> None:
	for component_name in ("schema", "authority"):
		component = document[component_name]
		assert isinstance(component, dict)
		manifest = component.get("manifest")
		if not isinstance(manifest, str):
			continue
		rows = json.loads(manifest)
		for row in rows:
			if (
				not isinstance(row, list) or len(row) != 3
				or not isinstance(row[0], str) or not isinstance(row[2], str)
			):
				raise TestFailure(f"malformed {component_name} manifest row")
			contract = json.loads(row[2])
			if not isinstance(contract, list):
				raise TestFailure(f"malformed {component_name} manifest contract")
			if row[0] in {"dependency", "function_dependency", "type_dependency"}:
				decode_dependency_manifest_row(row)


def decode_dependency_manifest_row(
	row: object,
) -> tuple[str, object, str, object, object, bool] | None:
	if not isinstance(row, list) or len(row) != 3 or row[0] not in {
		"dependency", "function_dependency", "type_dependency"
	}:
		return None
	kind, identity, encoded_contract = row
	contract = (
		json.loads(encoded_contract)
		if isinstance(encoded_contract, str)
		else encoded_contract
	)
	expected_identity_length = 5 if kind == "dependency" else 4
	if (
		not isinstance(identity, list)
		or len(identity) != expected_identity_length
		or not isinstance(contract, list)
		or len(contract) != 1
		or not isinstance(contract[0], bool)
	):
		raise TestFailure("malformed schema dependency contract")
	if kind == "dependency":
		(
			source_kind,
			source_identity,
			dependency_type,
			reference_class,
			reference_key,
		) = identity
	else:
		source_kind = kind
		source_identity, dependency_type, reference_class, reference_key = identity
	if (
		not isinstance(source_kind, str)
		or not isinstance(source_identity, list)
		or not isinstance(dependency_type, str)
		or not isinstance(reference_class, list)
		or reference_key is not None and not isinstance(reference_key, list)
	):
		raise TestFailure("malformed schema dependency contract")
	return (
		source_kind,
		source_identity,
		dependency_type,
		reference_class,
		reference_key,
		contract[0],
	)


def structured_manifest_query_error(error: str) -> dict[str, object]:
	try:
		document = json.loads(error)
	except json.JSONDecodeError:
		raise TestFailure("invalid structured manifest query error") from None
	if (
		not isinstance(document, dict)
		or set(document) != {
			"classification", "message", "message_truncated", "schema", "sqlstate"
		}
		or document["schema"] != MANIFEST_QUERY_ERROR_SCHEMA
		or document["classification"] not in {"database_error", "client_error"}
		or not isinstance(document["message"], str)
		or not isinstance(document["message_truncated"], bool)
	):
		raise TestFailure("invalid structured manifest query error")
	if document["classification"] == "database_error":
		sqlstate = document["sqlstate"]
		if (
			not document["message"]
			or len(document["message"]) > MANIFEST_DIAGNOSTIC_ERROR_LIMIT
			or (
				document["message_truncated"] is True
				and len(document["message"]) != MANIFEST_DIAGNOSTIC_ERROR_LIMIT
			)
			or not isinstance(sqlstate, str)
			or len(sqlstate) != 5
			or not sqlstate.isascii()
			or not sqlstate.isalnum()
			or sqlstate != sqlstate.upper()
		):
			raise TestFailure("invalid structured manifest database error")
	elif (
		document["message"] != MANIFEST_CLIENT_ERROR_MESSAGE
		or document["message_truncated"] is not False
		or document["sqlstate"] is not None
	):
		raise TestFailure("invalid structured manifest client error")
	return document


def validate_capture_component_errors(document: dict[str, object]) -> None:
	for component_name in ("schema", "authority"):
		component = document[component_name]
		assert isinstance(component, dict)
		error = component.get("error")
		if error is None:
			continue
		if component.get("available") is True or not isinstance(error, str):
			raise TestFailure(f"invalid {component_name} query error")
		structured_manifest_query_error(error)


def component_manifest(
	document: dict[str, object], component_name: str, *, require_complete: bool = True
) -> str | None:
	component = document[component_name]
	assert isinstance(component, dict)
	if not component["available"] or require_complete and not component["complete"]:
		return None
	manifest = component["manifest"]
	assert isinstance(manifest, str)
	return manifest


def redacted_text(value: str, secret_markers: tuple[str, ...]) -> str:
	redacted = value
	for marker in secret_markers:
		if marker:
			redacted = redacted.replace(marker, "[REDACTED]")
	return redacted


def bounded_redacted_text(
	value: str, secret_markers: tuple[str, ...], limit: int
) -> tuple[str, bool]:
	redacted = redacted_text(value, secret_markers)
	truncated = len(redacted) > limit
	return redacted[:limit], truncated


def bounded_binding(
	binding: object, secret_markers: tuple[str, ...]
) -> dict[str, object] | None:
	if not isinstance(binding, dict):
		return None
	result: dict[str, object] = {}
	for key in (
		"requested", "migration_url", "runtime_url", "observed_migration",
		"observed_runtime", "head", "tree",
	):
		value = binding.get(key)
		if isinstance(value, str):
			text, truncated = bounded_redacted_text(
				value, secret_markers, MANIFEST_DIAGNOSTIC_IDENTITY_LIMIT
			)
			result[key] = text
			if truncated:
				result[f"{key}_truncated"] = True
	return result


def bounded_evidence_value(
	value: object, secret_markers: tuple[str, ...]
) -> str:
	text = value if isinstance(value, str) else json.dumps(
		value, sort_keys=True, separators=(",", ":")
	)
	return bounded_redacted_text(
		text, secret_markers, MANIFEST_DIAGNOSTIC_IDENTITY_LIMIT
	)[0]


def unresolved_dependency_rows(manifest: str) -> list[object]:
	rows = json.loads(manifest)
	unresolved = []
	for row in rows:
		dependency = decode_dependency_manifest_row(row)
		if dependency is not None and dependency[-1] is False:
			unresolved.append(row)
	return unresolved


def manifest_component_diagnostic(
	document: dict[str, object],
	component_name: str,
	*,
	secret_markers: tuple[str, ...] = (),
) -> dict[str, object]:
	component = document[component_name]
	assert isinstance(component, dict)
	available = component.get("available") is True
	complete = component.get("complete") is True
	error = component.get("error")
	manifest = component.get("manifest")
	manifest_present = isinstance(manifest, str)
	if isinstance(error, str):
		structured_error = structured_manifest_query_error(error)
		classification = structured_error["classification"]
		sqlstate = structured_error["sqlstate"]
		error_text, bounded_truncated = bounded_redacted_text(
			structured_error["message"],
			secret_markers,
			MANIFEST_DIAGNOSTIC_ERROR_LIMIT,
		)
		error_truncated = structured_error["message_truncated"] or bounded_truncated
	elif manifest_present and not complete:
		classification = "incomplete_manifest"
		sqlstate = None
		error_text = "manifest reported incomplete"
		error_truncated = False
	elif not available:
		classification = "unavailable_without_error"
		sqlstate = None
		error_text = "manifest unavailable without a query error"
		error_truncated = False
	else:
		classification = None
		sqlstate = None
		error_text = None
		error_truncated = False

	manifest_diagnostic: dict[str, object] = {
		"present": manifest_present,
		"row_count": None,
		"sha256": None,
	}
	unresolved_count = 0
	unresolved_evidence: list[dict[str, object]] = []
	if isinstance(manifest, str):
		rows = json.loads(manifest)
		assert isinstance(rows, list)
		manifest_diagnostic["row_count"] = len(rows)
		manifest_diagnostic["sha256"] = hashlib.sha256(
			manifest.encode("utf-8")
		).hexdigest()
		if component_name == "schema" and not complete:
			unresolved = unresolved_dependency_rows(manifest)
			unresolved_count = len(unresolved)
			for row in unresolved[:MANIFEST_DIAGNOSTIC_EVIDENCE_LIMIT]:
				dependency = decode_dependency_manifest_row(row)
				assert dependency is not None
				(
					source_kind,
					source_identity,
					dependency_type,
					reference_class,
					reference_key,
					_,
				) = dependency
				unresolved_evidence.append({
					"dependency_type": bounded_evidence_value(
						dependency_type, secret_markers
					),
					"identity": bounded_evidence_value(
						source_identity, secret_markers
					),
					"kind": bounded_evidence_value(source_kind, secret_markers),
					"reference_class": bounded_evidence_value(
						reference_class, secret_markers
					),
					"reference_key": bounded_evidence_value(
						reference_key, secret_markers
					),
				})

	return {
		"available": available,
		"complete": complete,
		"component": component_name,
		"error": {
			"classification": classification,
			"sqlstate": sqlstate,
			"text": error_text,
			"truncated": error_truncated,
		},
		"manifest": manifest_diagnostic,
		"unresolved_dependencies": {
			"count": unresolved_count,
			"evidence": unresolved_evidence,
			"evidence_truncated": (
				unresolved_count > MANIFEST_DIAGNOSTIC_EVIDENCE_LIMIT
			),
		},
	}


def capture_manifest_diagnostic(
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
	document: dict[str, object] | None = None,
	component_names: tuple[str, ...] = (),
	artifact_classification: str | None = None,
	artifact_bytes: bytes | None = None,
	artifact_error: str | None = None,
) -> str:
	diagnostic = {
		"capture_only": True,
		"checkpoint": {
			"database_binding": bounded_binding(
				None if document is None else document.get("binding"), secret_markers
			),
			"expected_database": expected_database,
			"name": checkpoint,
		},
		"components": [] if document is None else [
			manifest_component_diagnostic(
				document, component_name, secret_markers=secret_markers
			)
			for component_name in component_names
		],
		"schema": MANIFEST_DIAGNOSTIC_SCHEMA,
		"source_binding": bounded_binding(source_binding, secret_markers),
	}
	if artifact_classification is not None:
		error_text, error_truncated = bounded_redacted_text(
			artifact_error or "artifact unavailable without parser error",
			secret_markers,
			MANIFEST_DIAGNOSTIC_ERROR_LIMIT,
		)
		diagnostic["artifact"] = {
			"byte_length": None if artifact_bytes is None else len(artifact_bytes),
			"classification": artifact_classification,
			"error": {"text": error_text, "truncated": error_truncated},
			"readable": artifact_bytes is not None,
			"sha256": None if artifact_bytes is None else hashlib.sha256(
				artifact_bytes
			).hexdigest(),
		}
	serialized = json.dumps(diagnostic, sort_keys=True, separators=(",", ":"))
	if any(marker and marker in serialized for marker in secret_markers):
		raise TestFailure("manifest diagnostic redaction failed")
	return serialized


def parse_capture_manifest(
	artifact_bytes: bytes,
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> dict[str, object]:
	try:
		document = json.loads(artifact_bytes.decode("utf-8"))
		validated = validate_semantic_manifest(
			document, checkpoint, require_semantic_authority=True
		)
		validate_capture_manifest_rows(validated)
		require_manifest_binding(validated, expected_database)
		validate_capture_component_errors(validated)
		return validated
	except Exception as error:
		raise TestFailure(
			"authority candidate capture diagnostic: "
			+ capture_manifest_diagnostic(
				checkpoint,
				expected_database,
				source_binding=source_binding,
				secret_markers=secret_markers,
				artifact_classification="artifact_malformed",
				artifact_bytes=artifact_bytes,
				artifact_error=str(error),
			)
		) from None


def load_capture_manifest(
	path: Path,
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> dict[str, object]:
	try:
		artifact_bytes = path.read_bytes()
	except Exception as error:
		raise TestFailure(
			"authority candidate capture diagnostic: "
			+ capture_manifest_diagnostic(
				checkpoint,
				expected_database,
				source_binding=source_binding,
				secret_markers=secret_markers,
				artifact_classification="artifact_unreadable",
				artifact_error=str(error),
			)
		) from None
	return parse_capture_manifest(
		artifact_bytes,
		checkpoint,
		expected_database,
		source_binding=source_binding,
		secret_markers=secret_markers,
	)


def require_capture_components(
	document: dict[str, object],
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> dict[str, str]:
	manifests = {
		component_name: component_manifest(document, component_name)
		for component_name in ("schema", "authority")
	}
	failed_components = tuple(
		component_name for component_name, manifest in manifests.items()
		if manifest is None
	)
	if failed_components:
		raise TestFailure(
			"authority candidate capture diagnostic: "
			+ capture_manifest_diagnostic(
				checkpoint,
				expected_database,
				source_binding=source_binding,
				secret_markers=secret_markers,
				document=document,
				component_names=failed_components,
			)
		)
	assert all(isinstance(manifest, str) for manifest in manifests.values())
	return {name: manifest for name, manifest in manifests.items() if manifest is not None}


def semantic_row_diff(before: str, after: str) -> dict[str, list[object]]:
	before_rows = json.loads(before)
	after_rows = json.loads(after)
	if not isinstance(before_rows, list) or not isinstance(after_rows, list):
		raise TestFailure("semantic manifest component is not a row array")
	def keyed(rows: list[object]) -> dict[str, list[object]]:
		indexed: dict[str, list[object]] = {}
		for row in rows:
			if not isinstance(row, list) or len(row) != 3:
				raise TestFailure("semantic manifest row is not a kind/identity/contract tuple")
			key = json.dumps(row[:2], sort_keys=True, separators=(",", ":"))
			if key in indexed:
				raise TestFailure("semantic manifest contains a duplicate kind/identity key")
			indexed[key] = row
		return indexed
	before_index = keyed(before_rows)
	after_index = keyed(after_rows)
	return {
		"before_only": [before_index[key] for key in sorted(before_index.keys()-after_index.keys())],
		"after_only": [after_index[key] for key in sorted(after_index.keys()-before_index.keys())],
		"contract_mismatches": [
			{
				"kind": before_index[key][0],
				"identity": before_index[key][1],
				"before_contract": before_index[key][2],
				"after_contract": after_index[key][2],
			}
			for key in sorted(before_index.keys() & after_index.keys())
			if before_index[key][2] != after_index[key][2]
		],
	}


def redacted_contract_sha256(
	contract: object, secret_markers: tuple[str, ...]
) -> str:
	encoded = contract if isinstance(contract, str) else json.dumps(
		contract, sort_keys=True, separators=(",", ":")
	)
	redacted = redacted_text(encoded, secret_markers)
	return hashlib.sha256(redacted.encode("utf-8")).hexdigest()


def constraint_contract_changes(
	mismatch: dict[str, object],
) -> list[tuple[str, object, object]]:
	contracts: list[list[object]] = []
	for name in ("before_contract", "after_contract"):
		encoded = mismatch[name]
		if not isinstance(encoded, str):
			raise TestFailure("constraint contract is not encoded JSON")
		contract = json.loads(encoded)
		if not isinstance(contract, list) or len(contract) != len(CONSTRAINT_CONTRACT_FIELDS):
			raise TestFailure("constraint contract does not have the expected field shape")
		if not isinstance(contract[1], str):
			raise TestFailure("constraint definition is not text")
		contracts.append(contract)
	before, after = contracts
	changes = [
		(field, before_value, after_value)
		for field, before_value, after_value in zip(
			CONSTRAINT_CONTRACT_FIELDS, before, after, strict=True
		)
		if before_value != after_value
	]
	if not changes:
		raise TestFailure("constraint contract mismatch has no changed fields")
	return changes


def constraint_definition_change(
	before: object, after: object, secret_markers: tuple[str, ...]
) -> dict[str, object]:
	if not isinstance(before, str) or not isinstance(after, str):
		raise TestFailure("constraint definition is not text")
	before_bytes = redacted_text(before, secret_markers).encode("utf-8")
	after_bytes = redacted_text(after, secret_markers).encode("utf-8")
	common_prefix = 0
	for before_byte, after_byte in zip(before_bytes, after_bytes):
		if before_byte != after_byte:
			break
		common_prefix += 1
	return {
		"after_sha256": hashlib.sha256(after_bytes).hexdigest(),
		"after_utf8_byte_length": len(after_bytes),
		"before_sha256": hashlib.sha256(before_bytes).hexdigest(),
		"before_utf8_byte_length": len(before_bytes),
		"common_prefix_utf8_byte_length": common_prefix,
		"field": "definition",
	}


def semantic_manifest_summary(
	manifest: str, secret_markers: tuple[str, ...]
) -> dict[str, object]:
	rows = json.loads(manifest)
	if not isinstance(rows, list):
		raise TestFailure("semantic manifest component is not a row array")
	grouped_counts: Counter[str] = Counter()
	for row in rows:
		if (
			not isinstance(row, list) or len(row) != 3
			or not isinstance(row[0], str)
		):
			raise TestFailure("semantic manifest row is not a kind/identity/contract tuple")
		grouped_counts[row[0]] += 1
	return {
		"grouped_kind_counts": [
			{
				"count": count,
				"kind": bounded_evidence_value(kind, secret_markers),
			}
			for kind, count in sorted(grouped_counts.items())
		],
		"row_count": len(rows),
		"sha256": hashlib.sha256(manifest.encode("utf-8")).hexdigest(),
	}


def semantic_row_sample(
	row: object, secret_markers: tuple[str, ...]
) -> dict[str, object]:
	if not isinstance(row, list) or len(row) != 3 or not isinstance(row[0], str):
		raise TestFailure("semantic manifest row is not a kind/identity/contract tuple")
	dependency = decode_dependency_manifest_row(row)
	if dependency is not None:
		(
			source_kind,
			source_identity,
			dependency_type,
			reference_class,
			reference_key,
			resolved,
		) = dependency
		return {
			"dependency_type": bounded_evidence_value(
				dependency_type, secret_markers
			),
			"reference_class": bounded_evidence_value(
				reference_class, secret_markers
			),
			"reference_key": bounded_evidence_value(
				reference_key, secret_markers
			),
			"resolved": resolved,
			"source_identity": bounded_evidence_value(
				source_identity, secret_markers
			),
			"source_kind": bounded_evidence_value(source_kind, secret_markers),
		}
	return {
		"identity": bounded_evidence_value(row[1], secret_markers),
		"kind": bounded_evidence_value(row[0], secret_markers),
	}


def semantic_contract_mismatch_sample(
	mismatch: object, secret_markers: tuple[str, ...]
) -> dict[str, object]:
	if not isinstance(mismatch, dict) or set(mismatch) != {
		"kind", "identity", "before_contract", "after_contract"
	}:
		raise TestFailure("semantic manifest contract mismatch is malformed")
	before = semantic_row_sample([
		mismatch["kind"], mismatch["identity"], mismatch["before_contract"]
	], secret_markers)
	after = semantic_row_sample([
		mismatch["kind"], mismatch["identity"], mismatch["after_contract"]
	], secret_markers)
	if "source_kind" in before:
		return {
			**{key: value for key, value in before.items() if key != "resolved"},
			"after_resolved": after["resolved"],
			"before_resolved": before["resolved"],
		}
	if mismatch["kind"] == "constraint":
		changed_fields = []
		for field, before_value, after_value in constraint_contract_changes(mismatch):
			if field == "definition":
				changed_fields.append(constraint_definition_change(
					before_value, after_value, secret_markers
				))
			else:
				changed_fields.append({
					"after": bounded_evidence_value(after_value, secret_markers),
					"before": bounded_evidence_value(before_value, secret_markers),
					"field": field,
				})
		return {
			"changed_fields": changed_fields,
			"identity": before["identity"],
			"kind": before["kind"],
		}
	return {
		"after_redacted_sha256": redacted_contract_sha256(
			mismatch["after_contract"], secret_markers
		),
		"before_redacted_sha256": redacted_contract_sha256(
			mismatch["before_contract"], secret_markers
		),
		"identity": before["identity"],
		"kind": before["kind"],
	}


def restore_parity_diagnostic(
	component: str,
	source_manifest: str,
	restored_manifest: str,
	diff: dict[str, list[object]],
	*,
	secret_markers: tuple[str, ...],
) -> str:
	if component not in {"schema", "authority"}:
		raise TestFailure("restore parity diagnostic component is invalid")
	changes: dict[str, object] = {}
	for category in ("before_only", "after_only", "contract_mismatches"):
		field_counts: Counter[str] = Counter()
		if category == "contract_mismatches":
			for mismatch in diff[category]:
				if isinstance(mismatch, dict) and mismatch.get("kind") == "constraint":
					field_counts.update(
						field for field, _, _ in constraint_contract_changes(mismatch)
					)
		projected = [
			(
				semantic_contract_mismatch_sample(row, secret_markers)
				if category == "contract_mismatches"
				else semantic_row_sample(row, secret_markers)
			)
			for row in diff[category]
		]
		projected.sort(
			key=lambda sample: json.dumps(
				sample, sort_keys=True, separators=(",", ":")
			)
		)
		category_changes: dict[str, object] = {
			"count": len(projected),
			"samples": projected[:MANIFEST_DIAGNOSTIC_EVIDENCE_LIMIT],
			"truncated": len(projected) > MANIFEST_DIAGNOSTIC_EVIDENCE_LIMIT,
		}
		if field_counts:
			category_changes["constraint_field_change_counts"] = [
				{"count": field_counts[field], "field": field}
				for field in CONSTRAINT_CONTRACT_FIELDS
				if field in field_counts
			]
		changes[category] = category_changes
	diagnostic = {
		"changes": changes,
		"component": component,
		"restored": semantic_manifest_summary(restored_manifest, secret_markers),
		"schema": RESTORE_PARITY_DIAGNOSTIC_SCHEMA,
		"source": semantic_manifest_summary(source_manifest, secret_markers),
	}
	serialized = json.dumps(diagnostic, sort_keys=True, separators=(",", ":"))
	if any(marker and marker in serialized for marker in secret_markers):
		raise TestFailure("restore parity diagnostic redaction failed")
	return serialized


def require_restore_parity(
	component: str,
	source_manifest: str,
	restored_manifest: str,
	*,
	secret_markers: tuple[str, ...],
) -> dict[str, list[object]]:
	diff = semantic_row_diff(source_manifest, restored_manifest)
	if any(diff.values()):
		try:
			diagnostic = restore_parity_diagnostic(
				component,
				source_manifest,
				restored_manifest,
				diff,
				secret_markers=secret_markers,
			)
		except Exception:
			diagnostic = json.dumps({
				"classification": "diagnostic_unavailable",
				"schema": RESTORE_PARITY_DIAGNOSTIC_SCHEMA,
			}, sort_keys=True, separators=(",", ":"))
		raise TestFailure(
			"authority candidate restore parity diagnostic: "
			+ diagnostic
		)
	return diff


def require_restore_checkpoint_parity(
	checkpoint: str,
	component: str,
	before_manifest: str,
	after_manifest: str,
	*,
	secret_markers: tuple[str, ...],
) -> dict[str, list[object]]:
	if checkpoint not in {edge[0] for edge in AUTHORITY_CAPTURE_RESTORE_EDGES}:
		raise TestFailure("authority candidate restore checkpoint is invalid")
	try:
		return require_restore_parity(
			component,
			before_manifest,
			after_manifest,
			secret_markers=secret_markers,
		)
	except TestFailure as error:
		raise TestFailure(
			f"authority candidate restore checkpoint {checkpoint} failed: {error}"
		) from error


def semantic_state_diff(before: list[object], after: list[object]) -> dict[str, list[object]]:
	encode = lambda row: json.dumps(row, sort_keys=True, separators=(",", ":"))
	before_counter = Counter(encode(row) for row in before)
	after_counter = Counter(encode(row) for row in after)
	return {
		"before_only": [
			json.loads(row)
			for row, count in sorted((before_counter - after_counter).items())
			for _ in range(count)
		],
		"after_only": [
			json.loads(row)
			for row, count in sorted((after_counter - before_counter).items())
			for _ in range(count)
		],
	}


def capture_restore_checkpoint(
	report: dict[str, object],
	name: str,
	path: Path,
	database: str,
	env: dict[str, str],
) -> str:
	checkpoints = report["checkpoints"]
	assert isinstance(checkpoints, dict)
	try:
		output = dump_schema_manifest(path, database, env)
		checkpoints[name] = load_semantic_manifest(path)
		record_restore_stage(report, f"{name}_capture", "passed")
		return output
	except TestFailure as error:
		try:
			checkpoints[name] = (
				load_semantic_manifest(path)
				if path.is_file()
				else unavailable_checkpoint(str(error))
			)
		except Exception as artifact_error:
			raise HarnessCorruption(
				f"checkpoint report artifact is invalid: {artifact_error}"
			)
		record_restore_stage(report, f"{name}_capture", "failed", error=str(error))
		return ""


RESTORE_STAGE_DEPENDENCIES = {
	"role_profile_restored_capture": ("role_profile_restore",),
	"role_profile_restored_check": ("role_profile_restore",),
	"runtime_session_restored_check": ("runtime_session_restore",),
	"primary_restored_capture": ("primary_restore",),
	"primary_store_restored_check": ("primary_restore",),
	"managed_repository_restored_check": ("primary_restore",),
}


def record_restore_stage(
	report: dict[str, object],
	name: str,
	status: str,
	*,
	error: str | None = None,
	blocked_by: list[str] | None = None,
) -> None:
	stages = report["stages"]
	assert isinstance(stages, dict)
	result: dict[str, object] = {"status": status}
	if error is not None:
		result["error"] = error
	if blocked_by:
		result["blocked_by"] = blocked_by
	stages[name] = result


def restore_stage_ready(report: dict[str, object], name: str) -> bool:
	stages = report["stages"]
	assert isinstance(stages, dict)
	blocked_by = [
		dependency
		for dependency in RESTORE_STAGE_DEPENDENCIES.get(name, ())
		if not isinstance(stages.get(dependency), dict)
		or stages[dependency].get("status") != "passed"
	]
	if blocked_by:
		record_restore_stage(report, name, "unavailable", blocked_by=blocked_by)
		return False
	return True


def record_restore_production_check(
	report: dict[str, object], name: str, action: Callable[[], str]
) -> str:
	checks = report["production_checks"]
	assert isinstance(checks, dict)
	stage_name = f"{name}_check"
	if not restore_stage_ready(report, stage_name):
		checks[name] = {"status": "unavailable"}
		return ""
	try:
		output = action()
		checks[name] = {"status": "passed"}
		record_restore_stage(report, stage_name, "passed")
		return output
	except TestFailure as error:
		checks[name] = {"status": "failed", "error": str(error)}
		record_restore_stage(report, stage_name, "failed", error=str(error))
		return ""


def finalize_restore_report(report: dict[str, object]) -> list[str]:
	checkpoints = report["checkpoints"]
	assert isinstance(checkpoints, dict)
	expected = {
		"schema": rust_digest_constant("SCHEMA_CONTRACT_SHA256"),
		"authority": rust_digest_constant("CONFIGURED_AUTHORITY_SHA256"),
	}
	diagnostics: dict[str, object] = {
		"expected_digests": expected,
		"checkpoints": {},
		"comparisons": {},
		"stages": report["stages"],
		"production_checks": report["production_checks"],
	}
	stages = report["stages"]
	assert isinstance(stages, dict)
	failures = [
		f"restore stage {name} is {result.get('status')}"
		for name, result in stages.items()
		if result.get("status") != "passed"
	]
	checkpoint_diagnostics = diagnostics["checkpoints"]
	assert isinstance(checkpoint_diagnostics, dict)
	for name, document in checkpoints.items():
		assert isinstance(document, dict)
		component_diagnostics: dict[str, object] = {}
		checkpoint_diagnostics[name] = {
			"binding": document.get("binding"),
			"components": component_diagnostics,
			"sequence_state": document.get("sequence_state"),
		}
		for component_name in ("schema", "authority"):
			component = document[component_name]
			assert isinstance(component, dict)
			manifest = component.get("manifest")
			result = {
				"available": component.get("available"),
				"complete": component.get("complete"),
				"error": component.get("error"),
			}
			if isinstance(manifest, str):
				result["rows"] = len(json.loads(manifest))
				if component_name == "schema":
					result["unresolved_dependencies"] = unresolved_dependency_rows(manifest)
			if component.get("available") and component.get("complete"):
				assert isinstance(manifest, str)
				digest = hashlib.sha256(manifest.encode("utf-8")).hexdigest()
				result["digest"] = digest
				result["matches_shipped"] = digest == expected[component_name]
				if digest != expected[component_name]:
					failures.append(
						f"restore checkpoint {name} {component_name} differs from shipped digest"
					)
			elif component.get("available"):
				failures.append(f"restore checkpoint {name} {component_name} is incomplete")
			else:
				failures.append(f"restore checkpoint {name} {component_name} is unavailable")
			component_diagnostics[component_name] = result
	comparisons = diagnostics["comparisons"]
	assert isinstance(comparisons, dict)
	for name, before_name, after_name, compare_state in (
		("source_to_role_profile_post_command", "source", "role_profile_post_command", False),
		("role_profile_restore", "role_profile_post_command", "role_profile_restored", True),
		("source_to_primary_post_command", "source", "primary_post_command", False),
		("primary_restore", "primary_post_command", "primary_restored", True),
	):
		before = checkpoints.get(before_name, unavailable_checkpoint("checkpoint not collected"))
		after = checkpoints.get(after_name, unavailable_checkpoint("checkpoint not collected"))
		assert isinstance(before, dict) and isinstance(after, dict)
		comparison: dict[str, object] = {}
		for component_name in ("schema", "authority"):
			before_manifest = component_manifest(before, component_name)
			after_manifest = component_manifest(after, component_name)
			if before_manifest is None or after_manifest is None:
				comparison[component_name] = {
					"unavailable": [
						checkpoint
						for checkpoint, manifest in (
							(before_name, before_manifest), (after_name, after_manifest)
						)
						if manifest is None
					]
				}
				failures.append(f"restore comparison {name} {component_name} is unavailable")
			else:
				comparison[component_name] = semantic_row_diff(before_manifest, after_manifest)
		if compare_state and isinstance(before.get("sequence_state"), list) and isinstance(
			after.get("sequence_state"), list
		):
			comparison["sequence_state"] = semantic_state_diff(
				before["sequence_state"], after["sequence_state"]
			)
		elif compare_state:
			comparison["sequence_state"] = {"unavailable": True}
			failures.append(f"restore comparison {name} sequence state is unavailable")
		comparisons[name] = comparison
		for component_name, value in comparison.items():
			if isinstance(value, dict) and (
				value.get("before_only") or value.get("after_only")
				or value.get("contract_mismatches")
			):
				failures.append(
					f"restore comparison {name} changed {component_name} evidence"
				)
	checks = report["production_checks"]
	assert isinstance(checks, dict)
	for name, result in checks.items():
		if result.get("status") != "passed":
			failures.append(f"production restore check {name} failed")
	print(
		"XY-1353 structured PostgreSQL restore report:\n"
		+ json.dumps(diagnostics, indent=2, sort_keys=True),
		file=sys.stderr,
		flush=True,
	)
	return failures


def manifest_diagnostics(
	checkpoints: dict[str, dict[str, object] | None],
) -> tuple[dict[str, object], list[str]]:
	shipped_expected = {
		"schema": rust_digest_constant("SCHEMA_CONTRACT_SHA256"),
		"authority": rust_digest_constant("CONFIGURED_AUTHORITY_SHA256"),
	}
	actual = {
		checkpoint: None if manifest is None else {
			component: None if component_manifest(manifest, component) is None else hashlib.sha256(
				component_manifest(manifest, component).encode("utf-8")
			).hexdigest()
			for component in ("schema", "authority")
		}
		for checkpoint, manifest in checkpoints.items()
	}
	missing_checkpoints = [
		checkpoint for checkpoint, manifest in checkpoints.items() if manifest is None
	]
	component_results: dict[str, object] = {}
	manifest_failures = [
		f"missing semantic manifest checkpoint: {checkpoint}"
		for checkpoint in missing_checkpoints
	]
	for component in ("schema", "authority"):
		matches_shipped = {
			checkpoint: None if manifest is None else manifest[component] == shipped_expected[component]
			for checkpoint, manifest in actual.items()
		}
		available_values = [
			manifest[component] for manifest in actual.values() if manifest is not None
		]
		available_equal = len(set(available_values)) <= 1
		checkpoints_equal = (
			len(available_values) == len(checkpoints)
			and available_equal
		)
		component_results[component] = {
			"matches_shipped": matches_shipped,
			"checkpoints_equal": checkpoints_equal,
		}
		for checkpoint, matches in matches_shipped.items():
			if matches is False:
				manifest_failures.append(
					f"{checkpoint} {component} digest differs from the shipped singleton"
				)
		if len(available_values) > 1 and not available_equal:
			manifest_failures.append(
				f"{component} manifest changed across available checkpoints"
			)
	diagnostics: dict[str, object] = {
		"shipped_expected": shipped_expected,
		"actual": actual,
		"missing_checkpoints": missing_checkpoints,
		"component_results": component_results,
		"semantic_row_diffs": {},
	}
	diffs = diagnostics["semantic_row_diffs"]
	assert isinstance(diffs, dict)
	for comparison, before_name, after_name in (
		("baseline_to_post_attempt", "baseline", "post_attempt"),
		("post_attempt_to_restored", "post_attempt", "restored"),
		("baseline_to_restored", "baseline", "restored"),
	):
		diffs[comparison] = {}
		for component in ("schema", "authority"):
			before = checkpoints[before_name]
			after = checkpoints[after_name]
			before_manifest = None if before is None else component_manifest(before, component)
			after_manifest = None if after is None else component_manifest(after, component)
			if before_manifest is None or after_manifest is None:
				diffs[comparison][component] = {
					"unavailable": [
						name for name, manifest in (
							(before_name, before_manifest), (after_name, after_manifest)
						)
						if manifest is None
					]
				}
			else:
				diffs[comparison][component] = semantic_row_diff(
					before_manifest, after_manifest
				)
		if before is not None and after is not None and isinstance(
			before["sequence_state"], list
		) and isinstance(after["sequence_state"], list):
			state_diff = semantic_state_diff(
				before["sequence_state"], after["sequence_state"]
			)
			diffs[comparison]["sequence_state"] = state_diff
			if comparison == "post_attempt_to_restored" and (
				state_diff["before_only"] or state_diff["after_only"]
			):
				manifest_failures.append(
					f"sequence state changed across {comparison}"
				)
	return diagnostics, manifest_failures


def run_work_item_focused_contracts(
	socket_dir: Path, port: int, work: Path, env: dict[str, str]
) -> str:
	create_database(WORK_ITEM_DATABASE, env)
	set_contract_urls(env, socket_dir, port, WORK_ITEM_DATABASE, RUNTIME_ROLE)
	migration_output = run_migration(env)
	provision_runtime(WORK_ITEM_DATABASE, RUNTIME_ROLE, env)
	baseline_path = work / "xy1343-baseline-manifest.json"
	post_attempt_path = work / "xy1343-post-attempt-manifest.json"
	restored_path = work / "xy1343-restored-manifest.json"
	checkpoints: dict[str, dict[str, str] | None] = {
		"baseline": None,
		"post_attempt": None,
		"restored": None,
	}
	stage_failures: list[str] = []
	command_output = ""
	source_behavior_error: Exception | None = None
	restore_output = ""

	try:
		dump_schema_manifest(baseline_path, WORK_ITEM_DATABASE, env)
		checkpoints["baseline"] = load_semantic_manifest(baseline_path)
	except TestFailure as error:
		stage_failures.append(f"baseline manifest capture failed:\n{error}")

	if checkpoints["baseline"] is not None:
		try:
			command_output = run_work_item_test("postgres_exact_work_item_commands", env)
		except TestFailure as error:
			source_behavior_error = error
	else:
		source_behavior_error = TestFailure(
			"source behavior was not run because the baseline manifest is unavailable"
		)

	try:
		dump_schema_manifest(post_attempt_path, WORK_ITEM_DATABASE, env)
		checkpoints["post_attempt"] = load_semantic_manifest(post_attempt_path)
	except TestFailure as error:
		stage_failures.append(f"post-attempt manifest capture failed:\n{error}")

	dump_path = work / "xy1343-work-items.dump"
	dump_succeeded = False
	restore_database_created = False
	try:
		run(["pg_dump", "-Fc", "-f", str(dump_path), WORK_ITEM_DATABASE], env)
		dump_succeeded = True
	except TestFailure as error:
		stage_failures.append(f"post-attempt pg_dump failed:\n{error}")
	if dump_succeeded:
		try:
			create_database(WORK_ITEM_RESTORE_DATABASE, env)
			restore_database_created = True
		except TestFailure as error:
			stage_failures.append(f"restore database creation failed:\n{error}")
	if restore_database_created:
		try:
			run(
				[
					"pg_restore", "--exit-on-error", "-d", WORK_ITEM_RESTORE_DATABASE,
					str(dump_path),
				],
				env,
			)
		except TestFailure as error:
			stage_failures.append(f"post-attempt pg_restore failed:\n{error}")
		set_contract_urls(env, socket_dir, port, WORK_ITEM_RESTORE_DATABASE, RUNTIME_ROLE)
		try:
			dump_schema_manifest(restored_path, WORK_ITEM_RESTORE_DATABASE, env)
			checkpoints["restored"] = load_semantic_manifest(restored_path)
		except TestFailure as error:
			stage_failures.append(f"restored manifest capture failed:\n{error}")

	diagnostics, manifest_failures = manifest_diagnostics(checkpoints)
	print(
		"XY-1343 V11 semantic manifest diagnostics:\n"
		+ json.dumps(diagnostics, indent=2, sort_keys=True),
		file=sys.stderr,
		flush=True,
	)

	failures = list(stage_failures)
	if source_behavior_error is not None:
		failures.append(f"source verifier/behavior failed:\n{source_behavior_error}")
	failures.extend(manifest_failures)
	if not failures:
		try:
			restore_output = run_work_item_test("postgres_exact_work_item_restore", env)
		except TestFailure as error:
			failures.append(f"restored verifier/behavior failed:\n{error}")
	if failures:
		raise TestFailure(
			"XY-1343 focused evidence finalized with failures:\n\n"
			+ "\n\n".join(failures)
		)
	return "\n".join((migration_output, command_output, restore_output))


def run_managed_run_focused_contracts(
	socket_dir: Path, port: int, work: Path, env: dict[str, str]
) -> str:
	static_output = run(
		[
			"python3", "-m", "unittest",
			"tests/scripts/test_managed_run_authority.py",
		],
		env,
	)
	create_database(MANAGED_RUN_DATABASE, env)
	set_contract_urls(env, socket_dir, port, MANAGED_RUN_DATABASE, RUNTIME_ROLE)
	migration_output = run_migration(env)
	provision_runtime(MANAGED_RUN_DATABASE, RUNTIME_ROLE, env)
	paths = {
		"baseline": work / "xy1338-baseline-manifest.json",
		"post_attempt": work / "xy1338-post-attempt-manifest.json",
		"restored": work / "xy1338-restored-manifest.json",
	}
	checkpoints: dict[str, dict[str, str] | None] = dict.fromkeys(paths)
	stage_failures: list[str] = []
	command_output = ""
	restore_output = ""
	source_behavior_error: Exception | None = None

	try:
		dump_schema_manifest(paths["baseline"], MANAGED_RUN_DATABASE, env)
		checkpoints["baseline"] = load_semantic_manifest(paths["baseline"])
	except TestFailure as error:
		stage_failures.append(f"baseline manifest capture failed:\n{error}")
	if checkpoints["baseline"] is not None:
		try:
			command_output = run_managed_run_test(
				"postgres_managed_run_safety_contract", env
			)
		except TestFailure as error:
			source_behavior_error = error
	else:
		source_behavior_error = TestFailure(
			"source behavior was not run because the baseline manifest is unavailable"
		)
	try:
		dump_schema_manifest(paths["post_attempt"], MANAGED_RUN_DATABASE, env)
		checkpoints["post_attempt"] = load_semantic_manifest(paths["post_attempt"])
	except TestFailure as error:
		stage_failures.append(f"post-attempt manifest capture failed:\n{error}")

	dump_path = work / "xy1338-managed-runs.dump"
	dump_succeeded = False
	restore_database_created = False
	try:
		run(["pg_dump", "-Fc", "-f", str(dump_path), MANAGED_RUN_DATABASE], env)
		dump_succeeded = True
	except TestFailure as error:
		stage_failures.append(f"post-attempt pg_dump failed:\n{error}")
	if dump_succeeded:
		try:
			create_database(MANAGED_RUN_RESTORE_DATABASE, env)
			restore_database_created = True
		except TestFailure as error:
			stage_failures.append(f"restore database creation failed:\n{error}")
	if restore_database_created:
		try:
			run(
				["pg_restore", "--exit-on-error", "-d", MANAGED_RUN_RESTORE_DATABASE,
				 str(dump_path)],
				env,
			)
		except TestFailure as error:
			stage_failures.append(f"post-attempt pg_restore failed:\n{error}")
		set_contract_urls(env, socket_dir, port, MANAGED_RUN_RESTORE_DATABASE, RUNTIME_ROLE)
		try:
			dump_schema_manifest(paths["restored"], MANAGED_RUN_RESTORE_DATABASE, env)
			checkpoints["restored"] = load_semantic_manifest(paths["restored"])
		except TestFailure as error:
			stage_failures.append(f"restored manifest capture failed:\n{error}")

	diagnostics, manifest_failures = manifest_diagnostics(checkpoints)
	print(
		"XY-1338 V12 semantic manifest diagnostics:\n"
		+ json.dumps(diagnostics, indent=2, sort_keys=True),
		file=sys.stderr,
		flush=True,
	)
	failures = list(stage_failures)
	if source_behavior_error is not None:
		failures.append(f"source verifier/behavior failed:\n{source_behavior_error}")
	failures.extend(manifest_failures)
	if not failures:
		try:
			restore_output = run_managed_run_test(
				"postgres_managed_run_safety_restore", env
			)
		except TestFailure as error:
			failures.append(f"restored verifier/behavior failed:\n{error}")
	if failures:
		raise TestFailure(
			"XY-1338 focused evidence finalized with failures:\n\n"
			+ "\n\n".join(failures)
		)
	return "\n".join((static_output, migration_output, command_output, restore_output))


def run_runtime_session_crash_recovery(
	data_dir: Path,
	log_path: Path,
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
) -> str:
	sync = work / "runtime-session-crash-recovery"
	sync.mkdir()
	test_env = env.copy()
	test_env["DECODEX_RUNTIME_SESSION_RESTART_SYNC"] = str(sync)
	process = subprocess.Popen(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			"runtime_sessions::postgres_exact_runtime_session_crash_recovery", "--exact",
		],
		text=True,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=test_env,
		cwd=REPO_ROOT,
	)
	try:
		deadline = time.monotonic() + 30
		while time.monotonic() < deadline:
			if (sync / "ready").exists():
				break
			if process.poll() is not None:
				stdout, stderr = process.communicate()
				raise TestFailure(
					f"RuntimeSession crash fixture exited early\n{stdout}\n{stderr}"
				)
			time.sleep(0.02)
		else:
			raise TestFailure("RuntimeSession crash fixture did not reach its lock barrier")
		run(["pg_ctl", "-D", str(data_dir), "-m", "immediate", "-w", "stop"], env)
		run(
			[
				"pg_ctl", "-D", str(data_dir), "-l", str(log_path), "-o",
				f"-k {socket_dir} -p {port} -h '' -F", "-w", "start",
			],
			env,
		)
		(sync / "restarted").write_text("restarted", encoding="utf-8")
		stdout, stderr = process.communicate(timeout=60)
		if process.returncode != 0:
			raise TestFailure(f"RuntimeSession crash/recovery failed\n{stdout}\n{stderr}")
		return stdout.strip() or stderr.strip()
	finally:
		if process.poll() is None:
			process.terminate()
			process.wait(timeout=10)


def run_runtime_session_final_gate_contracts(
	data_dir: Path,
	log_path: Path,
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
	restore_report: dict[str, object],
) -> str:
	outputs: list[str] = []
	for database, test in (
		(RUNTIME_SESSION_COMMAND_DATABASE, "postgres_exact_runtime_session_commands"),
		(RUNTIME_SESSION_ROLLBACK_DATABASE, "postgres_exact_runtime_session_atomic_rollback"),
		(RUNTIME_SESSION_RETRY_DATABASE, "postgres_exact_runtime_session_retry_convergence"),
	):
		prepare_role_profile_database(database, socket_dir, port, env)
		outputs.append(run_runtime_session_test(test, env))

	create_database(RUNTIME_SESSION_UPGRADE_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, RUNTIME_SESSION_UPGRADE_DATABASE, RUNTIME_ROLE
	)
	outputs.append(run_runtime_session_test("postgres_v9_to_v10_runtime_session_upgrade", env))
	create_database(RUNTIME_SESSION_FENCE_DATABASE, env)
	set_contract_urls(env, socket_dir, port, RUNTIME_SESSION_FENCE_DATABASE, RUNTIME_ROLE)
	outputs.append(
		run_runtime_session_test("postgres_v10_fences_blocked_old_runtime_writer", env)
	)
	for variant in (
		"profile_snapshot", "account_snapshot", "runtime_session", "legacy_receipt",
		"exact_receipt", "activity", "activity_nested_aggregate",
		"activity_legacy_other_aggregate", "outbox", "outbox_nested_effect",
		"outbox_activity_link",
	):
		database = f"decodex_xy1337_v10_{variant}"
		create_database(database, env)
		set_contract_urls(env, socket_dir, port, database, RUNTIME_ROLE)
		variant_env = env.copy()
		variant_env["DECODEX_V10_PRIOR_STATE"] = variant
		outputs.append(
			run_runtime_session_test(
				"postgres_v10_rejects_classified_runtime_state", variant_env
			)
		)

	prepare_role_profile_database(RUNTIME_SESSION_CRASH_DATABASE, socket_dir, port, env)
	outputs.append(
		run_runtime_session_crash_recovery(
			data_dir, log_path, socket_dir, port, work, env
		)
	)

	prepare_role_profile_database(
		RUNTIME_SESSION_RESTORE_SOURCE_DATABASE, socket_dir, port, env
	)
	outputs.append(run_runtime_session_test("postgres_exact_runtime_session_commands", env))
	restore_dump = work / "xy1337-runtime-sessions.dump"
	restore_ready = True
	try:
		run(
			[
				"pg_dump", "-Fc", "-f", str(restore_dump),
				RUNTIME_SESSION_RESTORE_SOURCE_DATABASE,
			],
			env,
		)
		create_database(RUNTIME_SESSION_RESTORE_DATABASE, env)
		run(
			[
				"pg_restore", "--exit-on-error", "-d", RUNTIME_SESSION_RESTORE_DATABASE,
				str(restore_dump),
			],
			env,
		)
	except TestFailure as error:
		restore_ready = False
		record_restore_stage(
			restore_report, "runtime_session_restore", "failed", error=str(error)
		)
	else:
		record_restore_stage(restore_report, "runtime_session_restore", "passed")
	if restore_ready:
		set_contract_urls(
			env, socket_dir, port, RUNTIME_SESSION_RESTORE_DATABASE, RUNTIME_ROLE
		)
		outputs.append(record_restore_production_check(
			restore_report,
			"runtime_session_restored",
			lambda: run_runtime_session_test("postgres_exact_runtime_session_restore", env),
		))
	else:
		record_restore_production_check(
			restore_report,
			"runtime_session_restored",
			lambda: run_runtime_session_test("postgres_exact_runtime_session_restore", env),
		)
	set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
	return "\n".join(outputs)


def quota_authority_snapshot(database: str, env: dict[str, str]) -> str:
	return psql(
		database,
		"SELECT jsonb_build_object("
		"'windows',coalesce((SELECT jsonb_agg(to_jsonb(row) ORDER BY row.account_id,row.window_class) "
		"FROM (SELECT account_id::text,window_class::text,duration_minutes,remaining_percent,"
		"(extract(epoch FROM resets_at)::numeric*1000000)::bigint AS resets_at_micros,"
		"(extract(epoch FROM observed_at)::numeric*1000000)::bigint AS observed_at_micros,"
		"confidence::text,metadata,revision,updated_at FROM decodex.quota_windows) AS row),'[]'),"
		"'exclusions',coalesce((SELECT jsonb_agg(to_jsonb(row) ORDER BY row.account_id,row.window_class) "
		"FROM (SELECT account_id::text,window_class::text,duration_minutes,observation_revision,"
		"remaining_percent,confidence::text,observation_metadata,observed_at_micros,resets_at_micros,"
		"excluded_at_micros,maximum_age_micros,mutation_sha256,mutation_length,dispatch_enabled,"
		"created_at FROM decodex.quota_exclusions) AS row),'[]'),"
		"'receipts',coalesce((SELECT jsonb_agg(to_jsonb(row) ORDER BY row.idempotency_key) "
		"FROM (SELECT idempotency_key,request_hash,operation,scope_id,entity_id,expected_revision,"
		"payload_hash,payload_length,receipt_state::text,response,encode(response_bytes,'hex') AS response_hex,"
		"created_at,completed_at FROM decodex.command_receipts WHERE operation IN "
		"('mutate_quota_window','persist_quota_exclusion') OR scope_id IN ('quota_windows','quota_exclusions')) AS row),'[]'),"
		"'activity',coalesce((SELECT jsonb_agg(to_jsonb(row) ORDER BY row.sequence) "
		"FROM (SELECT sequence,aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload,created_at "
		"FROM decodex.activity WHERE aggregate_kind='quota_window') AS row),'[]'),"
		"'outbox',coalesce((SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id) FROM "
		"(SELECT work.id,work.effect_key,work.aggregate_kind,work.aggregate_id,work.aggregate_revision,"
		"work.payload,work.state::text,work.effect_state::text,work.created_at FROM decodex.outbox AS work "
		"JOIN decodex.activity AS event ON work.payload @> jsonb_build_object('activity_sequence',event.sequence) "
		"WHERE event.aggregate_kind='quota_window') AS row),'[]'))",
		env,
	)


def run_v8_migration_boundary_contracts(
	env: dict[str, str], socket_dir: Path, port: int
) -> str:
	outputs: list[str] = []
	tests = ((V8_EMPTY_DATABASE, "postgres_v8_empty_boundary_contract"),
	         (V8_LOCK_DATABASE, "postgres_v8_fences_concurrent_prior_writer"))
	for database, test in tests:
		create_database(database, env)
		set_contract_urls(env, socket_dir, port, database, RUNTIME_ROLE)
		outputs.append(run(
			["cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			 "test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			 test, "--exact"],
			env,
		))

	for variant in (
		"quota_row", "receipt_operation", "receipt_scope", "receipt_completed",
		"activity_aggregate", "activity_event", "activity_payload_window",
		"activity_payload_kind", "activity_payload_seconds", "activity_payload_minutes",
		"outbox_aggregate", "outbox_envelope", "outbox_envelope_aggregate",
		"outbox_envelope_event", "outbox_envelope_kind", "outbox_envelope_window",
		"outbox_envelope_seconds", "outbox_link", "outbox_orphan",
	):
		database = f"decodex_xy1274_v8_{variant}"
		create_database(database, env)
		set_contract_urls(env, socket_dir, port, database, RUNTIME_ROLE)
		variant_env = env.copy()
		variant_env["DECODEX_V8_PRIOR_STATE"] = variant
		outputs.append(run(
			["cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			 "test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			 "postgres_v8_rejects_classified_prior_state", "--exact"],
			variant_env,
		))

	return "\n".join(outputs)


def provision_runtime(database: str, role: str, env: dict[str, str]) -> None:
	execute_signatures = ", ".join(RUNTIME_EXECUTE_SIGNATURES)
	trigger_signatures = ", ".join(TRIGGER_ONLY_SIGNATURES)
	type_names = ", ".join(RUNTIME_TYPE_NAMES)

	psql(
		database,
		f"GRANT CONNECT ON DATABASE {database} TO {role}; "
		f"GRANT USAGE ON SCHEMA public, decodex TO {role}; "
		f"GRANT SELECT ON TABLE public.refinery_schema_history TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE ON TABLE "
		f"decodex.accounts, decodex.quota_windows, decodex.command_receipts, "
		f"decodex.leases, decodex.conversations, "
		f"decodex.artifacts, decodex.turns, decodex.history_items TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.quota_exclusions TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.blob_objects, decodex.artifact_revisions, "
		f"decodex.context_packs, "
		f"decodex.context_pack_sources, decodex.transition_proposals TO {role}; "
		f"GRANT SELECT ON TABLE decodex.history_cursors, decodex.history_item_versions, "
		f"decodex.profile_snapshots, decodex.account_snapshots, "
		f"decodex.runtime_sessions TO {role}; "
		f"GRANT SELECT ON TABLE decodex.projects, decodex.agents, "
		f"decodex.policies, decodex.policy_revisions TO {role}; "
		f"GRANT SELECT ON TABLE decodex.programs, decodex.objectives, "
		f"decodex.objective_completion_evidence TO {role}; "
		f"GRANT SELECT ON TABLE decodex.work_items, decodex.work_item_objectives, "
		f"decodex.work_item_edges, decodex.work_item_readiness_blockers, "
		f"decodex.work_item_acceptances TO {role}; "
		f"GRANT SELECT ON TABLE decodex.managed_runs, decodex.managed_run_assignments, "
		f"decodex.managed_run_effect_barriers, decodex.managed_run_effects, "
		f"decodex.managed_run_submitted_turn_receipts, "
		f"decodex.managed_run_safety_inputs TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.repository_admissions, "
		f"decodex.repository_authority_transitions, decodex.repository_operations, "
		f"decodex.repository_operation_events, decodex.repository_operation_evidence, "
		f"decodex.repository_operation_results TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE ON TABLE decodex.managed_repositories TO {role}; "
		f"GRANT DELETE ON TABLE decodex.blob_objects TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.activity TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decodex.outbox TO {role}; "
		f"GRANT USAGE ON SEQUENCE decodex.activity_sequence_seq, decodex.outbox_id_seq TO {role}; "
		f"GRANT USAGE ON TYPE {type_names} TO {role}; "
		f"GRANT EXECUTE ON FUNCTION {execute_signatures} TO {role}; "
		f"REVOKE ALL ON FUNCTION {trigger_signatures} FROM {role}, PUBLIC",
		env,
	)


def authority_manifest_evidence(manifest: str) -> dict[str, object]:
	rows = json.loads(manifest)
	if not isinstance(rows, list):
		raise TestFailure("authority candidate manifest is not a row array")
	grouped_counts: Counter[str] = Counter()
	identities: Counter[str] = Counter()
	decoded_identities: dict[str, list[object]] = {}
	for row in rows:
		if (
			not isinstance(row, list) or len(row) != 3
			or not isinstance(row[0], str)
		):
			raise TestFailure("authority candidate manifest row is malformed")
		grouped_counts[row[0]] += 1
		key = json.dumps(row[:2], sort_keys=True, separators=(",", ":"))
		identities[key] += 1
		decoded_identities[key] = row[:2]
	duplicates = [
		{
			"kind": decoded_identities[key][0],
			"identity": decoded_identities[key][1],
			"multiplicity": count,
		}
		for key, count in sorted(identities.items())
		if count > 1
	]
	return {
		"complete": True,
		"row_count": len(rows),
		"grouped_row_counts": dict(sorted(grouped_counts.items())),
		"duplicate_key_multiplicities": duplicates,
		"resolved": not unresolved_dependency_rows(manifest),
		"sha256": hashlib.sha256(manifest.encode("utf-8")).hexdigest(),
		"unique": not duplicates,
	}


def capture_migration_ledger(
	database: str, env: dict[str, str], *, through_version: int = 20
) -> list[object]:
	ledger = json.loads(psql(
		database,
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		"'version',version,'name',name,'checksum',checksum) ORDER BY version),"
		"'[]'::pg_catalog.json)::text FROM public.refinery_schema_history",
		env,
	))
	if not isinstance(ledger, list) or not ledger:
		raise TestFailure("authority candidate migration ledger is empty or malformed")
	actual_identity = []
	for row in ledger:
		if (
			not isinstance(row, dict)
			or not isinstance(row.get("version"), int)
			or not isinstance(row.get("name"), str)
			or not isinstance(row.get("checksum"), str)
			or not row["checksum"]
		):
			raise TestFailure("authority candidate migration ledger row is malformed")
		actual_identity.append((row["version"], row["name"]))
	expected_identity = []
	for path in (REPO_ROOT / "crates/decodex-postgres/migrations").glob("V*__*.sql"):
		match = re.fullmatch(r"V([1-9][0-9]*)__([a-z0-9_]+)\.sql", path.name)
		if match is None:
			raise TestFailure("embedded migration filename is malformed")
		expected_identity.append((int(match.group(1)), match.group(2)))
	expected_identity.sort()
	expected_identity = [
		identity for identity in expected_identity if identity[0] <= through_version
	]
	if actual_identity != expected_identity:
		raise TestFailure("authority candidate migration ledger differs from embedded source")
	return ledger


def capture_runtime_authority(database: str, env: dict[str, str]) -> dict[str, object]:
	probe = json.loads(psql(
		database,
		"SELECT pg_catalog.json_build_object("
		f"'database',pg_catalog.current_database(),'migration_role','{MIGRATION_ROLE}',"
		f"'runtime_role','{RUNTIME_ROLE}',"
		f"'non_default_runtime_role','{RUNTIME_ROLE}'<>'decodex_runtime',"
		f"'runtime_login',(SELECT rolcanlogin FROM pg_catalog.pg_roles WHERE rolname='{RUNTIME_ROLE}'),"
		f"'anchor_execute',pg_catalog.has_function_privilege('{RUNTIME_ROLE}',"
		"'decodex.apply_managed_run_safety_input_exact(text,text,uuid,uuid,bigint,"
		"decodex.managed_run_safety_input_kind,uuid,uuid,uuid)','EXECUTE'),"
		"'direct_non_grantable_execute_count',(SELECT pg_catalog.count(*) FROM "
		"pg_catalog.pg_proc AS procedure CROSS JOIN LATERAL "
		"pg_catalog.aclexplode(procedure.proacl) AS privilege WHERE "
		f"privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole AND "
		f"privilege.grantor='{MIGRATION_ROLE}'::pg_catalog.regrole AND "
		"privilege.privilege_type='EXECUTE' AND NOT privilege.is_grantable),"
		"'direct_non_grantable_type_usage_count',(SELECT pg_catalog.count(*) FROM "
		"pg_catalog.pg_type AS type CROSS JOIN LATERAL "
		"pg_catalog.aclexplode(type.typacl) AS privilege WHERE "
		f"privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole AND "
		f"privilege.grantor='{MIGRATION_ROLE}'::pg_catalog.regrole AND "
		"privilege.privilege_type='USAGE' AND NOT privilege.is_grantable))::text",
		env,
	))
	if not isinstance(probe, dict) or (
		probe.get("database") != database
		or probe.get("migration_role") != MIGRATION_ROLE
		or probe.get("runtime_role") != RUNTIME_ROLE
		or probe.get("non_default_runtime_role") is not True
		or probe.get("runtime_login") is not True
		or probe.get("anchor_execute") is not True
		or not isinstance(probe.get("direct_non_grantable_execute_count"), int)
		or probe["direct_non_grantable_execute_count"] <= 0
		or not isinstance(probe.get("direct_non_grantable_type_usage_count"), int)
		or probe["direct_non_grantable_type_usage_count"] <= 0
	):
		raise TestFailure("configured non-default runtime authority is not populated")
	return probe


def comparable_runtime_authority(probe: dict[str, object]) -> dict[str, object]:
	return {
		key: probe[key]
		for key in (
			"migration_role",
			"runtime_role",
			"non_default_runtime_role",
			"runtime_login",
			"anchor_execute",
			"direct_non_grantable_execute_count",
			"direct_non_grantable_type_usage_count",
		)
	}


def restore_edge_evidence(
	checkpoint: str,
	before_manifests: dict[str, str],
	after_manifests: dict[str, str],
	*,
	before_ledger: list[object],
	after_ledger: list[object],
	before_semantic_state: object,
	after_semantic_state: object,
	before_runtime_authority: dict[str, object],
	after_runtime_authority: dict[str, object],
	before_population: object,
	after_population: object,
	secret_markers: tuple[str, ...],
) -> dict[str, bool]:
	diffs = {
		component: require_restore_checkpoint_parity(
			checkpoint,
			component,
			before_manifests[component],
			after_manifests[component],
			secret_markers=secret_markers,
		)
		for component in ("schema", "authority")
	}
	evidence = {
		"schema_manifest": not any(diffs["schema"].values()),
		"configured_authority_manifest": not any(diffs["authority"].values()),
		"migration_ledger": before_ledger == after_ledger,
		"semantic_state": before_semantic_state == after_semantic_state,
		"runtime_authority_shape": (
			comparable_runtime_authority(before_runtime_authority)
			== comparable_runtime_authority(after_runtime_authority)
		),
		"populated_fixture": before_population == after_population,
	}
	if not all(evidence.values()):
		raise TestFailure(f"authority candidate restore checkpoint {checkpoint} is incomplete")
	return evidence


def capture_zero_grantee_migration_authority(
	database: str, env: dict[str, str]
) -> dict[str, object]:
	probe = json.loads(psql(
		database,
		"SELECT pg_catalog.json_build_object("
		"'database',pg_catalog.current_database(),"
		"'direct_function_acl_rows',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc "
		"AS procedure JOIN pg_catalog.pg_namespace AS namespace ON "
		"namespace.oid=procedure.pronamespace CROSS JOIN LATERAL "
		"pg_catalog.aclexplode(COALESCE(procedure.proacl,"
		"pg_catalog.acldefault('f',procedure.proowner))) AS privilege WHERE "
		"namespace.nspname='decodex' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'direct_type_acl_rows',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_type AS type "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(type.typacl,"
		"pg_catalog.acldefault('T',type.typowner))) AS privilege WHERE "
		"namespace.nspname='decodex' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole))::text",
		env,
	))
	if probe != {
		"database": database,
		"direct_function_acl_rows": 0,
		"direct_type_acl_rows": 0,
	}:
		raise TestFailure("fresh migration did not take the zero-grantee authority branch")
	return probe


def capture_upgrade_anchor_binding(database: str, env: dict[str, str]) -> dict[str, object]:
	rows = json.loads(psql(
		database,
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		f"'identity','{AUTHORITY_ANCHOR_SIGNATURE}',"
		"'catalog_identity',procedure.oid::pg_catalog.regprocedure::text,"
		"'grantor',pg_catalog.pg_get_userbyid(privilege.grantor),"
		"'grantee',pg_catalog.pg_get_userbyid(privilege.grantee),"
		"'is_grantable',privilege.is_grantable)),'[]'::pg_catalog.json)::text "
		"FROM pg_catalog.pg_proc AS procedure CROSS JOIN LATERAL "
		"pg_catalog.aclexplode(COALESCE(procedure.proacl,"
		"pg_catalog.acldefault('f',procedure.proowner))) AS privilege WHERE "
		f"procedure.oid=pg_catalog.to_regprocedure('{AUTHORITY_ANCHOR_SIGNATURE}') "
		"AND privilege.privilege_type='EXECUTE' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole",
		env,
	))
	if (
		not isinstance(rows, list)
		or len(rows) != 1
		or not isinstance(rows[0], dict)
		or rows[0].get("identity") != AUTHORITY_ANCHOR_SIGNATURE
		or rows[0].get("grantor") != MIGRATION_ROLE
		or rows[0].get("grantee") != RUNTIME_ROLE
		or rows[0].get("is_grantable") is not False
	):
		raise TestFailure("V13 upgrade anchor binding is not direct and non-grantable")
	return rows[0]


def capture_upgrade_runtime_authority(database: str, env: dict[str, str]) -> dict[str, object]:
	allowed_function_identities = tuple(sorted((
		AUTHORITY_ANCHOR_SIGNATURE,
		*UPGRADE_RUNTIME_EXECUTE_SIGNATURES,
	)))
	function_values = ",".join(
		f"('{identity}',pg_catalog.to_regprocedure('{identity}'))"
		for identity in allowed_function_identities
	)
	type_values = ",".join(
		f"('{identity}',pg_catalog.to_regtype('{identity}'))"
		for identity in sorted(UPGRADE_RUNTIME_TYPE_NAMES)
	)
	internal_values = ",".join(
		f"('{identity}',pg_catalog.to_regprocedure('{identity}'))"
		for identity in sorted(V19_INTERNAL_SIGNATURES)
	)

	function_grants = json.loads(psql(
		database,
		"WITH allowed(identity,oid) AS (VALUES " + function_values + ") "
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		"'identity',allowed.identity,'catalog_identity',procedure.oid::pg_catalog.regprocedure::text,"
		"'grantor',pg_catalog.pg_get_userbyid(privilege.grantor),"
		"'grantee',pg_catalog.pg_get_userbyid(privilege.grantee),"
		"'is_grantable',privilege.is_grantable) ORDER BY allowed.identity),"
		"'[]'::pg_catalog.json)::text FROM pg_catalog.pg_proc AS procedure "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=procedure.pronamespace "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(procedure.proacl,"
		"pg_catalog.acldefault('f',procedure.proowner))) AS privilege "
		"LEFT JOIN allowed ON allowed.oid=procedure.oid "
		"WHERE namespace.nspname='decodex' AND privilege.privilege_type='EXECUTE' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole",
		env,
	))
	type_grants = json.loads(psql(
		database,
		"WITH allowed(identity,oid) AS (VALUES " + type_values + ") "
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		"'identity',allowed.identity,'catalog_identity',type.oid::pg_catalog.regtype::text,"
		"'grantor',pg_catalog.pg_get_userbyid(privilege.grantor),"
		"'grantee',pg_catalog.pg_get_userbyid(privilege.grantee),"
		"'is_grantable',privilege.is_grantable) ORDER BY allowed.identity),"
		"'[]'::pg_catalog.json)::text FROM pg_catalog.pg_type AS type "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(type.typacl,"
		"pg_catalog.acldefault('T',type.typowner))) AS privilege "
		"LEFT JOIN allowed ON allowed.oid=type.oid "
		"WHERE namespace.nspname='decodex' AND privilege.privilege_type='USAGE' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole",
		env,
	))
	internal_sealing = json.loads(psql(
		database,
		"WITH internal(identity,oid) AS (VALUES " + internal_values + ") "
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		"'identity',internal.identity,"
		"'runtime_effective_execute',pg_catalog.has_function_privilege("
		f"'{RUNTIME_ROLE}'::pg_catalog.regrole,internal.oid,'EXECUTE'),"
		"'runtime_direct_execute',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc "
		"AS procedure CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(procedure.proacl,"
		"pg_catalog.acldefault('f',procedure.proowner))) AS privilege WHERE "
		"procedure.oid=internal.oid AND privilege.privilege_type='EXECUTE' AND "
		f"privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'public_execute',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS procedure "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(procedure.proacl,"
		"pg_catalog.acldefault('f',procedure.proowner))) AS privilege WHERE "
		"procedure.oid=internal.oid AND privilege.privilege_type='EXECUTE' AND "
		"privilege.grantee=0)) ORDER BY internal.identity),'[]'::pg_catalog.json)::text "
		"FROM internal JOIN pg_catalog.pg_proc AS procedure ON procedure.oid=internal.oid",
		env,
	))
	unrelated_authority = json.loads(psql(
		database,
		"SELECT pg_catalog.json_build_object("
		"'relation_acl_rows',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS class "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=class.relnamespace "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(class.relacl,"
		"pg_catalog.acldefault(CASE WHEN class.relkind='S' THEN 's'::\"char\" ELSE "
		"'r'::\"char\" END,class.relowner))) AS privilege WHERE namespace.nspname='decodex' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'schema_acl_rows',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_namespace AS namespace "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(namespace.nspacl,"
		"pg_catalog.acldefault('n',namespace.nspowner))) AS privilege WHERE "
		"namespace.nspname='decodex' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'owned_relations',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS class "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=class.relnamespace "
		"WHERE namespace.nspname='decodex' "
		f"AND class.relowner='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'owned_functions',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS procedure "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=procedure.pronamespace "
		"WHERE namespace.nspname='decodex' "
		f"AND procedure.proowner='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'owned_types',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_type AS type "
		"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace "
		"WHERE namespace.nspname='decodex' "
		f"AND type.typowner='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'owned_schemas',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_namespace "
		f"WHERE nspowner='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'owned_databases',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_database "
		f"WHERE datdba='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'role_memberships',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_auth_members "
		f"WHERE member='{RUNTIME_ROLE}'::pg_catalog.regrole OR "
		f"roleid='{RUNTIME_ROLE}'::pg_catalog.regrole),"
		"'default_acl_rows',(SELECT pg_catalog.count(*) FROM pg_catalog.pg_default_acl "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(defaclacl) AS privilege "
		f"WHERE privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole))::text",
		env,
	))

	if (
		not isinstance(function_grants, list)
		or [row.get("identity") for row in function_grants if isinstance(row, dict)]
		!= list(allowed_function_identities)
		or any(
			not isinstance(row, dict)
			or row.get("grantor") != MIGRATION_ROLE
			or row.get("grantee") != RUNTIME_ROLE
			or row.get("is_grantable") is not False
			for row in function_grants
		)
	):
		raise TestFailure("V13 upgrade runtime function authority is not exact")
	if (
		not isinstance(type_grants, list)
		or [row.get("identity") for row in type_grants if isinstance(row, dict)]
		!= list(sorted(UPGRADE_RUNTIME_TYPE_NAMES))
		or any(
			not isinstance(row, dict)
			or row.get("grantor") != MIGRATION_ROLE
			or row.get("grantee") != RUNTIME_ROLE
			or row.get("is_grantable") is not False
			for row in type_grants
		)
	):
		raise TestFailure("V13 upgrade runtime type authority is not exact")
	if (
		not isinstance(internal_sealing, list)
		or [row.get("identity") for row in internal_sealing if isinstance(row, dict)]
		!= list(sorted(V19_INTERNAL_SIGNATURES))
		or any(
			not isinstance(row, dict)
			or row.get("runtime_effective_execute") is not False
			or row.get("runtime_direct_execute") != 0
			or row.get("public_execute") != 0
			for row in internal_sealing
		)
	):
		raise TestFailure("V19 internal function sealing is not exact")
	if not isinstance(unrelated_authority, dict) or any(
		value != 0 for value in unrelated_authority.values()
	):
		raise TestFailure("V13 upgrade added unrelated runtime authority")

	return {
		"database": database,
		"migration_role": MIGRATION_ROLE,
		"runtime_role": RUNTIME_ROLE,
		"anchor_binding": next(
			row for row in function_grants if row["identity"] == AUTHORITY_ANCHOR_SIGNATURE
		),
		"migration_delta": {
			"execute_count": len(UPGRADE_RUNTIME_EXECUTE_SIGNATURES),
			"execute_grants": [
				row for row in function_grants
				if row["identity"] != AUTHORITY_ANCHOR_SIGNATURE
			],
			"type_usage_count": len(UPGRADE_RUNTIME_TYPE_NAMES),
			"type_usage_grants": type_grants,
		},
		"all_direct_runtime_function_grants": function_grants,
		"v19_internal_sealing": internal_sealing,
		"unrelated_authority": unrelated_authority,
	}


def require_manifest_binding(document: dict[str, object], database: str) -> None:
	binding = document.get("binding")
	expected = {
		"requested": database,
		"migration_url": database,
		"runtime_url": database,
		"observed_migration": database,
		"observed_runtime": database,
	}
	if binding != expected:
		raise TestFailure(f"authority candidate database binding differs for {database}")


def require_exact_keys(value: object, expected: set[str], classification: str) -> dict[str, object]:
	if not isinstance(value, dict) or set(value) != expected:
		raise TestFailure(classification)
	return value


def require_source_binding(value: object, classification: str) -> dict[str, str]:
	binding = require_exact_keys(value, {"head", "tree"}, classification)
	head = binding["head"]
	tree = binding["tree"]
	if (
		not isinstance(head, str) or re.fullmatch(r"[0-9a-f]{40}", head) is None
		or not isinstance(tree, str) or re.fullmatch(r"[0-9a-f]{40}", tree) is None
	):
		raise TestFailure(classification)
	return {"head": head, "tree": tree}


def require_receipt_ledger(value: object, *, through_version: int) -> list[object]:
	if not isinstance(value, list) or len(value) != through_version:
		raise TestFailure("Phase A receipt migration ledger is malformed")
	for expected_version, row in enumerate(value, start=1):
		if (
			not isinstance(row, dict)
			or set(row) != {"checksum", "name", "version"}
			or row["version"] != expected_version
			or not isinstance(row["name"], str)
			or re.fullmatch(r"[a-z0-9_]+", row["name"]) is None
			or not isinstance(row["checksum"], str)
			or not row["checksum"].isdigit()
		):
			raise TestFailure("Phase A receipt migration ledger is malformed")
	return value


def require_manifest_summary(value: object, expected_sha256: str) -> None:
	summary = require_exact_keys(
		value,
		{
			"complete", "duplicate_key_multiplicities", "grouped_row_counts", "resolved",
			"row_count", "sha256", "unique",
		},
		"Phase A receipt manifest summary is malformed",
	)
	counts = summary["grouped_row_counts"]
	if (
		summary["complete"] is not True
		or summary["unique"] is not True
		or summary["resolved"] is not True
		or summary["duplicate_key_multiplicities"] != []
		or not isinstance(counts, dict) or not counts
		or any(not isinstance(kind, str) or not isinstance(count, int) or count <= 0
			for kind, count in counts.items())
		or not isinstance(summary["row_count"], int)
		or sum(counts.values()) != summary["row_count"]
		or summary["sha256"] != expected_sha256
	):
		raise TestFailure("Phase A receipt manifest summary is incomplete")


def comparable_receipt_runtime_authority(value: object) -> dict[str, object]:
	authority = require_exact_keys(
		value,
		{
			"anchor_execute", "database", "direct_non_grantable_execute_count",
			"direct_non_grantable_type_usage_count", "migration_role",
			"non_default_runtime_role", "runtime_login", "runtime_role",
		},
		"Phase A receipt runtime authority is malformed",
	)
	if (
		authority["anchor_execute"] is not True
		or authority["non_default_runtime_role"] is not True
		or authority["runtime_login"] is not True
		or authority["migration_role"] != MIGRATION_ROLE
		or authority["runtime_role"] != RUNTIME_ROLE
		or not isinstance(authority["database"], str)
		or not isinstance(authority["direct_non_grantable_execute_count"], int)
		or authority["direct_non_grantable_execute_count"] <= 0
		or not isinstance(authority["direct_non_grantable_type_usage_count"], int)
		or authority["direct_non_grantable_type_usage_count"] <= 0
	):
		raise TestFailure("Phase A receipt runtime authority is incomplete")
	return {key: authority[key] for key in authority if key != "database"}


def validate_phase_a_receipt_document(document: object) -> dict[str, object]:
	receipt = require_exact_keys(
		document,
		{
			"acceptance", "acceptance_lineage", "bindings", "capture_only", "digests",
			"expected_digests", "manifests", "migration_ledger", "mismatches",
			"one_grantee_upgrade", "phase_b_provenance", "population", "postgres",
			"restore_edges", "runtime_authority", "schema", "semantic_authority",
			"semantic_state", "source_binding",
		},
		"Phase A receipt schema is not exact",
	)
	if (
		receipt["schema"] != AUTHORITY_CANDIDATE_SCHEMA
		or receipt["capture_only"] is not True
		or receipt["acceptance"] is not False
	):
		raise TestFailure("Phase A receipt is not non-accepting capture evidence")
	source_binding = require_exact_keys(
		receipt["source_binding"], {"end", "start"}, "Phase A source binding is malformed"
	)
	start = require_source_binding(source_binding["start"], "Phase A source binding is malformed")
	end = require_source_binding(source_binding["end"], "Phase A source binding is malformed")
	if start != end:
		raise TestFailure("Phase A source binding changed during capture")
	lineage = require_exact_keys(
		receipt["acceptance_lineage"],
		{"phase_a_receipt_sha256", "phase_a_source_binding", "phase_b_source_binding"},
		"Phase A receipt lineage is malformed",
	)
	if (
		lineage["phase_a_receipt_sha256"] is not None
		or lineage["phase_b_source_binding"] is not None
		or lineage["phase_a_source_binding"] != end
	):
		raise TestFailure("Phase A receipt lineage is not derivation-only")
	postgres = require_exact_keys(
		receipt["postgres"], {"major", "version", "version_num"},
		"Phase A PostgreSQL version evidence is malformed",
	)
	if (
		postgres["major"] != 18
		or not isinstance(postgres["version"], str)
		or not isinstance(postgres["version_num"], int)
		or postgres["version_num"] // 10_000 != 18
	):
		raise TestFailure("Phase A receipt is not PostgreSQL 18 evidence")
	bindings = require_exact_keys(
		receipt["bindings"],
		{
			"expected_peer_uid", "migration_role", "restored_once_database",
			"restored_twice_database", "runtime_role", "source_database",
		},
		"Phase A principal or database binding is malformed",
	)
	if (
		bindings["migration_role"] != MIGRATION_ROLE
		or bindings["runtime_role"] != RUNTIME_ROLE
		or bindings["expected_peer_uid"] != os.geteuid()
	):
		raise TestFailure("Phase A principal binding differs")
	for binding_name, expected_database in (
		("source_database", AUTHORITY_CAPTURE_DATABASE),
		("restored_once_database", AUTHORITY_CAPTURE_RESTORE_DATABASE),
		("restored_twice_database", AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE),
	):
		database_binding = require_exact_keys(
			bindings[binding_name],
			{
				"migration_url", "observed_migration", "observed_runtime", "requested",
				"runtime_url",
			},
			"Phase A database binding is malformed",
		)
		if any(value != expected_database for value in database_binding.values()):
			raise TestFailure("Phase A database binding is not a bounded database name")

	provenance = require_exact_keys(
		receipt["phase_b_provenance"],
		{
			"allowed_source_delta", "any_other_source_delta_invalidates_candidate",
			"designated_evidence_surface", "phase_a_tree",
			"phase_b_must_record_phase_a_and_phase_b_trees",
		},
		"Phase A provenance is malformed",
	)
	mismatches = receipt["mismatches"]
	if not isinstance(mismatches, list):
		raise TestFailure("Phase A receipt mismatch set is malformed")
	mismatch_components = [
		mismatch.get("component") if isinstance(mismatch, dict) else None
		for mismatch in mismatches
	]
	canonical_components = [component for component, _ in AUTHORITY_DIGEST_CONSTANTS]
	if (
		len(mismatch_components) > len(canonical_components)
		or mismatch_components != [
			component for component in canonical_components
			if component in mismatch_components
		]
	):
		raise TestFailure("Phase A receipt mismatch set or order is not exact")
	allowed_source_delta = [
		name for component, name in AUTHORITY_DIGEST_CONSTANTS
		if component in mismatch_components
	]
	if (
		provenance["phase_a_tree"] != end["tree"]
		or provenance["allowed_source_delta"] != allowed_source_delta
		or provenance["designated_evidence_surface"] != {"schema": AUTHORITY_CANDIDATE_SCHEMA}
		or provenance["phase_b_must_record_phase_a_and_phase_b_trees"] is not True
		or provenance["any_other_source_delta_invalidates_candidate"] is not True
	):
		raise TestFailure("Phase A provenance does not authorize exact Phase B")

	digest_by_component: dict[str, tuple[str, str]] = {}
	for mismatch in mismatches:
		assert isinstance(mismatch, dict)
		if (
			set(mismatch) != {
				"actual_sha256", "classification", "component", "expected_sha256"
			}
			or mismatch["classification"] != "candidate_digest_mismatch"
			or mismatch["actual_sha256"] == mismatch["expected_sha256"]
			or any(
				not isinstance(mismatch[key], str)
				or re.fullmatch(r"[0-9a-f]{64}", mismatch[key]) is None
				for key in ("actual_sha256", "expected_sha256")
			)
		):
			raise TestFailure("Phase A receipt mismatch is malformed")
		digest_by_component[mismatch["component"]] = (
			mismatch["expected_sha256"], mismatch["actual_sha256"]
		)
	expected_digests = require_exact_keys(
		receipt["expected_digests"], {"authority", "schema"},
		"Phase A expected digest evidence is malformed",
	)
	if any(
		expected_digests["schema" if component == "schema" else "authority"]
		!= expected_sha256
		for component, (expected_sha256, _) in digest_by_component.items()
	):
		raise TestFailure("Phase A expected digest evidence is inconsistent")

	digests = require_exact_keys(
		receipt["digests"], {"authority", "schema"}, "Phase A digest evidence is malformed"
	)
	manifests = require_exact_keys(
		receipt["manifests"], {"authority", "schema"}, "Phase A manifest evidence is malformed"
	)
	for receipt_component, mismatch_component in (
		("schema", "schema"), ("authority", "configured_authority")
	):
		digest = require_exact_keys(
			digests[receipt_component], {"actual_sha256", "expected_sha256"},
			"Phase A digest evidence is malformed",
		)
		actuals = require_exact_keys(
			digest["actual_sha256"], {"restored_once", "restored_twice", "source"},
			"Phase A digest checkpoints are malformed",
		)
		expected_sha256 = expected_digests[receipt_component]
		if digest["expected_sha256"] != expected_sha256 or any(
			not isinstance(value, str)
			or re.fullmatch(r"[0-9a-f]{64}", value) is None
			for value in actuals.values()
		):
			raise TestFailure("Phase A digest checkpoints are inconsistent")
		actual_sha256 = actuals["source"]
		if any(value != actual_sha256 for value in actuals.values()):
			raise TestFailure("Phase A digest checkpoints are inconsistent")
		mismatch_digest = digest_by_component.get(mismatch_component)
		if (actual_sha256 == expected_sha256) != (mismatch_digest is None):
			raise TestFailure("Phase A mismatch set is inconsistent with digest evidence")
		if mismatch_digest is not None and mismatch_digest != (
			expected_sha256, actual_sha256
		):
			raise TestFailure("Phase A mismatch evidence is inconsistent")
		checkpoint_summaries = require_exact_keys(
			manifests[receipt_component], {"restored_once", "restored_twice", "source"},
			"Phase A manifest checkpoints are malformed",
		)
		for summary in checkpoint_summaries.values():
			require_manifest_summary(summary, actual_sha256)

	checkpoints = {"source", "restored_once", "restored_twice"}
	ledgers = require_exact_keys(
		receipt["migration_ledger"], checkpoints, "Phase A ledger checkpoints are malformed"
	)
	for ledger in ledgers.values():
		require_receipt_ledger(ledger, through_version=20)
	if ledgers["source"] != ledgers["restored_once"] or ledgers["source"] != ledgers["restored_twice"]:
		raise TestFailure("Phase A V20 ledgers differ across restore checkpoints")
	if ledgers["source"][-1]["name"] != "constraint_restore_canonicalization":
		raise TestFailure("Phase A ledger does not end at V20")
	upgrade = require_exact_keys(
		receipt["one_grantee_upgrade"],
		{"database", "pre_v14_anchor_binding", "runtime_authority", "v13_ledger", "v20_ledger"},
		"Phase A one-grantee upgrade evidence is malformed",
	)
	require_receipt_ledger(upgrade["v13_ledger"], through_version=13)
	upgrade_v20 = require_receipt_ledger(upgrade["v20_ledger"], through_version=20)
	if (
		upgrade["database"] != AUTHORITY_CAPTURE_UPGRADE_DATABASE
		or upgrade_v20 != ledgers["source"]
	):
		raise TestFailure("Phase A one-grantee upgrade does not reach the exact V20 ledger")
	upgrade_authority = require_exact_keys(
		upgrade["runtime_authority"],
		{
			"all_direct_runtime_function_grants", "anchor_binding", "database",
			"migration_delta", "migration_role", "runtime_role", "unrelated_authority",
			"v19_internal_sealing",
		},
		"Phase A one-grantee authority evidence is malformed",
	)
	delta = require_exact_keys(
		upgrade_authority["migration_delta"],
		{"execute_count", "execute_grants", "type_usage_count", "type_usage_grants"},
		"Phase A one-grantee authority delta is malformed",
	)
	if (
		upgrade_authority["database"] != AUTHORITY_CAPTURE_UPGRADE_DATABASE
		or upgrade_authority["migration_role"] != MIGRATION_ROLE
		or upgrade_authority["runtime_role"] != RUNTIME_ROLE
		or delta["execute_count"] != 15
		or not isinstance(delta["execute_grants"], list)
		or len(delta["execute_grants"]) != 15
		or delta["type_usage_count"] != 5
		or not isinstance(delta["type_usage_grants"], list)
		or len(delta["type_usage_grants"]) != 5
		or not isinstance(upgrade_authority["all_direct_runtime_function_grants"], list)
		or len(upgrade_authority["all_direct_runtime_function_grants"]) != 16
		or not isinstance(upgrade_authority["v19_internal_sealing"], list)
		or len(upgrade_authority["v19_internal_sealing"]) != 4
		or upgrade_authority["unrelated_authority"] != {
			"default_acl_rows": 0,
			"owned_databases": 0,
			"owned_functions": 0,
			"owned_relations": 0,
			"owned_schemas": 0,
			"owned_types": 0,
			"relation_acl_rows": 0,
			"role_memberships": 0,
			"schema_acl_rows": 0,
		}
	):
		raise TestFailure("Phase A one-grantee authority delta is incomplete")
	if (
		[row.get("identity") for row in delta["execute_grants"]]
		!= list(sorted(UPGRADE_RUNTIME_EXECUTE_SIGNATURES))
		or [row.get("identity") for row in delta["type_usage_grants"]]
		!= list(sorted(UPGRADE_RUNTIME_TYPE_NAMES))
		or [
			row.get("identity")
			for row in upgrade_authority["all_direct_runtime_function_grants"]
		]
		!= list(sorted((AUTHORITY_ANCHOR_SIGNATURE, *UPGRADE_RUNTIME_EXECUTE_SIGNATURES)))
		or [row.get("identity") for row in upgrade_authority["v19_internal_sealing"]]
		!= list(sorted(V19_INTERNAL_SIGNATURES))
	):
		raise TestFailure("Phase A one-grantee authority identities differ")
	for grant in (
		*delta["execute_grants"], *delta["type_usage_grants"],
		*upgrade_authority["all_direct_runtime_function_grants"],
		upgrade["pre_v14_anchor_binding"], upgrade_authority["anchor_binding"],
	):
		if (
			not isinstance(grant, dict)
			or set(grant) != {
				"catalog_identity", "grantee", "grantor", "identity", "is_grantable"
			}
			or grant["grantee"] != RUNTIME_ROLE
			or grant["grantor"] != MIGRATION_ROLE
			or grant["is_grantable"] is not False
			or not isinstance(grant["identity"], str)
			or not isinstance(grant["catalog_identity"], str)
		):
			raise TestFailure("Phase A one-grantee authority grant is not exact")
	if (
		upgrade["pre_v14_anchor_binding"] != upgrade_authority["anchor_binding"]
		or upgrade_authority["anchor_binding"]["identity"] != AUTHORITY_ANCHOR_SIGNATURE
	):
		raise TestFailure("Phase A one-grantee anchor lineage differs")
	for sealing in upgrade_authority["v19_internal_sealing"]:
		if (
			not isinstance(sealing, dict)
			or set(sealing) != {
				"identity", "public_execute", "runtime_direct_execute",
				"runtime_effective_execute",
			}
			or sealing["public_execute"] != 0
			or sealing["runtime_direct_execute"] != 0
			or sealing["runtime_effective_execute"] is not False
		):
			raise TestFailure("Phase A V19 internal sealing evidence is not exact")

	semantic = require_exact_keys(
		receipt["semantic_authority"], checkpoints,
		"Phase A semantic authority checkpoints are malformed",
	)
	semantic_values = []
	for value in semantic.values():
		summary = require_exact_keys(
			value, {"all_passed", "passed_predicates", "predicate_count", "schema"},
			"Phase A semantic authority summary is malformed",
		)
		passed = summary["passed_predicates"]
		if (
			summary["schema"] != SEMANTIC_AUTHORITY_SCHEMA
			or summary["all_passed"] is not True
			or passed != sorted(SEMANTIC_AUTHORITY_PREDICATES)
			or summary["predicate_count"] != len(SEMANTIC_AUTHORITY_PREDICATES)
		):
			raise TestFailure("Phase A semantic authority did not pass exactly")
		semantic_values.append(summary)
	if semantic_values[0] != semantic_values[1] or semantic_values[0] != semantic_values[2]:
		raise TestFailure("Phase A semantic authority evidence differs across checkpoints")

	restore_edges = require_exact_keys(
		receipt["restore_edges"],
		{"restored_once_to_restored_twice", "source_to_restored_once"},
		"Phase A restore edges are malformed",
	)
	for edge in restore_edges.values():
		exact_edge = require_exact_keys(
			edge,
			{
				"configured_authority_manifest", "migration_ledger", "populated_fixture",
				"runtime_authority_shape", "schema_manifest", "semantic_state",
			},
			"Phase A restore edge is malformed",
		)
		if any(value is not True for value in exact_edge.values()):
			raise TestFailure("Phase A restore edge is incomplete")

	for field in ("semantic_state", "population"):
		values = require_exact_keys(
			receipt[field], checkpoints, f"Phase A {field} checkpoints are malformed"
		)
		if values["source"] != values["restored_once"] or values["source"] != values["restored_twice"]:
			raise TestFailure(f"Phase A {field} differs across checkpoints")
	runtime = require_exact_keys(
		receipt["runtime_authority"], checkpoints | {"zero_grantee_migration"},
		"Phase A runtime authority checkpoints are malformed",
	)
	zero_grantee = require_exact_keys(
		runtime["zero_grantee_migration"],
		{"database", "direct_function_acl_rows", "direct_type_acl_rows"},
		"Phase A zero-grantee authority evidence is malformed",
	)
	if zero_grantee != {
		"database": AUTHORITY_CAPTURE_DATABASE,
		"direct_function_acl_rows": 0,
		"direct_type_acl_rows": 0,
	}:
		raise TestFailure("Phase A zero-grantee authority evidence differs")
	expected_runtime_databases = {
		"source": AUTHORITY_CAPTURE_DATABASE,
		"restored_once": AUTHORITY_CAPTURE_RESTORE_DATABASE,
		"restored_twice": AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
	}
	for checkpoint, database in expected_runtime_databases.items():
		value = runtime[checkpoint]
		if not isinstance(value, dict) or value.get("database") != database:
			raise TestFailure("Phase A runtime database binding differs")
	runtime_shapes = [
		comparable_receipt_runtime_authority(runtime[name]) for name in sorted(checkpoints)
	]
	if runtime_shapes[0] != runtime_shapes[1] or runtime_shapes[0] != runtime_shapes[2]:
		raise TestFailure("Phase A runtime authority shape differs across checkpoints")

	def inspect_public_value(value: object) -> None:
		if isinstance(value, dict):
			for key, nested in value.items():
				if re.search(
					r"(^|_)(path|socket|port|credential|secret|token|temporary|temp)($|_)", key
				):
					raise TestFailure("Phase A receipt contains forbidden operational state")
				inspect_public_value(nested)
		elif isinstance(value, list):
			for nested in value:
				inspect_public_value(nested)
		elif isinstance(value, str) and re.search(
			r"/private/|/tmp/|file://|postgres(?:ql)?://|(?:^|[?;& ])(?:host|port|password|secret|token)=",
			value,
			flags=re.IGNORECASE,
		):
			raise TestFailure("Phase A receipt contains forbidden operational value")
	inspect_public_value(receipt)
	return receipt


def read_private_authority_receipt(path: Path) -> tuple[bytes, str]:
	parent_components = path.parent.parts[1:]
	if (
		not path.is_absolute()
		or not path.name
		or path.name in {".", ".."}
		or any(component in {"", ".", ".."} for component in parent_components)
	):
		raise TestFailure("Phase A receipt location is invalid")
	directory_flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(
		os, "O_CLOEXEC", 0
	)
	no_follow = getattr(os, "O_NOFOLLOW", 0)
	parent_descriptor: int | None = None
	receipt_descriptor: int | None = None
	try:
		parent_descriptor = os.open("/", directory_flags)
		for component in parent_components:
			next_descriptor = os.open(
				component,
				directory_flags | no_follow,
				dir_fd=parent_descriptor,
			)
			os.close(parent_descriptor)
			parent_descriptor = next_descriptor
		parent_metadata = os.fstat(parent_descriptor)
		if (
			not stat.S_ISDIR(parent_metadata.st_mode)
			or parent_metadata.st_uid != os.geteuid()
			or stat.S_IMODE(parent_metadata.st_mode) & 0o077
		):
			raise TestFailure("Phase A receipt parent is not operator-private")
		receipt_descriptor = os.open(
			path.name,
			os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | no_follow,
			dir_fd=parent_descriptor,
		)
		before = os.fstat(receipt_descriptor)
		if (
			not stat.S_ISREG(before.st_mode)
			or before.st_uid != os.geteuid()
			or stat.S_IMODE(before.st_mode) != 0o600
			or before.st_size <= 0
			or before.st_size > AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES
		):
			raise TestFailure("Phase A receipt file metadata is invalid")
		payload_parts: list[bytes] = []
		remaining = AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES + 1
		while remaining:
			chunk = os.read(receipt_descriptor, min(64 * 1024, remaining))
			if not chunk:
				break
			payload_parts.append(chunk)
			remaining -= len(chunk)
		payload = b"".join(payload_parts)
		after = os.fstat(receipt_descriptor)
		parent_after = os.fstat(parent_descriptor)
		stable_fields = (
			"st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink", "st_size",
			"st_mtime_ns", "st_ctime_ns",
		)
		stable_parent_fields = (
			"st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_mtime_ns",
			"st_ctime_ns",
		)
		if (
			len(payload) != before.st_size
			or len(payload) > AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES
			or any(getattr(before, field) != getattr(after, field) for field in stable_fields)
			or any(
				getattr(parent_metadata, field) != getattr(parent_after, field)
				for field in stable_parent_fields
			)
		):
			raise TestFailure("Phase A receipt changed during bounded read")
		return payload, hashlib.sha256(payload).hexdigest()
	except TestFailure:
		raise
	except (OSError, ValueError):
		raise TestFailure("Phase A receipt could not be read safely") from None
	finally:
		if receipt_descriptor is not None:
			os.close(receipt_descriptor)
		if parent_descriptor is not None:
			os.close(parent_descriptor)


def load_phase_a_authority_receipt(path: Path) -> PhaseAAuthorityReceipt:
	payload, digest = read_private_authority_receipt(path)
	try:
		def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
			result: dict[str, object] = {}
			for key, value in pairs:
				if key in result:
					raise ValueError("duplicate JSON key")
				result[key] = value
			return result
		document = json.loads(payload.decode("utf-8"), object_pairs_hook=unique_object)
	except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
		raise TestFailure("Phase A receipt is malformed") from None
	canonical = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()
	if payload != canonical:
		raise TestFailure("Phase A receipt bytes are not canonical and immutable")
	return PhaseAAuthorityReceipt(validate_phase_a_receipt_document(document), digest)


def git_read_bytes(
	*arguments: str,
	byte_limit: int,
	timeout_seconds: float = GIT_READ_TIMEOUT_SECONDS,
) -> bytes:
	process: subprocess.Popen[bytes] | None = None
	stdout = None
	try:
		if byte_limit <= 0 or timeout_seconds <= 0:
			raise TestFailure("Git source lineage is unavailable")
		git_env = os.environ.copy()
		git_env["GIT_NO_REPLACE_OBJECTS"] = "1"
		process = subprocess.Popen(
			["git", *arguments],
			stdin=subprocess.DEVNULL,
			stdout=subprocess.PIPE,
			stderr=subprocess.DEVNULL,
			cwd=REPO_ROOT,
			env=git_env,
		)
		stdout = process.stdout
		if stdout is None:
			raise TestFailure("Git source lineage is unavailable")
		descriptor = stdout.fileno()
		os.set_blocking(descriptor, False)
		deadline = time.monotonic() + timeout_seconds
		payload = bytearray()
		while True:
			remaining = deadline - time.monotonic()
			if remaining <= 0:
				raise TestFailure("Git source lineage is unavailable")
			readable, _, _ = select.select([descriptor], [], [], remaining)
			if not readable:
				raise TestFailure("Git source lineage is unavailable")
			chunk = os.read(descriptor, min(64 * 1024, byte_limit + 1 - len(payload)))
			if not chunk:
				break
			payload.extend(chunk)
			if len(payload) > byte_limit:
				raise TestFailure("Git source lineage is unavailable")
		remaining = deadline - time.monotonic()
		if remaining <= 0 or process.wait(timeout=remaining) != 0:
			raise TestFailure("Git source lineage is unavailable")
		return bytes(payload)
	except TestFailure:
		raise
	except (OSError, ValueError, subprocess.SubprocessError):
		raise TestFailure("Git source lineage is unavailable") from None
	finally:
		if process is not None:
			if process.poll() is None:
				process.kill()
			process.wait()
		if stdout is not None:
			stdout.close()


def git_read_text(
	*arguments: str,
	byte_limit: int,
	timeout_seconds: float = GIT_READ_TIMEOUT_SECONDS,
) -> str:
	try:
		return git_read_bytes(
			*arguments, byte_limit=byte_limit, timeout_seconds=timeout_seconds
		).decode("utf-8")
	except UnicodeDecodeError:
		raise TestFailure("Git source lineage is unavailable") from None


def canonical_digest_constant(
	source: str, name: str
) -> tuple[re.Match[str], str]:
	byte_literal = r"0x[0-9a-f]{2}"
	array = rf"\[\s*{byte_literal}(?:\s*,\s*{byte_literal}){{31}}\s*,?\s*\]"
	pattern = re.compile(
		rf"^const {name}: \[u8; 32\] = (?P<array>{array});[ \t]*$",
		flags=re.MULTILINE,
	)
	if len(re.findall(rf"(?m)^const\s+{name}\b", source)) != 1:
		raise TestFailure("Phase B digest constant source is malformed")
	matches = list(pattern.finditer(source))
	if len(matches) != 1:
		raise TestFailure("Phase B digest constant source is malformed")
	match = matches[0]
	values = re.findall(r"0x([0-9a-f]{2})", match.group("array"))
	if len(values) != 32:
		raise TestFailure("Phase B digest constant source is malformed")
	return match, "".join(values)


def digest_constants_from_source(source: str) -> dict[str, str]:
	return {
		component: canonical_digest_constant(source, name)[1]
		for component, name in AUTHORITY_DIGEST_CONSTANTS
	}


def normalized_digest_source(source: str, components: set[str]) -> str:
	normalized = source
	for component, name in AUTHORITY_DIGEST_CONSTANTS:
		if component not in components:
			continue
		match, _ = canonical_digest_constant(normalized, name)
		start, end = match.span("array")
		normalized = normalized[:start] + "[<PHASE_B_DIGEST>]" + normalized[end:]
	return normalized


def commit_object_binding(head: str) -> tuple[str, tuple[str, ...]]:
	if not re.fullmatch(r"[0-9a-f]{40}", head):
		raise TestFailure("source commit binding is invalid")
	try:
		content = git_read_text(
			"cat-file", "commit", head, byte_limit=GIT_COMMIT_MAX_BYTES
		)
	except TestFailure:
		raise TestFailure("source commit binding is invalid") from None
	headers, separator, _ = content.partition("\n\n")
	if not separator:
		raise TestFailure("source commit binding is invalid")
	trees = tuple(
		line.removeprefix("tree ")
		for line in headers.splitlines()
		if line.startswith("tree ")
	)
	parents = tuple(
		line.removeprefix("parent ")
		for line in headers.splitlines()
		if line.startswith("parent ")
	)
	if (
		len(trees) != 1
		or not re.fullmatch(r"[0-9a-f]{40}", trees[0])
		or any(re.fullmatch(r"[0-9a-f]{40}", parent) is None for parent in parents)
	):
		raise TestFailure("source commit binding is invalid")
	return trees[0], parents


def require_commit_tree_binding(binding: dict[str, str]) -> tuple[str, ...]:
	head = binding["head"]
	tree = binding["tree"]
	if not re.fullmatch(r"[0-9a-f]{40}", head) or not re.fullmatch(r"[0-9a-f]{40}", tree):
		raise TestFailure("source commit binding is invalid")
	actual_tree, parents = commit_object_binding(head)
	if actual_tree != tree:
		raise TestFailure("source commit tree binding differs")
	try:
		tree_type = git_read_text(
			"cat-file", "-t", tree, byte_limit=GIT_METADATA_MAX_BYTES
		).strip()
	except TestFailure:
		raise TestFailure("source commit tree binding is invalid") from None
	if tree_type != "tree":
		raise TestFailure("source commit tree binding is invalid")
	return parents


def require_direct_parent_lineage(
	phase_a_head: str, phase_b_parents: tuple[str, ...]
) -> None:
	if phase_b_parents != (phase_a_head,):
		raise TestFailure("Phase B source commit lineage is invalid")


def require_exact_phase_transition(
	phase_a_binding: dict[str, str], phase_b_binding: dict[str, str]
) -> bool:
	"""Return whether Phase B is a digest-changing direct child."""
	require_commit_tree_binding(phase_a_binding)
	phase_b_parents = require_commit_tree_binding(phase_b_binding)
	if phase_b_binding == phase_a_binding:
		return False
	require_direct_parent_lineage(phase_a_binding["head"], phase_b_parents)
	return True


def require_phase_b_changed_paths(
	changed_paths: list[str], mismatches: list[object]
) -> None:
	expected = [] if not mismatches else ["crates/decodex-postgres/src/authority.rs"]
	if changed_paths != expected:
		raise TestFailure("Phase B source changes do not match the reported digest set")


def require_digest_only_authority_source(
	phase_a_source: str,
	phase_b_source: str,
	mismatches: list[object],
	expected_digests: dict[str, object],
) -> None:
	mismatch_components = {
		mismatch["component"] for mismatch in mismatches if isinstance(mismatch, dict)
	}
	if normalized_digest_source(
		phase_a_source, mismatch_components
	) != normalized_digest_source(phase_b_source, mismatch_components):
		raise TestFailure("Phase B authority source changes exceed the reported digest constants")
	phase_a_constants = digest_constants_from_source(phase_a_source)
	phase_b_constants = digest_constants_from_source(phase_b_source)
	mismatch_by_component = {
		mismatch["component"]: mismatch
		for mismatch in mismatches if isinstance(mismatch, dict)
	}
	for component, _ in AUTHORITY_DIGEST_CONSTANTS:
		receipt_component = "authority" if component == "configured_authority" else component
		if phase_a_constants[component] != expected_digests.get(receipt_component):
			raise TestFailure("Phase A source digest constants do not match its receipt")
		mismatch = mismatch_by_component.get(component)
		if mismatch is None:
			if phase_b_constants[component] != phase_a_constants[component]:
				raise TestFailure("Phase B changed an unreported digest constant")
			continue
		if phase_a_constants[component] != mismatch["expected_sha256"]:
			raise TestFailure("Phase A source digest constants do not match its receipt")
		if phase_b_constants[component] != mismatch["actual_sha256"]:
			raise TestFailure("Phase B source digest constants do not match Phase A evidence")
		if phase_b_constants[component] == phase_a_constants[component]:
			raise TestFailure("Phase B did not change every reported digest constant")


def validate_phase_b_source_delta(
	phase_a: PhaseAAuthorityReceipt, phase_b_binding: dict[str, str]
) -> None:
	source_binding = phase_a.document["source_binding"]
	assert isinstance(source_binding, dict)
	phase_a_binding = require_source_binding(
		source_binding["end"], "Phase A source binding is malformed"
	)
	mismatches = phase_a.document["mismatches"]
	assert isinstance(mismatches, list)
	changed_commit = require_exact_phase_transition(phase_a_binding, phase_b_binding)
	if changed_commit != bool(mismatches):
		raise TestFailure("Phase B commit transition does not match the reported digest set")
	changed_paths = git_read_text(
		"diff", "--name-only", phase_a_binding["tree"], phase_b_binding["tree"],
		byte_limit=GIT_PATH_LIST_MAX_BYTES,
	).splitlines()
	authority_path = "crates/decodex-postgres/src/authority.rs"
	require_phase_b_changed_paths(changed_paths, mismatches)
	phase_a_source = git_read_text(
		"show", f"{phase_a_binding['tree']}:{authority_path}",
		byte_limit=GIT_AUTHORITY_SOURCE_MAX_BYTES,
	)
	phase_b_source = git_read_text(
		"show", f"{phase_b_binding['tree']}:{authority_path}",
		byte_limit=GIT_AUTHORITY_SOURCE_MAX_BYTES,
	)
	expected_digests = phase_a.document["expected_digests"]
	assert isinstance(expected_digests, dict)
	require_digest_only_authority_source(
		phase_a_source, phase_b_source, mismatches, expected_digests
	)


def validate_authority_candidate_output_path(path: Path) -> None:
	if not path.is_absolute():
		raise TestFailure("authority candidate output path must be absolute")
	try:
		path.relative_to(REPO_ROOT)
	except ValueError:
		pass
	else:
		raise TestFailure("authority candidate output path must be outside the source tree")
	parent = path.parent
	for component in (parent, *parent.parents):
		if component.is_symlink():
			raise TestFailure("authority candidate output path must not contain a symlink")
	if not parent.is_dir() or parent.resolve(strict=True) != parent:
		raise TestFailure("authority candidate output parent must be an exact existing directory")
	parent_metadata = parent.stat()
	if parent_metadata.st_uid != os.geteuid() or parent_metadata.st_mode & 0o077:
		raise TestFailure("authority candidate output parent must be operator-owned and private")
	if path.exists() or path.is_symlink():
		raise TestFailure("authority candidate output already exists")


def publish_authority_candidate(path: Path, receipt: dict[str, object]) -> None:
	validate_authority_candidate_output_path(path)
	parent = path.parent
	payload = (json.dumps(receipt, sort_keys=True, separators=(",", ":")) + "\n").encode()
	file_descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=parent)
	temporary_path = Path(temporary_name)
	try:
		with os.fdopen(file_descriptor, "wb") as output:
			os.fchmod(output.fileno(), 0o600)
			output.write(payload)
			output.flush()
			os.fsync(output.fileno())
	except BaseException:
		try:
			temporary_path.unlink(missing_ok=True)
		except BaseException:
			pass
		raise
	try:
		os.link(temporary_path, path)
	except BaseException:
		try:
			temporary_path.unlink(missing_ok=True)
		except BaseException:
			pass
		raise
	# The create-only hard link is the publication commit. Nothing after this point
	# removes or mutates the final receipt; later failure is reconciled by readback.
	temporary_path.unlink(missing_ok=True)
	directory_descriptor = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
	try:
		os.fsync(directory_descriptor)
	finally:
		os.close(directory_descriptor)


def authority_candidate_phase_fields(
	phase_a: PhaseAAuthorityReceipt | None,
	start_binding: dict[str, str],
	end_binding: dict[str, str],
) -> dict[str, object]:
	return {
		"acceptance": phase_a is not None,
		"acceptance_lineage": {
			"phase_a_receipt_sha256": None if phase_a is None else phase_a.sha256,
			"phase_a_source_binding": (
				start_binding if phase_a is None
				else phase_a.document["source_binding"]["end"]
			),
			"phase_b_source_binding": None if phase_a is None else {
				"start": start_binding,
				"end": end_binding,
			},
		},
	}


def run_authority_candidate_capture(
	socket_dir: Path,
	port: int,
	work: Path,
	log_path: Path,
	env: dict[str, str],
	secret_markers: tuple[str, ...],
	phase_a: PhaseAAuthorityReceipt | None = None,
) -> dict[str, object]:
	start_binding = frozen_source_binding()
	if phase_a is not None:
		validate_phase_b_source_delta(phase_a, start_binding)
	pg_version = json.loads(psql(
		"postgres",
		"SELECT pg_catalog.json_build_object("
		"'version',pg_catalog.current_setting('server_version'),"
		"'version_num',pg_catalog.current_setting('server_version_num')::integer,"
		"'major',pg_catalog.current_setting('server_version_num')::integer/10000)::text",
		env,
	))
	if not isinstance(pg_version, dict) or pg_version.get("major") != 18:
		raise TestFailure("authority candidate capture requires PostgreSQL major 18")

	create_database(AUTHORITY_CAPTURE_UPGRADE_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, AUTHORITY_CAPTURE_UPGRADE_DATABASE, RUNTIME_ROLE
	)
	run_migration_through_v13(env)
	upgrade_v13_ledger = capture_migration_ledger(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env, through_version=13
	)
	psql_as(
		MIGRATION_ROLE,
		AUTHORITY_CAPTURE_UPGRADE_DATABASE,
		f"GRANT EXECUTE ON FUNCTION {AUTHORITY_ANCHOR_SIGNATURE} TO {RUNTIME_ROLE}",
		env,
	)
	upgrade_anchor_binding = capture_upgrade_anchor_binding(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	run_migration(env)
	upgrade_v20_ledger = capture_migration_ledger(AUTHORITY_CAPTURE_UPGRADE_DATABASE, env)
	upgrade_runtime_authority = capture_upgrade_runtime_authority(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)

	create_database(AUTHORITY_CAPTURE_DATABASE, env)
	set_contract_urls(env, socket_dir, port, AUTHORITY_CAPTURE_DATABASE, RUNTIME_ROLE)
	run_migration(env)
	zero_grantee_migration_authority = capture_zero_grantee_migration_authority(
		AUTHORITY_CAPTURE_DATABASE, env
	)
	provision_runtime(AUTHORITY_CAPTURE_DATABASE, RUNTIME_ROLE, env)
	psql_as(
		RUNTIME_ROLE,
		AUTHORITY_CAPTURE_DATABASE,
		"INSERT INTO decodex.accounts(account_id,display_label) VALUES "
		"('10000000-0000-4000-8000-000000001300','XY-1300 capture fixture')",
		env,
	)

	source_path = work / "authority-candidate-source.json"
	dump_schema_manifest(
		source_path, AUTHORITY_CAPTURE_DATABASE, env, structured_errors=True
	)
	source = load_capture_manifest(
		source_path,
		"source",
		AUTHORITY_CAPTURE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	source_manifests = require_capture_components(
		source,
		"source",
		AUTHORITY_CAPTURE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	source_semantic_authority = require_capture_semantic_authority(
		source, "source", secret_markers=secret_markers
	)
	source_ledger = capture_migration_ledger(AUTHORITY_CAPTURE_DATABASE, env)
	source_runtime_authority = capture_runtime_authority(AUTHORITY_CAPTURE_DATABASE, env)
	source_population = json.loads(psql(
		AUTHORITY_CAPTURE_DATABASE,
		"SELECT pg_catalog.row_to_json(row)::text FROM (SELECT account_id::text,"
		"display_label,state::text,metadata,revision,observed_at,updated_at FROM "
		"decodex.accounts WHERE account_id='10000000-0000-4000-8000-000000001300') AS row",
		env,
	))
	if not isinstance(source_population, dict):
		raise TestFailure("authority candidate source database is not populated")

	dump_path = work / "authority-candidate.dump"
	run(["pg_dump", "-Fc", "-f", str(dump_path), AUTHORITY_CAPTURE_DATABASE], env)
	create_database(AUTHORITY_CAPTURE_RESTORE_DATABASE, env)
	run(
		["pg_restore", "--exit-on-error", "-d", AUTHORITY_CAPTURE_RESTORE_DATABASE,
		 str(dump_path)],
		env,
	)
	set_contract_urls(env, socket_dir, port, AUTHORITY_CAPTURE_RESTORE_DATABASE, RUNTIME_ROLE)
	restored_path = work / "authority-candidate-restored.json"
	dump_schema_manifest(
		restored_path, AUTHORITY_CAPTURE_RESTORE_DATABASE, env, structured_errors=True
	)
	restored = load_capture_manifest(
		restored_path,
		"restored",
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	restored_manifests = require_capture_components(
		restored,
		"restored",
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	restored_semantic_authority = require_capture_semantic_authority(
		restored, "restored_once", secret_markers=secret_markers
	)
	restored_ledger = capture_migration_ledger(AUTHORITY_CAPTURE_RESTORE_DATABASE, env)
	restored_runtime_authority = capture_runtime_authority(
		AUTHORITY_CAPTURE_RESTORE_DATABASE, env
	)
	restored_population = json.loads(psql(
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
		"SELECT pg_catalog.row_to_json(row)::text FROM (SELECT account_id::text,"
		"display_label,state::text,metadata,revision,observed_at,updated_at FROM "
		"decodex.accounts WHERE account_id='10000000-0000-4000-8000-000000001300') AS row",
		env,
	))
	if not isinstance(restored_population, dict):
		raise TestFailure("authority candidate restored database is not populated")

	second_dump_path = work / "authority-candidate-restored-once.dump"
	run([
		"pg_dump", "-Fc", "-f", str(second_dump_path),
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
	], env)
	create_database(AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE, env)
	run(
		["pg_restore", "--exit-on-error", "-d", AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		 str(second_dump_path)],
		env,
	)
	set_contract_urls(
		env, socket_dir, port, AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE, RUNTIME_ROLE
	)
	second_restored_path = work / "authority-candidate-restored-twice.json"
	dump_schema_manifest(
		second_restored_path,
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		env,
		structured_errors=True,
	)
	second_restored = load_capture_manifest(
		second_restored_path,
		"restored_twice",
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	second_restored_manifests = require_capture_components(
		second_restored,
		"restored_twice",
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		source_binding=start_binding,
		secret_markers=secret_markers,
	)
	second_restored_semantic_authority = require_capture_semantic_authority(
		second_restored, "restored_twice", secret_markers=secret_markers
	)
	second_restored_ledger = capture_migration_ledger(
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE, env
	)
	second_restored_runtime_authority = capture_runtime_authority(
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE, env
	)
	second_restored_population = json.loads(psql(
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		"SELECT pg_catalog.row_to_json(row)::text FROM (SELECT account_id::text,"
		"display_label,state::text,metadata,revision,observed_at,updated_at FROM "
		"decodex.accounts WHERE account_id='10000000-0000-4000-8000-000000001300') AS row",
		env,
	))
	if not isinstance(second_restored_population, dict):
		raise TestFailure("authority candidate second-restored database is not populated")

	checkpoint_manifests = {
		"source": source_manifests,
		"restored_once": restored_manifests,
		"restored_twice": second_restored_manifests,
	}
	checkpoint_documents = {
		"source": source,
		"restored_once": restored,
		"restored_twice": second_restored,
	}
	checkpoint_ledgers = {
		"source": source_ledger,
		"restored_once": restored_ledger,
		"restored_twice": second_restored_ledger,
	}
	checkpoint_runtime_authority = {
		"source": source_runtime_authority,
		"restored_once": restored_runtime_authority,
		"restored_twice": second_restored_runtime_authority,
	}
	checkpoint_semantic_authority = {
		"source": source_semantic_authority,
		"restored_once": restored_semantic_authority,
		"restored_twice": second_restored_semantic_authority,
	}
	checkpoint_population = {
		"source": source_population,
		"restored_once": restored_population,
		"restored_twice": second_restored_population,
	}
	manifest_evidence: dict[str, dict[str, object]] = {
		"schema": {},
		"authority": {},
	}
	expected_digests = {
		"schema": rust_digest_constant("SCHEMA_CONTRACT_SHA256"),
		"authority": rust_digest_constant("CONFIGURED_AUTHORITY_SHA256"),
	}
	mismatches: list[dict[str, object]] = []
	for component in ("schema", "authority"):
		for checkpoint, manifests in checkpoint_manifests.items():
			manifest = manifests[component]
			evidence = authority_manifest_evidence(manifest)
			manifest_evidence[component][checkpoint] = evidence
			if evidence["duplicate_key_multiplicities"]:
				raise TestFailure(
					f"authority candidate {checkpoint} {component} identities are duplicated"
				)
			if component == "schema" and unresolved_dependency_rows(manifest):
				raise TestFailure(
					f"authority candidate {checkpoint} schema has unresolved dependencies"
				)
		source_evidence = manifest_evidence[component]["source"]
		if source_evidence["sha256"] != expected_digests[component]:
			mismatches.append({
				"component": "configured_authority" if component == "authority" else "schema",
				"classification": "candidate_digest_mismatch",
				"expected_sha256": expected_digests[component],
				"actual_sha256": source_evidence["sha256"],
			})

	restore_edges: dict[str, dict[str, bool]] = {}
	for edge, before, after in AUTHORITY_CAPTURE_RESTORE_EDGES:
		restore_edges[edge] = restore_edge_evidence(
			edge,
			checkpoint_manifests[before],
			checkpoint_manifests[after],
			before_ledger=checkpoint_ledgers[before],
			after_ledger=checkpoint_ledgers[after],
			before_semantic_state=checkpoint_documents[before]["sequence_state"],
			after_semantic_state=checkpoint_documents[after]["sequence_state"],
			before_runtime_authority=checkpoint_runtime_authority[before],
			after_runtime_authority=checkpoint_runtime_authority[after],
			before_population=checkpoint_population[before],
			after_population=checkpoint_population[after],
			secret_markers=secret_markers,
		)
	if phase_a is None:
		mismatch_components = [mismatch["component"] for mismatch in mismatches]
		if mismatch_components != [
			component for component, _ in AUTHORITY_DIGEST_CONSTANTS
			if component in mismatch_components
		]:
			raise TestFailure("authority candidate digest mismatch set is not exact")
	else:
		if mismatches:
			raise TestFailure("Phase B authority acceptance retains a digest mismatch")
		phase_a_mismatches = phase_a.document["mismatches"]
		assert isinstance(phase_a_mismatches, list)
		for mismatch in phase_a_mismatches:
			assert isinstance(mismatch, dict)
			component = "authority" if mismatch["component"] == "configured_authority" else "schema"
			if manifest_evidence[component]["source"]["sha256"] != mismatch["actual_sha256"]:
				raise TestFailure("Phase B manifest digest differs from Phase A evidence")
		if checkpoint_ledgers != phase_a.document["migration_ledger"]:
			raise TestFailure("Phase B migration ledger differs from Phase A evidence")
		if manifest_evidence != phase_a.document["manifests"]:
			raise TestFailure("Phase B manifest summaries differ from Phase A evidence")
		if checkpoint_semantic_authority != phase_a.document["semantic_authority"]:
			raise TestFailure("Phase B semantic authority differs from Phase A evidence")
		phase_b_upgrade = {
			"database": AUTHORITY_CAPTURE_UPGRADE_DATABASE,
			"v13_ledger": upgrade_v13_ledger,
			"pre_v14_anchor_binding": upgrade_anchor_binding,
			"v20_ledger": upgrade_v20_ledger,
			"runtime_authority": upgrade_runtime_authority,
		}
		if phase_b_upgrade != phase_a.document["one_grantee_upgrade"]:
			raise TestFailure("Phase B one-grantee evidence differs from Phase A evidence")
		phase_b_runtime_authority = {
			"zero_grantee_migration": zero_grantee_migration_authority,
			**checkpoint_runtime_authority,
		}
		if phase_b_runtime_authority != phase_a.document["runtime_authority"]:
			raise TestFailure("Phase B runtime authority differs from Phase A evidence")

	end_binding = frozen_source_binding()
	if end_binding != start_binding:
		raise TestFailure("authority candidate source binding changed during capture")
	phase_fields = authority_candidate_phase_fields(phase_a, start_binding, end_binding)
	transition_mismatches = (
		mismatches if phase_a is None else phase_a.document["mismatches"]
	)
	assert isinstance(transition_mismatches, list)
	receipt = {
		"schema": AUTHORITY_CANDIDATE_SCHEMA,
		"capture_only": True,
		**phase_fields,
		"source_binding": {"start": start_binding, "end": end_binding},
		"postgres": pg_version,
		"bindings": {
			"source_database": source["binding"],
			"restored_once_database": restored["binding"],
			"restored_twice_database": second_restored["binding"],
			"migration_role": MIGRATION_ROLE,
			"runtime_role": RUNTIME_ROLE,
			"expected_peer_uid": os.geteuid(),
		},
		"migration_ledger": checkpoint_ledgers,
		"one_grantee_upgrade": {
			"database": AUTHORITY_CAPTURE_UPGRADE_DATABASE,
			"v13_ledger": upgrade_v13_ledger,
			"pre_v14_anchor_binding": upgrade_anchor_binding,
			"v20_ledger": upgrade_v20_ledger,
			"runtime_authority": upgrade_runtime_authority,
		},
		"runtime_authority": {
			"zero_grantee_migration": zero_grantee_migration_authority,
			"source": source_runtime_authority,
			"restored_once": restored_runtime_authority,
			"restored_twice": second_restored_runtime_authority,
		},
		"semantic_authority": checkpoint_semantic_authority,
		"expected_digests": expected_digests,
		"digests": {
			component: {
				"expected_sha256": expected_digests[component],
				"actual_sha256": {
					checkpoint: manifest_evidence[component][checkpoint]["sha256"]
					for checkpoint in ("source", "restored_once", "restored_twice")
				},
			}
			for component in ("schema", "authority")
		},
		"manifests": manifest_evidence,
		"semantic_state": {
			"source": source["sequence_state"],
			"restored_once": restored["sequence_state"],
			"restored_twice": second_restored["sequence_state"],
		},
		"population": checkpoint_population,
		"restore_edges": restore_edges,
		"mismatches": mismatches,
		"phase_b_provenance": {
			"phase_a_tree": (
				end_binding["tree"] if phase_a is None
				else phase_a.document["source_binding"]["end"]["tree"]
			),
			"allowed_source_delta": [
				name for component, name in AUTHORITY_DIGEST_CONSTANTS
				if component in {
					mismatch["component"] for mismatch in transition_mismatches
				}
			],
			"designated_evidence_surface": {
				"schema": AUTHORITY_CANDIDATE_SCHEMA,
			},
			"phase_b_must_record_phase_a_and_phase_b_trees": True,
			"any_other_source_delta_invalidates_candidate": True,
		},
	}
	serialized = json.dumps(receipt, sort_keys=True, separators=(",", ":"))
	if any(marker in serialized for marker in secret_markers):
		raise TestFailure("authority candidate receipt contains a secret marker")
	assert_postgres_logs_redact((log_path,), secret_markers)
	return receipt


def write_bootstrap_config(
	root: Path,
	socket_dir: Path,
	port: int,
	database: str,
	migration_role: str,
	runtime_role: str,
) -> None:
	"""Write one private typed daemon-bootstrap root without credentials."""
	root.mkdir(mode=0o700)
	config_path = root / "config.toml"
	config_path.write_text(
		f'''version = 1
active_profile = "local"

[profiles.local]
kind = "local"
address = "127.0.0.1:49152"

[server_host.repositories.decodex]
host_path = "{REPO_ROOT.as_posix()}"

[postgres]
socket_directory = "{socket_dir.as_posix()}"
expected_peer_uid = {os.geteuid()}
port = {port}
database = "{database}"

[postgres.migration]
user = "{migration_role}"

[postgres.runtime]
user = "{runtime_role}"

[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
''',
		encoding="utf-8",
	)
	config_path.chmod(0o600)


def main() -> int | AuthorityCandidatePublication:
	orchestrator = StageOrchestrator({}, [])
	focused_work_items = sys.argv[1:] == ["--focus-work-items"]
	focused_managed_runs = sys.argv[1:] == ["--focus-managed-runs"]
	focused_managed_repositories = sys.argv[1:] == ["--focus-managed-repositories"]
	capture_only = len(sys.argv) == 3 and sys.argv[1] == "--capture-authority-candidate"
	acceptance_mode = len(sys.argv) == 4 and sys.argv[1] == "--accept-authority-candidate"
	capture_output = (
		Path(sys.argv[3]) if acceptance_mode
		else Path(sys.argv[2]) if capture_only
		else None
	)
	authority_mode = capture_only or acceptance_mode
	def configuration_preflight() -> dict[str, object]:
		if sys.argv[1:] and not (
			focused_work_items or focused_managed_runs or focused_managed_repositories
			or authority_mode
		):
			raise TestFailure(
				"usage: postgres_store_test.py [--focus-work-items|--focus-managed-runs|"
				"--focus-managed-repositories|"
				"--capture-authority-candidate ABSOLUTE_OUTPUT_PATH|"
				"--accept-authority-candidate PHASE_A_RECEIPT ABSOLUTE_OUTPUT_PATH]"
			)
		source_binding = frozen_source_binding()
		phase_a = (
			load_phase_a_authority_receipt(Path(sys.argv[2]))
			if acceptance_mode else None
		)
		if phase_a is not None:
			validate_phase_b_source_delta(phase_a, source_binding)
		if capture_output is not None:
			validate_authority_candidate_output_path(capture_output)
		temp_root: Path | None = None
		temp_root_value = os.environ.get("DECODEX_TEST_TEMP_ROOT")
		if temp_root_value:
			requested_root = Path(temp_root_value)
			if not requested_root.is_absolute():
				raise TestFailure("DECODEX_TEST_TEMP_ROOT must be absolute")
			for component in (requested_root, *requested_root.parents):
				if component.is_symlink():
					raise TestFailure("DECODEX_TEST_TEMP_ROOT must not contain a symlink")
			if not requested_root.is_dir():
				raise TestFailure(
					"DECODEX_TEST_TEMP_ROOT must be a real existing directory"
				)
			temp_root = requested_root.resolve(strict=True)
		tools: dict[str, Path] = {}
		for name in ("initdb", "pg_ctl", "psql", "pg_dump", "pg_restore"):
			location = shutil.which(name)
			if location is None:
				raise TestFailure(f"required PostgreSQL tool is unavailable: {name}")
			tools[name] = Path(location).resolve(strict=True)
		work = Path(tempfile.mkdtemp(
			prefix=("decodex-xy1343-" if focused_work_items else
				"decodex-xy1338-" if focused_managed_runs else
				"decodex-xy1364-" if focused_managed_repositories else
				"decodex-xy1300-capture-" if authority_mode else "decodex-xy1267-"),
			dir=temp_root,
		)).resolve()
		return {
			"phase_a": phase_a,
			"source_binding": source_binding,
			"tools": tools,
			"work": work,
		}
	preflight = run_stage(
		orchestrator, "configuration_preflight", configuration_preflight, fatal=True
	)
	if not isinstance(preflight, dict):
		if orchestrator.primary_failure is None:
			raise HarnessCorruption("configuration preflight lost its primary failure")
		raise orchestrator.primary_failure
	phase_a = preflight["phase_a"]
	if phase_a is not None and not isinstance(phase_a, PhaseAAuthorityReceipt):
		raise HarnessCorruption("configuration preflight Phase A state is invalid")
	source_binding = preflight["source_binding"]
	if not isinstance(source_binding, dict):
		raise HarnessCorruption("configuration preflight source binding is invalid")
	tools = preflight["tools"]
	work = preflight["work"]
	if not isinstance(tools, dict) or not isinstance(work, Path):
		raise HarnessCorruption("configuration preflight result is invalid")
	data_dir = work / "postgres"
	socket_dir = work / "socket"
	log_path = work / "postgres.log"
	# TCP is disabled; the port only distinguishes the socket filename inside this unique directory.
	port = 55_432
	role_setting_canary_guc = f"xy1272.canary_{secrets.token_hex(16)}"
	role_setting_secret_canary = secrets.token_hex(32)
	env = os.environ.copy()
	initdb_path = tools["initdb"]
	if not isinstance(initdb_path, Path):
		raise HarnessCorruption("PostgreSQL tool discovery state is invalid")
	postgres_share = initdb_path.parent.parent / "share" / "postgresql"
	env.update(
		{
			"PATH": f"{initdb_path.parent}{os.pathsep}{env['PATH']}",
			"PGHOST": str(socket_dir),
			"PGPORT": str(port),
			"PGUSER": os.environ.get("USER", "postgres"),
			"DECODEX_TEST_BLOB_ROOT": str(work / "blob-root"),
		}
	)
	try:
		def fatal_postgres_preflight() -> None:
			socket_dir.mkdir()
			run(
				[
					"initdb",
					"-D",
					str(data_dir),
					"--auth=trust",
					"--encoding=UTF8",
					"--locale=C",
					"--data-checksums",
					"-L",
					str(postgres_share),
				],
				env,
			)
			run(
				[
					"pg_ctl",
					"-D",
					str(data_dir),
					"-l",
					str(log_path),
					"-o",
					f"-k {socket_dir} -p {port} -h '' -F",
					"-w",
					"start",
				],
				env,
			)
			orchestrator.cluster_started = True
			roles = [MIGRATION_ROLE, RUNTIME_ROLE]
			if not authority_mode:
				roles.extend((
					MISSING_SELECT_ROLE,
					HOSTILE_SEARCH_ROLE,
					FUNCTION_OWNER_ROLE,
					SET_BYPASS_ROLE,
					SET_LEDGER_WRITE_ROLE,
					SET_SEQUENCE_UPDATE_ROLE,
					MEMBERSHIP_ADMIN_ROLE,
					*UNSAFE_ROLES.values(),
				))
			for role in roles:
				group_roles = {
					FUNCTION_OWNER_ROLE,
					SET_BYPASS_ROLE,
					SET_LEDGER_WRITE_ROLE,
					SET_SEQUENCE_UPDATE_ROLE,
					MEMBERSHIP_ADMIN_ROLE,
				}
				attributes = "" if role in group_roles else " LOGIN"
				if role in {MIGRATION_ROLE, RUNTIME_ROLE}:
					attributes += " NOINHERIT VALID UNTIL 'infinity'"
				if role == UNSAFE_ROLES["bypassrls"]:
					attributes += " BYPASSRLS"
				elif role == UNSAFE_ROLES["superuser"]:
					attributes += " SUPERUSER"
				psql("postgres", f"CREATE ROLE {role}{attributes}", env)

			return None
		run_stage(
			orchestrator,
			"cluster_preflight",
			fatal_postgres_preflight,
			depends_on=("configuration_preflight",),
			fatal=True,
		)
		if orchestrator.stages["cluster_preflight"]["status"] != "passed":
			if orchestrator.primary_failure is None:
				raise HarnessCorruption("fatal preflight lost its primary failure")
			raise orchestrator.primary_failure

		if focused_work_items:
			output = run_stage(
				orchestrator,
				"focused_work_items",
				lambda: run_work_item_focused_contracts(socket_dir, port, work, env),
				depends_on=("cluster_preflight",),
			)
			if isinstance(output, str):
				print(output)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			return 0
		if focused_managed_runs:
			output = run_stage(
				orchestrator,
				"focused_managed_runs",
				lambda: run_managed_run_focused_contracts(socket_dir, port, work, env),
				depends_on=("cluster_preflight",),
			)
			if isinstance(output, str):
				print(output)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			return 0
		if focused_managed_repositories:
			output = run_stage(
				orchestrator,
				"focused_managed_repositories",
				lambda: run_managed_repository_focused_contracts(socket_dir, port, env),
				depends_on=("cluster_preflight",),
			)
			if isinstance(output, str):
				print(output)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			return 0
		if capture_output is not None:
			capture_receipt = run_stage(
				orchestrator,
				"authority_acceptance" if phase_a is not None else "authority_capture",
				lambda: run_authority_candidate_capture(
					socket_dir,
					port,
					work,
					log_path,
					env,
					(role_setting_canary_guc, role_setting_secret_canary),
					phase_a,
				),
				depends_on=("cluster_preflight",),
			)
			if not isinstance(capture_receipt, dict):
				if orchestrator.primary_failure is None:
					raise HarnessCorruption("authority receipt stage lost its result")
				raise orchestrator.primary_failure
			return AuthorityCandidatePublication(capture_output, capture_receipt)
		restore_report: dict[str, object] = {
			"checkpoints": {},
			"production_checks": {},
			"stages": {},
		}
		acceptance_failures: list[str] = []
		def primary_foundation() -> str:
			create_database(DATABASE, env)
			set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
			migration_output = run_migration(env)
			provision_runtime(DATABASE, RUNTIME_ROLE, env)
			initial_manifest_output = capture_restore_checkpoint(
				restore_report,
				"source",
				work / "schema-manifest-initial.json",
				DATABASE,
				env,
			)
			initial_stages = restore_report["stages"]
			if not isinstance(initial_stages, dict):
				raise HarnessCorruption("initial restore report state is invalid")
			if initial_stages.get("source_capture") != {"status": "passed"}:
				raise TestFailure("initial raw PostgreSQL manifest capture did not pass")
			readiness_output = run(
				["cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
				 "postgres_store", "--run-ignored", "all", "--",
				 "postgres_manifest_readiness_fixture", "--exact"],
				env,
			)
			return "\n".join((migration_output, initial_manifest_output, readiness_output))
		run_stage(
			orchestrator,
			"primary_foundation",
			primary_foundation,
			depends_on=("cluster_preflight",),
		)

		def role_profile_suite() -> str:
			return run_role_profile_final_gate_contracts(
				data_dir, log_path, socket_dir, port, work, env, restore_report
			)
		run_stage(
			orchestrator,
			"role_profile_suite",
			role_profile_suite,
			depends_on=("cluster_preflight",),
		)

		def runtime_session_suite() -> str:
			return run_runtime_session_final_gate_contracts(
				data_dir, log_path, socket_dir, port, work, env, restore_report
			)
		run_stage(
			orchestrator,
			"runtime_session_suite",
			runtime_session_suite,
			depends_on=("cluster_preflight",),
		)
		run_stage(
			orchestrator,
			"v8_migration_boundary",
			lambda: run_v8_migration_boundary_contracts(env, socket_dir, port),
			depends_on=("cluster_preflight",),
		)
		def blob_session_restart() -> str:
			set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
			return run_blob_session_restart_contract(
				data_dir, log_path, socket_dir, port, work, env
			)
		run_stage(
			orchestrator,
			"blob_session_restart",
			blob_session_restart,
			depends_on=("primary_foundation",),
		)
		def postgres_store_contract() -> str:
			if psql_as(
				RUNTIME_ROLE,
				DATABASE,
				"SELECT current_setting('session_replication_role'), "
				"has_parameter_privilege(current_user, 'session_replication_role', 'SET'), "
				"has_parameter_privilege(current_user, 'session_replication_role', 'ALTER SYSTEM'), "
				"has_table_privilege(current_user, 'public.refinery_schema_history', 'UPDATE'), "
				"has_sequence_privilege(current_user, "
				"'decodex.activity_sequence_seq', 'USAGE'), "
				"has_sequence_privilege(current_user, "
				"'decodex.activity_sequence_seq', 'UPDATE'), "
				"has_sequence_privilege(current_user, "
				"'decodex.activity_sequence_seq', 'USAGE WITH GRANT OPTION')",
				env,
			) != "origin|f|f|f|t|f|f":
				raise TestFailure(
					"valid runtime role is not a non-vacuous least-privilege fixture"
				)
			return run(
				[
				"cargo",
				"nextest",
				"run",
				"-p",
				"decodex-postgres",
				"--features",
				"test-support",
				"--test",
				"postgres_store",
				"--run-ignored",
				"all",
				"--",
				"postgres_store_contract",
				"--exact",
				],
				env,
			)
		run_stage(
			orchestrator,
			"postgres_store_contract",
			postgres_store_contract,
			depends_on=("primary_foundation",),
		)
		run_stage(
			orchestrator,
			"managed_repository_contracts",
			lambda: "\n".join((
				run_managed_repository_test(
					"postgres_managed_repository_authority_contract", env
				),
				run_managed_repository_test(
					"postgres_managed_repository_restart_backlog_bound", env
				),
			)),
			depends_on=("primary_foundation",),
		)
		env["DECODEX_TEST_SOCKET_DIRECTORY"] = str(socket_dir)
		run_stage(
			orchestrator,
			"account_composition",
			lambda: run([
				"cargo",
				"nextest",
				"run",
				"-p",
				"decodex-runtime",
				"--lib",
				"--all-features",
				"--run-ignored",
				"all",
				"--",
				"account_launch::postgres_composition_tests::postgres_private_capacity_and_codex_composition_is_fail_closed",
				"--exact",
			], env),
			depends_on=("primary_foundation",),
		)
		bootstrap_root = work / "decodex-root"
		def bootstrap_configuration() -> None:
			write_bootstrap_config(
				bootstrap_root, socket_dir, port, DATABASE, MIGRATION_ROLE, RUNTIME_ROLE
			)
			env["DECODEX_TEST_BOOTSTRAP_ROOT"] = str(bootstrap_root)
			env["DECODEX_TEST_SOCKET_PORT"] = str(port)
		run_stage(
			orchestrator,
			"bootstrap_configuration",
			bootstrap_configuration,
			depends_on=("primary_foundation",),
		)
		run_stage(
			orchestrator,
			"bootstrap_doctor_history_daemon",
			lambda: run(
					[
						"cargo",
						"nextest",
						"run",
						"-p",
						"decodex-runtime",
						"--test",
						"bootstrap_doctor",
						"--run-ignored",
						"all",
						"--",
						"isolated_postgres_bootstrap_is_available_through_the_daemon",
						"--exact",
					],
					env,
				),
			depends_on=("postgres_store_contract", "bootstrap_configuration"),
		)
		run_stage(
			orchestrator,
			"live_endpoint_doctor",
			lambda: run([
				"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
				"bootstrap_doctor", "--run-ignored", "all", "--",
				"isolated_postgres_live_doctor_rejects_replaced_endpoint", "--exact",
			], env),
			depends_on=("primary_foundation",),
		)
		auth_bootstrap_root = work / "decodex-auth-root"
		def authentication_rejection() -> str:
			write_bootstrap_config(
				auth_bootstrap_root,
				socket_dir,
				port,
				DATABASE,
				"decodex_xy1307_role_that_does_not_exist",
				RUNTIME_ROLE,
			)
			env["DECODEX_TEST_AUTH_BOOTSTRAP_ROOT"] = str(auth_bootstrap_root)
			return run([
				"cargo",
				"nextest",
				"run",
				"-p",
				"decodex-runtime",
				"--test",
				"bootstrap_doctor",
				"--run-ignored",
				"all",
				"--",
				"isolated_postgres_rejected_role_is_authentication",
				"--exact",
			], env)
		run_stage(
			orchestrator,
			"authentication_rejection",
			authentication_rejection,
			depends_on=("primary_foundation",),
		)
		def collation_contract() -> str:
			create_database(COLLATION_DATABASE, env, locale="tr-TR")
			set_contract_urls(env, socket_dir, port, COLLATION_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(COLLATION_DATABASE, RUNTIME_ROLE, env)
			env["DECODEX_TEST_COLLATION_MIGRATION_DATABASE_URL"] = env[
				"DECODEX_TEST_MIGRATION_DATABASE_URL"
			]
			env["DECODEX_TEST_COLLATION_RUNTIME_DATABASE_URL"] = env[
				"DECODEX_TEST_RUNTIME_DATABASE_URL"
			]
			return run([
				"cargo",
				"nextest",
				"run",
				"-p",
				"decodex-postgres",
				"--test",
				"postgres_store",
				"--run-ignored",
				"all",
				"--",
				"postgres_store_turkish_collation_contract",
				"--exact",
			], env)
		run_stage(
			orchestrator,
			"turkish_collation_contract",
			collation_contract,
			depends_on=("cluster_preflight",),
		)

		def authority_safety_suite() -> str:
			create_database(AUTHORITY_DATABASE, env)
			set_contract_urls(env, socket_dir, port, AUTHORITY_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			for role in UNSAFE_ROLES.values():
				provision_runtime(AUTHORITY_DATABASE, role, env)
			provision_runtime(AUTHORITY_DATABASE, MISSING_SELECT_ROLE, env)
			psql(
				AUTHORITY_DATABASE,
				f"ALTER TABLE decodex.accounts OWNER TO {UNSAFE_ROLES['table-owner']}; "
				f"GRANT TRUNCATE ON TABLE decodex.outbox TO {UNSAFE_ROLES['truncate']}; "
				f"GRANT CREATE ON SCHEMA decodex TO {UNSAFE_ROLES['schema-create']}; "
				f"GRANT SET ON PARAMETER session_replication_role "
				f"TO {UNSAFE_ROLES['trigger-bypass']}; "
				f"GRANT ALTER SYSTEM ON PARAMETER session_replication_role "
				f"TO {UNSAFE_ROLES['alter-system-bypass']}; "
				f"ALTER ROLE {UNSAFE_ROLES['login-default-replica']} "
				f"SET session_replication_role = replica; "
				f"GRANT UPDATE ON TABLE public.refinery_schema_history "
				f"TO {UNSAFE_ROLES['migration-history-write']}; "
				f"GRANT SELECT (version) ON TABLE public.refinery_schema_history "
				f"TO {UNSAFE_ROLES['migration-history-column-grant']} WITH GRANT OPTION; "
				f"GRANT UPDATE ON TABLE public.refinery_schema_history "
				f"TO {SET_LEDGER_WRITE_ROLE}; "
				f"GRANT {SET_LEDGER_WRITE_ROLE} "
				f"TO {UNSAFE_ROLES['migration-history-set-write']} "
				f"WITH INHERIT FALSE, SET TRUE; "
				f"GRANT UPDATE ON ALL SEQUENCES IN SCHEMA decodex "
				f"TO {UNSAFE_ROLES['sequence-update']}; "
				f"GRANT UPDATE ON ALL SEQUENCES IN SCHEMA decodex "
				f"TO {SET_SEQUENCE_UPDATE_ROLE}; "
				f"GRANT {SET_SEQUENCE_UPDATE_ROLE} "
				f"TO {UNSAFE_ROLES['sequence-set-update']} "
				f"WITH INHERIT FALSE, SET TRUE; "
				f"GRANT USAGE ON ALL SEQUENCES IN SCHEMA decodex "
				f"TO {UNSAFE_ROLES['sequence-grant-option']} WITH GRANT OPTION; "
				f"GRANT SELECT ON TABLE decodex.accounts "
				f"TO {UNSAFE_ROLES['table-grant-option']} WITH GRANT OPTION; "
				f"GRANT EXECUTE ON FUNCTION decodex.enforce_lease_operation_time() "
				f"TO {UNSAFE_ROLES['function-grant-option']} WITH GRANT OPTION; "
				f"CREATE COLLATION decodex.unsafe_owned_collation FROM pg_catalog.\"C\"; "
				f"ALTER COLLATION decodex.unsafe_owned_collation "
				f"OWNER TO {UNSAFE_ROLES['collation-owner']}; "
				f"CREATE CONVERSION decodex.unsafe_owned_conversion "
				f"FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1; "
				f"ALTER CONVERSION decodex.unsafe_owned_conversion "
				f"OWNER TO {UNSAFE_ROLES['conversion-owner']}; "
				f"CREATE OPERATOR decodex.=== (FUNCTION = pg_catalog.int4eq, "
				f"LEFTARG = integer, RIGHTARG = integer); "
				f"ALTER OPERATOR decodex.=== (integer, integer) "
				f"OWNER TO {UNSAFE_ROLES['operator-owner']}; "
				f"CREATE TEXT SEARCH CONFIGURATION decodex.unsafe_owned_text_search "
				f"(COPY = pg_catalog.simple); "
				f"ALTER TEXT SEARCH CONFIGURATION decodex.unsafe_owned_text_search "
				f"OWNER TO {UNSAFE_ROLES['text-search-owner']}; "
				f"GRANT USAGE ON SCHEMA decodex TO {SET_BYPASS_ROLE}; "
				f"GRANT TRUNCATE ON TABLE decodex.outbox TO {SET_BYPASS_ROLE}; "
				f"GRANT SET ON PARAMETER session_replication_role TO {SET_BYPASS_ROLE}; "
				f"GRANT {SET_BYPASS_ROLE} TO {UNSAFE_ROLES['set-role-bypass']} "
				f"WITH INHERIT FALSE, SET TRUE; "
				f"GRANT CREATE ON SCHEMA decodex TO {FUNCTION_OWNER_ROLE}; "
				f"ALTER FUNCTION decodex.enforce_outbox_terminal_retention() "
				f"OWNER TO {FUNCTION_OWNER_ROLE}; "
				f"GRANT {FUNCTION_OWNER_ROLE} "
				f"TO {UNSAFE_ROLES['function-owner-membership']} "
				f"WITH INHERIT FALSE, SET TRUE; "
				f"GRANT {MEMBERSHIP_ADMIN_ROLE} TO {UNSAFE_ROLES['membership-admin']} "
				f"WITH ADMIN TRUE, INHERIT FALSE, SET FALSE; "
				f"REVOKE SELECT ON TABLE public.refinery_schema_history "
				f"FROM {MISSING_SELECT_ROLE}",
				env,
			)
			if psql_as(
				UNSAFE_ROLES["collation-owner"],
				AUTHORITY_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_collation AS object "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid = object.collnamespace "
				"WHERE namespace.nspname = 'decodex' AND object.collowner = current_user::regrole",
				env,
			) != "1":
				raise TestFailure("collation ownership fixture is vacuous")
			if psql_as(
				UNSAFE_ROLES["conversion-owner"],
				AUTHORITY_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_conversion AS object "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid = object.connamespace "
				"WHERE namespace.nspname = 'decodex' AND object.conowner = current_user::regrole",
				env,
			) != "1":
				raise TestFailure("conversion ownership fixture is vacuous")
			if psql_as(
				UNSAFE_ROLES["operator-owner"],
				AUTHORITY_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_operator AS object "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid = object.oprnamespace "
				"WHERE namespace.nspname = 'decodex' AND object.oprowner = current_user::regrole",
				env,
			) != "1":
				raise TestFailure("operator ownership fixture is vacuous")
			if psql_as(
				UNSAFE_ROLES["text-search-owner"],
				AUTHORITY_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_ts_config AS object "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid = object.cfgnamespace "
				"WHERE namespace.nspname = 'decodex' AND object.cfgowner = current_user::regrole",
				env,
			) != "1":
				raise TestFailure("text-search ownership fixture is vacuous")
			if psql_as(
				UNSAFE_ROLES["function-owner-membership"],
				AUTHORITY_DATABASE,
				f"SELECT has_schema_privilege(current_user, 'decodex', 'CREATE'), "
				f"pg_has_role(current_user, '{FUNCTION_OWNER_ROLE}', 'SET')",
				env,
			) != "f|t":
				raise TestFailure("function-owner fixture is not isolated to SET ROLE authority")
			if psql_as(
				UNSAFE_ROLES["login-default-replica"],
				AUTHORITY_DATABASE,
				"SELECT current_setting('session_replication_role'), "
				"has_parameter_privilege(current_user, 'session_replication_role', 'SET')",
				env,
			) != "replica|f":
				raise TestFailure("login-default replica fixture is not effective without SET")
			if psql_as(
				UNSAFE_ROLES["alter-system-bypass"],
				AUTHORITY_DATABASE,
				"SELECT has_parameter_privilege(current_user, 'session_replication_role', 'SET'), "
				"has_parameter_privilege(current_user, 'session_replication_role', 'ALTER SYSTEM')",
				env,
			) != "f|t":
				raise TestFailure("ALTER SYSTEM fixture is not isolated from SET authority")
			if psql_as(
				UNSAFE_ROLES["migration-history-write"],
				AUTHORITY_DATABASE,
				"SELECT has_table_privilege(current_user, "
				"'public.refinery_schema_history', 'UPDATE')",
				env,
			) != "t":
				raise TestFailure("migration-history fixture lacks write authority")
			if psql_as(
				UNSAFE_ROLES["set-role-bypass"],
				AUTHORITY_DATABASE,
				f"SELECT has_table_privilege(current_user, 'decodex.outbox', 'TRUNCATE'), "
				f"has_parameter_privilege(current_user, 'session_replication_role', 'SET'), "
				f"(SELECT rolbypassrls FROM pg_catalog.pg_roles WHERE rolname = current_user), "
				f"has_schema_privilege(current_user, 'decodex', 'CREATE'), "
				f"pg_has_role(current_user, '{SET_BYPASS_ROLE}', 'SET')",
				env,
			) != "f|f|f|f|t":
				raise TestFailure("SET-only retention fixture leaks authority without SET ROLE")
			if psql_as(
				UNSAFE_ROLES["set-role-bypass"],
				AUTHORITY_DATABASE,
				f"SET ROLE {SET_BYPASS_ROLE}; "
				f"SELECT current_user, "
				f"has_table_privilege(current_user, 'decodex.outbox', 'TRUNCATE'), "
				f"has_parameter_privilege(current_user, 'session_replication_role', 'SET'), "
				f"(SELECT rolbypassrls FROM pg_catalog.pg_roles WHERE rolname = current_user), "
				f"has_schema_privilege(current_user, 'decodex', 'CREATE')",
				env,
			) != f"{SET_BYPASS_ROLE}|t|t|f|f":
				raise TestFailure("SET-only retention fixture lacks authority after SET ROLE")
			if psql_as(
				UNSAFE_ROLES["migration-history-set-write"],
				AUTHORITY_DATABASE,
				f"SELECT has_table_privilege(current_user, "
				f"'public.refinery_schema_history', 'UPDATE'), "
				f"pg_has_role(current_user, '{SET_LEDGER_WRITE_ROLE}', 'SET')",
				env,
			) != "f|t":
				raise TestFailure("SET-only migration-ledger fixture is not isolated")
			if psql_as(
				UNSAFE_ROLES["sequence-set-update"],
				AUTHORITY_DATABASE,
				f"SELECT has_sequence_privilege(current_user, "
				f"'decodex.activity_sequence_seq', 'UPDATE'), "
				f"pg_has_role(current_user, '{SET_SEQUENCE_UPDATE_ROLE}', 'SET')",
				env,
			) != "f|t":
				raise TestFailure("SET-only sequence fixture is not isolated")
			if psql_as(
				UNSAFE_ROLES["migration-history-column-grant"],
				AUTHORITY_DATABASE,
				"SELECT has_any_column_privilege(current_user, "
				"'public.refinery_schema_history', 'SELECT WITH GRANT OPTION')",
				env,
			) != "t":
				raise TestFailure("migration-ledger column grant-option fixture is vacuous")
			if psql_as(
				UNSAFE_ROLES["membership-admin"],
				AUTHORITY_DATABASE,
				f"SELECT pg_has_role(current_user, '{MEMBERSHIP_ADMIN_ROLE}', "
				f"'MEMBER WITH ADMIN OPTION'), "
				f"pg_has_role(current_user, '{MEMBERSHIP_ADMIN_ROLE}', 'SET')",
				env,
			) != "t|f":
				raise TestFailure("membership-admin fixture is not isolated from SET authority")
			if psql_as(
				MISSING_SELECT_ROLE,
				AUTHORITY_DATABASE,
				"SELECT has_table_privilege(current_user, "
				"'public.refinery_schema_history', 'SELECT')",
				env,
			) != "f":
				raise TestFailure("missing migration-ledger SELECT fixture is vacuous")
			unsafe_roots = []
			for case, role in UNSAFE_ROLES.items():
				unsafe_root = work / f"decodex-unsafe-{case}"
				write_bootstrap_config(
					unsafe_root, socket_dir, port, AUTHORITY_DATABASE, MIGRATION_ROLE, role
				)
				unsafe_roots.append(unsafe_root)
			missing_select_root = work / "decodex-unsafe-missing-history-select"
			write_bootstrap_config(
				missing_select_root,
				socket_dir,
				port,
				AUTHORITY_DATABASE,
				MIGRATION_ROLE,
				MISSING_SELECT_ROLE,
			)
			unsafe_roots.append(missing_select_root)

			create_database(TRIGGER_DATABASE, env)
			set_contract_urls(env, socket_dir, port, TRIGGER_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(TRIGGER_DATABASE, RUNTIME_ROLE, env)
			psql(
				TRIGGER_DATABASE,
				"ALTER TABLE decodex.outbox DISABLE TRIGGER outbox_terminal_retention; "
				"DROP TRIGGER leases_operation_time ON decodex.leases; "
				"CREATE TRIGGER leases_operation_time BEFORE INSERT OR UPDATE "
				"ON decodex.leases FOR EACH ROW EXECUTE FUNCTION "
				"decodex.enforce_outbox_operation_time()",
				env,
			)
			trigger_contract = psql(
				TRIGGER_DATABASE,
				"SELECT string_agg(trigger.tgenabled::text || ':' || proc.proname, ',' "
				"ORDER BY trigger.tgname) FROM pg_trigger AS trigger "
				"JOIN pg_proc AS proc ON proc.oid = trigger.tgfoid "
				"WHERE trigger.tgname IN ('leases_operation_time', "
				"'outbox_terminal_retention')",
				env,
			)
			if trigger_contract != (
				"O:enforce_outbox_operation_time,D:enforce_outbox_terminal_retention"
			):
				raise TestFailure("trigger-contract fixture did not preserve both adversarial deltas")
			trigger_root = work / "decodex-unsafe-trigger-contract"
			write_bootstrap_config(
				trigger_root,
				socket_dir,
				port,
				TRIGGER_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			unsafe_roots.append(trigger_root)

			create_database(PRIVILEGED_FUNCTION_DATABASE, env)
			set_contract_urls(
				env, socket_dir, port, PRIVILEGED_FUNCTION_DATABASE, RUNTIME_ROLE
			)
			run_migration(env)
			provision_runtime(PRIVILEGED_FUNCTION_DATABASE, RUNTIME_ROLE, env)
			psql_as(
				MIGRATION_ROLE,
				PRIVILEGED_FUNCTION_DATABASE,
				"CREATE FUNCTION decodex.privileged_runtime_escape() "
				"RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER "
				"SET search_path = pg_catalog, decodex AS $$ BEGIN "
				"EXECUTE 'ALTER TABLE decodex.outbox DISABLE TRIGGER "
				"outbox_terminal_retention'; RETURN false; END $$; "
				f"GRANT EXECUTE ON FUNCTION decodex.privileged_runtime_escape() "
				f"TO {RUNTIME_ROLE}",
				env,
			)
			if psql(
				PRIVILEGED_FUNCTION_DATABASE,
				f"SELECT proc.prosecdef, proc.proconfig IS NOT NULL, "
				f"owner.rolname = '{MIGRATION_ROLE}', "
				f"has_function_privilege('{RUNTIME_ROLE}', proc.oid, 'EXECUTE'), "
				f"proc.prosrc LIKE '%ALTER TABLE decodex.outbox DISABLE TRIGGER%', "
				f"NOT has_schema_privilege('{RUNTIME_ROLE}', 'decodex', 'CREATE'), "
				f"NOT has_table_privilege("
				f"'{RUNTIME_ROLE}', 'decodex.outbox', 'TRIGGER'), "
				f"has_table_privilege("
				f"'{MIGRATION_ROLE}', 'decodex.outbox', 'TRIGGER') "
				f"FROM pg_catalog.pg_proc AS proc "
				f"JOIN pg_catalog.pg_namespace AS namespace "
				f"ON namespace.oid = proc.pronamespace "
				f"JOIN pg_catalog.pg_roles AS owner ON owner.oid = proc.proowner "
				f"WHERE namespace.nspname = 'decodex' "
				f"AND proc.oid = 'decodex.privileged_runtime_escape()'::regprocedure "
				f"AND (SELECT count(*) FROM pg_catalog.pg_proc AS inventory "
				f"JOIN pg_catalog.pg_namespace AS inventory_namespace "
				f"ON inventory_namespace.oid = inventory.pronamespace "
				f"WHERE inventory_namespace.nspname = 'decodex') = 157",
				env,
			) != "t|t|t|t|t|t|t|t":
				raise TestFailure("additional privileged-function fixture is vacuous")
			assert_psql_rejected(
				RUNTIME_ROLE,
				PRIVILEGED_FUNCTION_DATABASE,
				"ALTER TABLE decodex.outbox DISABLE TRIGGER outbox_terminal_retention",
				env,
				"runtime direct trigger DDL",
			)
			if psql_as(
				RUNTIME_ROLE,
				PRIVILEGED_FUNCTION_DATABASE,
				"SELECT decodex.privileged_runtime_escape(); "
				"SELECT tgenabled = 'D' FROM pg_catalog.pg_trigger "
				"WHERE tgrelid = 'decodex.outbox'::pg_catalog.regclass "
				"AND tgname = 'outbox_terminal_retention'",
				env,
			) != "f\nt":
				raise TestFailure("runtime did not exercise the additional function's owner authority")
			psql_as(
				MIGRATION_ROLE,
				PRIVILEGED_FUNCTION_DATABASE,
				"ALTER TABLE decodex.outbox ENABLE TRIGGER outbox_terminal_retention",
				env,
			)
			if psql(
				PRIVILEGED_FUNCTION_DATABASE,
				"SELECT tgenabled FROM pg_catalog.pg_trigger "
				"WHERE tgrelid = 'decodex.outbox'::pg_catalog.regclass "
				"AND tgname = 'outbox_terminal_retention'",
				env,
			) != "O":
				raise TestFailure("additional function fixture did not restore trigger state")
			privileged_function_root = work / "decodex-unsafe-additional-privileged-function"
			write_bootstrap_config(
				privileged_function_root,
				socket_dir,
				port,
				PRIVILEGED_FUNCTION_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			unsafe_roots.append(privileged_function_root)

			create_database(TRIGGER_ESCAPE_DATABASE, env)
			set_contract_urls(env, socket_dir, port, TRIGGER_ESCAPE_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(TRIGGER_ESCAPE_DATABASE, RUNTIME_ROLE, env)
			psql_as(
				MIGRATION_ROLE,
				TRIGGER_ESCAPE_DATABASE,
				"CREATE FUNCTION public.indirect_owner_escape() RETURNS trigger "
				"LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, decodex AS $$ "
				"BEGIN EXECUTE 'ALTER TABLE decodex.outbox DISABLE TRIGGER "
				"outbox_terminal_retention'; RETURN NULL; END $$; "
				"REVOKE ALL ON FUNCTION public.indirect_owner_escape() FROM PUBLIC; "
				f"REVOKE ALL ON FUNCTION public.indirect_owner_escape() FROM {RUNTIME_ROLE}; "
				"CREATE TRIGGER accounts_indirect_owner_escape AFTER INSERT ON decodex.accounts "
				"FOR EACH STATEMENT EXECUTE FUNCTION public.indirect_owner_escape()",
				env,
			)
			if psql(
				TRIGGER_ESCAPE_DATABASE,
				f"SELECT has_function_privilege('{RUNTIME_ROLE}', "
				"'public.indirect_owner_escape()', 'EXECUTE'), "
				f"has_table_privilege('{RUNTIME_ROLE}', 'decodex.activity', 'UPDATE'), "
				f"has_table_privilege('{RUNTIME_ROLE}', 'decodex.outbox', 'TRIGGER')",
				env,
			) != "f|f|f":
				raise TestFailure("indirect trigger fixture leaked direct runtime authority")
			psql_as(
				RUNTIME_ROLE,
				TRIGGER_ESCAPE_DATABASE,
				"INSERT INTO decodex.accounts(account_id, display_label) "
				"VALUES ('91000000-0000-0000-0000-000000000001', 'indirect owner escape')",
				env,
			)
			if psql(
				TRIGGER_ESCAPE_DATABASE,
				"SELECT tgenabled = 'D' FROM pg_catalog.pg_trigger "
				"WHERE tgrelid = 'decodex.outbox'::pg_catalog.regclass "
				"AND tgname = 'outbox_terminal_retention'",
				env,
			) != "t":
				raise TestFailure("runtime-triggered owner effect did not execute")
			psql_as(
				MIGRATION_ROLE,
				TRIGGER_ESCAPE_DATABASE,
				"ALTER TABLE decodex.outbox ENABLE TRIGGER outbox_terminal_retention",
				env,
			)
			trigger_escape_root = work / "decodex-unsafe-indirect-trigger-owner-effect"
			write_bootstrap_config(
				trigger_escape_root,
				socket_dir,
				port,
				TRIGGER_ESCAPE_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			unsafe_roots.append(trigger_escape_root)

			create_database(EXTENSION_CONTROL_DATABASE, env)
			set_contract_urls(env, socket_dir, port, EXTENSION_CONTROL_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(EXTENSION_CONTROL_DATABASE, RUNTIME_ROLE, env)
			psql(
				"postgres",
				f"GRANT CREATE ON DATABASE {EXTENSION_CONTROL_DATABASE} TO {RUNTIME_ROLE}",
				env,
			)
			psql(
				EXTENSION_CONTROL_DATABASE,
				f"GRANT CREATE ON SCHEMA public, decodex TO {RUNTIME_ROLE}",
				env,
			)
			psql_as(
				RUNTIME_ROLE,
				EXTENSION_CONTROL_DATABASE,
				"CREATE EXTENSION hstore WITH SCHEMA public; "
				"CREATE COLLATION decodex.extension_control_member FROM pg_catalog.\"C\"; "
				"ALTER EXTENSION hstore ADD COLLATION decodex.extension_control_member",
				env,
			)
			psql(
				EXTENSION_CONTROL_DATABASE,
				f"ALTER COLLATION decodex.extension_control_member OWNER TO {MIGRATION_ROLE}; "
				f"REVOKE CREATE ON SCHEMA public, decodex FROM {RUNTIME_ROLE}",
				env,
			)
			psql(
				"postgres",
				f"REVOKE CREATE ON DATABASE {EXTENSION_CONTROL_DATABASE} FROM {RUNTIME_ROLE}",
				env,
			)
			if psql(
				EXTENSION_CONTROL_DATABASE,
				f"SELECT extension.extowner = '{RUNTIME_ROLE}'::pg_catalog.regrole, "
				f"owned_collation.collowner = '{MIGRATION_ROLE}'::pg_catalog.regrole, "
				"dependency.deptype = 'e' FROM pg_catalog.pg_extension AS extension "
				"JOIN pg_catalog.pg_depend AS dependency "
				"ON dependency.refclassid = 'pg_catalog.pg_extension'::pg_catalog.regclass "
				"AND dependency.refobjid = extension.oid "
				"JOIN pg_catalog.pg_collation AS owned_collation "
				"ON dependency.classid = 'pg_catalog.pg_collation'::pg_catalog.regclass "
				"AND dependency.objid = owned_collation.oid "
				"WHERE extension.extname = 'hstore' "
				"AND owned_collation.oid = "
				"'decodex.extension_control_member'::pg_catalog.regcollation",
				env,
			) != "t|t|t":
				raise TestFailure("extension dependency-control fixture is vacuous")
			if psql_as(
				RUNTIME_ROLE,
				EXTENSION_CONTROL_DATABASE,
				"BEGIN; DROP EXTENSION hstore; "
				"SELECT pg_catalog.to_regcollation('decodex.extension_control_member') IS NULL; "
				"ROLLBACK",
				env,
			) != "t":
				raise TestFailure("runtime extension owner could not drop the Decodex member")
			extension_control_root = work / "decodex-unsafe-extension-member-control"
			write_bootstrap_config(
				extension_control_root,
				socket_dir,
				port,
				EXTENSION_CONTROL_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			unsafe_roots.append(extension_control_root)
			env["DECODEX_TEST_UNSAFE_AUTHORITY_ROOTS"] = os.pathsep.join(
				str(root) for root in unsafe_roots
			)
			unsafe_authority_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
					"bootstrap_doctor", "--run-ignored", "all", "--",
					"isolated_postgres_overprivileged_runtime_is_unavailable", "--exact",
				],
				env,
			)

			create_database(FUNCTION_DATABASE, env)
			set_contract_urls(env, socket_dir, port, FUNCTION_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(FUNCTION_DATABASE, RUNTIME_ROLE, env)
			psql(
				FUNCTION_DATABASE,
				"CREATE OR REPLACE FUNCTION decodex.enforce_outbox_terminal_retention() "
				"RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$",
				env,
			)
			if psql(
				FUNCTION_DATABASE,
				"SELECT prosrc LIKE '%RETURN NEW%' AND prosrc NOT LIKE '%retention pruning%' "
				"FROM pg_catalog.pg_proc AS proc "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid = proc.pronamespace "
				"WHERE namespace.nspname = 'decodex' "
				"AND proc.proname = 'enforce_outbox_terminal_retention'",
				env,
			) != "t":
				raise TestFailure("same-metadata no-op retention fixture is vacuous")

			function_contract_root = work / "decodex-incompatible-function-contract"
			write_bootstrap_config(
				function_contract_root,
				socket_dir,
				port,
				FUNCTION_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)

			create_database(CONSTRAINT_DRIFT_DATABASE, env)
			set_contract_urls(env, socket_dir, port, CONSTRAINT_DRIFT_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(CONSTRAINT_DRIFT_DATABASE, RUNTIME_ROLE, env)
			assert_psql_rejected(
				RUNTIME_ROLE,
				CONSTRAINT_DRIFT_DATABASE,
				"INSERT INTO decodex.accounts(account_id, display_label) "
				"VALUES ('92000000-0000-0000-0000-000000000001', 'token=fixture-secret')",
				env,
				"canonical account credential boundary",
			)
			psql_as(
				MIGRATION_ROLE,
				CONSTRAINT_DRIFT_DATABASE,
				"ALTER TABLE decodex.accounts DROP CONSTRAINT accounts_no_credentials",
				env,
			)
			psql_as(
				RUNTIME_ROLE,
				CONSTRAINT_DRIFT_DATABASE,
				"INSERT INTO decodex.accounts(account_id, display_label) "
				"VALUES ('92000000-0000-0000-0000-000000000001', 'token=fixture-secret')",
				env,
			)
			if psql(
				CONSTRAINT_DRIFT_DATABASE,
				"SELECT count(*) FROM decodex.accounts WHERE account_id = "
				"'92000000-0000-0000-0000-000000000001'",
				env,
			) != "1":
				raise TestFailure("dropped credential constraint did not change the boundary")
			constraint_drift_root = work / "decodex-incompatible-credential-constraint"
			write_bootstrap_config(
				constraint_drift_root,
				socket_dir,
				port,
				CONSTRAINT_DRIFT_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)

			create_database(IDENTITY_CAST_DATABASE, env)
			set_contract_urls(env, socket_dir, port, IDENTITY_CAST_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(IDENTITY_CAST_DATABASE, RUNTIME_ROLE, env)
			psql(
				IDENTITY_CAST_DATABASE,
				"CREATE FUNCTION public.xy1315_uuid_to_text(pg_catalog.uuid) "
				"RETURNS pg_catalog.text LANGUAGE sql IMMUTABLE STRICT "
				"AS 'SELECT $1::pg_catalog.text'; "
				"CREATE CAST (pg_catalog.uuid AS pg_catalog.text) "
				"WITH FUNCTION public.xy1315_uuid_to_text(pg_catalog.uuid) AS IMPLICIT",
				env,
			)
			if psql(
				IDENTITY_CAST_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_cast AS conversion "
				"WHERE conversion.castsource='pg_catalog.uuid'::pg_catalog.regtype "
				"AND conversion.casttarget='pg_catalog.text'::pg_catalog.regtype "
				"AND conversion.castcontext='i'",
				env,
			) != "1":
				raise TestFailure("implicit UUID-to-text cast fixture is vacuous")
			identity_cast_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
					"postgres_store", "--run-ignored", "all", "--",
					"postgres_store_rejects_implicit_uuid_to_text_cast", "--exact",
				],
				env,
			)

			create_database(EXTERNAL_CASCADE_DATABASE, env)
			set_contract_urls(env, socket_dir, port, EXTERNAL_CASCADE_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(EXTERNAL_CASCADE_DATABASE, RUNTIME_ROLE, env)
			psql_as(
				MIGRATION_ROLE,
				EXTERNAL_CASCADE_DATABASE,
				"CREATE TABLE public.external_outbox_child ("
				"child_id bigint PRIMARY KEY, outbox_id bigint NOT NULL REFERENCES "
				"decodex.outbox(id) ON DELETE CASCADE); "
				"REVOKE ALL ON TABLE public.external_outbox_child FROM PUBLIC; "
				"INSERT INTO decodex.outbox (id, effect_key, aggregate_kind, aggregate_id, "
				"aggregate_revision, payload, state, effect_state, receipt, reconciliation, "
				"created_at, delivered_at, retain_until) OVERRIDING SYSTEM VALUE VALUES (920001, "
				"'external-cascade', 'account', 'fixture', 1, '{}', 'delivered', "
				"'receipt_recorded', '{\"ok\":true}', '{\"ok\":true}', "
				"date_trunc('milliseconds', clock_timestamp()) - interval '2 days', "
				"date_trunc('milliseconds', clock_timestamp()) - interval '1 day', "
				"date_trunc('milliseconds', clock_timestamp()) - interval '1 second'); "
				"INSERT INTO public.external_outbox_child(child_id, outbox_id) "
				"VALUES (1, 920001)",
				env,
			)
			assert_psql_rejected(
				RUNTIME_ROLE,
				EXTERNAL_CASCADE_DATABASE,
				"DELETE FROM public.external_outbox_child WHERE child_id = 1",
				env,
				"runtime direct external-child delete",
			)
			psql_as(
				RUNTIME_ROLE,
				EXTERNAL_CASCADE_DATABASE,
				"DELETE FROM decodex.outbox WHERE id = 920001",
				env,
			)
			if psql(
				EXTERNAL_CASCADE_DATABASE,
				"SELECT count(*) FROM public.external_outbox_child WHERE child_id = 1",
				env,
			) != "0":
				raise TestFailure("runtime parent delete did not exercise owner-mediated cascade")
			external_cascade_root = work / "decodex-incompatible-external-cascade"
			write_bootstrap_config(
				external_cascade_root,
				socket_dir,
				port,
				EXTERNAL_CASCADE_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)

			create_database(LEDGER_TAMPER_DATABASE, env)
			set_contract_urls(env, socket_dir, port, LEDGER_TAMPER_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(LEDGER_TAMPER_DATABASE, RUNTIME_ROLE, env)
			ledger_tamper_root = work / "decodex-incompatible-ledger-tamper"
			write_bootstrap_config(
				ledger_tamper_root,
				socket_dir,
				port,
				LEDGER_TAMPER_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			ledger_live_doctor_output = run_live_doctor_mutation(
				ledger_tamper_root,
				LEDGER_TAMPER_DATABASE,
				"UPDATE public.refinery_schema_history SET name = name || '_tampered' "
				"WHERE version = 1",
				"ledger-tamper",
				work,
				env,
			)
			if psql(
				LEDGER_TAMPER_DATABASE,
				"SELECT count(*), count(*) FILTER (WHERE name LIKE '%_tampered') "
				"FROM public.refinery_schema_history",
				env,
			) != "19|1":
				raise TestFailure("migration-ledger tamper did not preserve the row count")

			create_database(MISSING_EXTENSION_DATABASE, env)
			set_contract_urls(env, socket_dir, port, MISSING_EXTENSION_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(MISSING_EXTENSION_DATABASE, RUNTIME_ROLE, env)
			missing_extension_root = work / "decodex-incompatible-missing-pgcrypto"
			write_bootstrap_config(
				missing_extension_root,
				socket_dir,
				port,
				MISSING_EXTENSION_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			missing_extension_live_doctor_output = run_live_doctor_mutation(
				missing_extension_root,
				MISSING_EXTENSION_DATABASE,
				"DROP EXTENSION pgcrypto CASCADE",
				"missing-pgcrypto",
				work,
				env,
			)
			if psql(
				MISSING_EXTENSION_DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'",
				env,
			) != "0":
				raise TestFailure("missing-pgcrypto fixture retained the extension")
			env["DECODEX_TEST_INCOMPATIBLE_AUTHORITY_ROOTS"] = os.pathsep.join(
				[
					str(function_contract_root),
					str(constraint_drift_root),
					str(external_cascade_root),
					str(ledger_tamper_root),
					str(missing_extension_root),
				]
			)
			incompatible_authority_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
					"bootstrap_doctor", "--run-ignored", "all", "--",
					"isolated_postgres_incompatible_runtime_is_unavailable", "--exact",
				],
				env,
			)

			return "\n".join((
				unsafe_authority_output,
				identity_cast_output,
				ledger_live_doctor_output,
				missing_extension_live_doctor_output,
				incompatible_authority_output,
			))
		run_stage(
			orchestrator,
			"authority_safety_suite",
			authority_safety_suite,
			depends_on=("cluster_preflight",),
		)

		def hostile_search_path_suite() -> str:
			create_database(HOSTILE_SEARCH_DATABASE, env)
			set_contract_urls(env, socket_dir, port, HOSTILE_SEARCH_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			provision_runtime(HOSTILE_SEARCH_DATABASE, HOSTILE_SEARCH_ROLE, env)
			psql(
				HOSTILE_SEARCH_DATABASE,
				f"CREATE SCHEMA hostile; "
				f"CREATE TABLE hostile.refinery_schema_history (sentinel text); "
				f"CREATE TABLE hostile.pg_proc (sentinel text); "
				f"CREATE TABLE hostile.pg_class (sentinel text); "
				f"CREATE FUNCTION hostile.clock_timestamp() RETURNS timestamptz "
				f"LANGUAGE sql IMMUTABLE AS 'SELECT ''infinity''::timestamptz'; "
				f"CREATE FUNCTION hostile.octet_length(text) RETURNS integer "
				f"LANGUAGE sql IMMUTABLE AS 'SELECT 1'; "
				f"GRANT USAGE ON SCHEMA hostile TO {HOSTILE_SEARCH_ROLE}; "
				f"GRANT SELECT ON TABLE hostile.refinery_schema_history TO {HOSTILE_SEARCH_ROLE}; "
				f"GRANT EXECUTE ON FUNCTION hostile.clock_timestamp(), "
				f"hostile.octet_length(text) TO {HOSTILE_SEARCH_ROLE}; "
				f"ALTER ROLE {HOSTILE_SEARCH_ROLE} IN DATABASE {HOSTILE_SEARCH_DATABASE} "
				f"SET search_path = hostile, public, pg_catalog",
				env,
			)
			if psql_as(
				HOSTILE_SEARCH_ROLE,
				HOSTILE_SEARCH_DATABASE,
				"SELECT (current_schemas(false))[1], "
				"'refinery_schema_history'::regclass::oid = "
				"'hostile.refinery_schema_history'::regclass::oid, "
				"'pg_proc'::regclass::oid = 'hostile.pg_proc'::regclass::oid, "
				"'pg_class'::regclass::oid = 'hostile.pg_class'::regclass::oid, "
				"'pg_catalog.pg_proc'::regclass::oid <> 'hostile.pg_proc'::regclass::oid, "
				"'pg_catalog.pg_class'::regclass::oid <> 'hostile.pg_class'::regclass::oid",
				env,
			) != "hostile|t|t|t|t|t":
				raise TestFailure(
					"hostile search_path fixture does not shadow ledger and catalog names"
				)
			if psql_as(
				HOSTILE_SEARCH_ROLE,
				HOSTILE_SEARCH_DATABASE,
				"SELECT clock_timestamp() = 'infinity'::timestamptz, octet_length('abc') = 1, "
				"NOT decodex.is_canonical_media_type('not a media type')",
				env,
			) != "t|t|t":
				raise TestFailure("secure Decodex function path did not resist callable shadowing")
			hostile_startup_path_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
					"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
					"postgres_session_search_path_startup_fixture", "--exact",
				],
				env,
			)
			if psql_as(
				HOSTILE_SEARCH_ROLE,
				HOSTILE_SEARCH_DATABASE,
				"INSERT INTO decodex.conversations (conversation_id,title) VALUES "
				"('4f000000-0000-4000-8000-000000000001','hostile callable fixture'); "
				"UPDATE decodex.conversations SET status='archived',revision=2 "
				"WHERE conversation_id='4f000000-0000-4000-8000-000000000001' "
				"RETURNING isfinite(updated_at)",
				env,
			) != "t":
				raise TestFailure("hostile callable shadow reached Decodex runtime DML")
			hostile_search_root = work / "decodex-hostile-search-path"
			write_bootstrap_config(
				hostile_search_root,
				socket_dir,
				port,
				HOSTILE_SEARCH_DATABASE,
				MIGRATION_ROLE,
				HOSTILE_SEARCH_ROLE,
			)
			env["DECODEX_TEST_HOSTILE_SEARCH_ROOT"] = str(hostile_search_root)
			hostile_search_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
					"bootstrap_doctor", "--run-ignored", "all", "--",
					"isolated_postgres_hostile_search_path_is_unavailable", "--exact",
				],
				env,
			)

			return "\n".join((hostile_startup_path_output, hostile_search_output))
		run_stage(
			orchestrator,
			"hostile_search_path_suite",
			hostile_search_path_suite,
			depends_on=("cluster_preflight",),
		)

		aggregate_context: dict[str, object] = {}
		def primary_restore_suite() -> str:
			live_manifest_path = work / "schema-manifest-live.json"
			set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
			capture_restore_checkpoint(
				restore_report,
				"primary_post_command",
				live_manifest_path,
				DATABASE,
				env,
			)
			live_quota_snapshot = quota_authority_snapshot(DATABASE, env)
			live_quota = json.loads(live_quota_snapshot)
			if len(live_quota["windows"]) != 8 or len(live_quota["exclusions"]) != 2:
				acceptance_failures.append(
					"live quota snapshot is not the populated V8/V14 eight-window fixture"
				)
			if any(row["dispatch_enabled"] for row in live_quota["exclusions"]):
				acceptance_failures.append("live quota exclusion unexpectedly enables dispatch")
			dump_path = work / "decodex_xy1267.dump"
			primary_restore_ready = True
			try:
				run(["pg_dump", "-Fc", "-f", str(dump_path), DATABASE], env)
				create_database(RESTORE_DATABASE, env)
				run(
					["pg_restore", "--exit-on-error", "-d", RESTORE_DATABASE, str(dump_path)],
					env,
				)
			except TestFailure as error:
				primary_restore_ready = False
				record_restore_stage(
					restore_report, "primary_restore", "failed", error=str(error)
				)
			else:
				record_restore_stage(restore_report, "primary_restore", "passed")
			restored_manifest_path = work / "schema-manifest-restored.json"
			if restore_stage_ready(
				restore_report, "primary_restored_capture"
			) and primary_restore_ready:
				set_contract_urls(env, socket_dir, port, RESTORE_DATABASE, RUNTIME_ROLE)
				capture_restore_checkpoint(
					restore_report,
					"primary_restored",
					restored_manifest_path,
					RESTORE_DATABASE,
					env,
				)
				restored_quota_snapshot = quota_authority_snapshot(RESTORE_DATABASE, env)
				if restored_quota_snapshot != live_quota_snapshot:
					record_restore_stage(
						restore_report,
						"primary_sequence_state",
						"failed",
						error="dump/restore changed immutable quota authority evidence",
					)
				else:
					record_restore_stage(restore_report, "primary_sequence_state", "passed")
			else:
				checkpoints = restore_report["checkpoints"]
				assert isinstance(checkpoints, dict)
				checkpoints["primary_restored"] = unavailable_checkpoint(
					"primary_restore prerequisite is unavailable"
				)
			restore_output = record_restore_production_check(
				restore_report,
				"primary_store_restored",
				lambda: run(
					[
						"cargo", "nextest", "run", "-p", "decodex-postgres",
						"--features", "test-support", "--test", "postgres_store",
						"--run-ignored", "all", "--",
						"postgres_store_restored_contract", "--exact",
					],
					env,
				),
			)
			managed_repository_restore_output = record_restore_production_check(
				restore_report,
				"managed_repository_restored",
				lambda: run_managed_repository_test(
					"postgres_managed_repository_restored_contract", env
				),
			)
			checkpoints = restore_report["checkpoints"]
			assert isinstance(checkpoints, dict)
			live_document = checkpoints["primary_post_command"]
			assert isinstance(live_document, dict)
			live_authority = component_manifest(
				live_document, "authority", require_complete=False
			)
			aggregate_context["dump_path"] = dump_path
			aggregate_context["live_authority"] = live_authority
			return "\n".join((restore_output, managed_repository_restore_output))
		run_stage(
			orchestrator,
			"primary_restore_suite",
			primary_restore_suite,
			depends_on=(
				"primary_foundation", "role_profile_suite", "runtime_session_suite",
			),
		)

		canary_markers = (role_setting_canary_guc, role_setting_secret_canary)
		def redaction_canary_suite() -> None:
			live_authority = aggregate_context.get("live_authority")
			canary_manifest_path = work / "schema-manifest-role-setting-canary.json"
			psql_secret(
				DATABASE,
				f"ALTER ROLE xy1272_missing_secret_log_role SET {role_setting_canary_guc} = "
				f"'{role_setting_secret_canary}'",
				env,
				expect_failure=True,
			)
			assert_postgres_logs_redact((log_path,), canary_markers)
			psql_secret(
				DATABASE,
				f"ALTER ROLE {RUNTIME_ROLE} SET {role_setting_canary_guc} = "
				f"'{role_setting_secret_canary}'",
				env,
			)
			try:
				catalog_probe = psql_secret(
					DATABASE,
					"SELECT count(*) FROM pg_catalog.pg_db_role_setting AS setting "
					"CROSS JOIN LATERAL pg_catalog.unnest(setting.setconfig) AS item(value) "
					f"WHERE setting.setrole='{RUNTIME_ROLE}'::pg_catalog.regrole "
					"AND setting.setdatabase=0 "
					f"AND item.value='{role_setting_canary_guc}={role_setting_secret_canary}'",
					env,
				)
				if catalog_probe != "1":
					raise TestFailure("secret-bearing role-setting canary is absent from the live catalog")
				canary_env = env.copy()
				set_contract_urls(canary_env, socket_dir, port, DATABASE, RUNTIME_ROLE)
				dump_schema_manifest(canary_manifest_path, DATABASE, canary_env)
				canary_document = load_semantic_manifest(canary_manifest_path)
				canary_authority = component_manifest(
					canary_document, "authority", require_complete=False
				)
				if canary_authority is None or live_authority is None:
					record_restore_stage(
						restore_report,
						"authority_canary_manifest",
						"unavailable",
						blocked_by=["primary_post_command_capture"],
					)
				elif canary_authority == live_authority:
					raise TestFailure("secret-bearing role setting did not change configured authority")
				else:
					record_restore_stage(
						restore_report, "authority_canary_manifest", "passed"
					)
				canary_manifest = canary_manifest_path.read_text(encoding="utf-8")
				if (
					role_setting_secret_canary in canary_manifest
					or role_setting_canary_guc in canary_manifest
				):
					raise TestFailure(
						"configured authority manifest serialized a role-setting canary"
					)
			finally:
				psql_secret(
					DATABASE,
					f"ALTER ROLE {RUNTIME_ROLE} RESET {role_setting_canary_guc}",
					env,
				)
			if psql_secret(
				DATABASE,
				"SELECT count(*) FROM pg_catalog.pg_db_role_setting AS setting "
				"CROSS JOIN LATERAL pg_catalog.unnest(setting.setconfig) AS item(value) "
				f"WHERE setting.setrole='{RUNTIME_ROLE}'::pg_catalog.regrole "
				"AND setting.setdatabase=0 "
				f"AND pg_catalog.split_part(item.value,'=',1)='{role_setting_canary_guc}'",
				env,
			) != "0":
				raise TestFailure("secret-bearing role-setting canary was not restored")
			return None
		run_stage(
			orchestrator,
			"redaction_canary_suite",
			redaction_canary_suite,
			depends_on=("primary_restore_suite",),
		)

		def default_acl_restore_suite() -> str:
			dump_path = aggregate_context["dump_path"]
			if not isinstance(dump_path, Path):
				raise HarnessCorruption("primary restore dump path is invalid")
			create_database(DEFAULT_ACL_TAMPER_DATABASE, env)
			run(
				[
					"pg_restore", "--exit-on-error", "-d",
					DEFAULT_ACL_TAMPER_DATABASE, str(dump_path),
				],
				env,
			)
			default_acl_tamper_root = work / "decodex-incompatible-schema-default-acl"
			write_bootstrap_config(
				default_acl_tamper_root,
				socket_dir,
				port,
				DEFAULT_ACL_TAMPER_DATABASE,
				MIGRATION_ROLE,
				RUNTIME_ROLE,
			)
			default_acl_live_doctor_output = run_live_doctor_mutation(
				default_acl_tamper_root,
				DEFAULT_ACL_TAMPER_DATABASE,
				f"ALTER DEFAULT PRIVILEGES FOR ROLE {MIGRATION_ROLE} IN SCHEMA decodex "
				"GRANT EXECUTE ON FUNCTIONS TO PUBLIC",
				"schema-default-acl",
				work,
				env,
				unsafe_authority=True,
			)
			default_acl_probe = (
				"SELECT count(*) "
				"FROM pg_catalog.pg_default_acl AS default_acl "
				"JOIN pg_catalog.pg_namespace AS namespace "
				"ON namespace.oid=default_acl.defaclnamespace "
				f"WHERE default_acl.defaclrole='{MIGRATION_ROLE}'::pg_catalog.regrole "
				"AND namespace.nspname='decodex' AND default_acl.defaclobjtype='f' "
				"AND EXISTS (SELECT 1 FROM pg_catalog.aclexplode(default_acl.defaclacl) "
				"AS privilege WHERE privilege.grantee=0 "
				"AND privilege.privilege_type='EXECUTE')"
			)
			if psql(DEFAULT_ACL_TAMPER_DATABASE, default_acl_probe, env) != "1":
				raise TestFailure("schema-scoped PUBLIC default-ACL fixture is vacuous")

			default_acl_dump_path = work / "decodex_xy1315_default_acl.dump"
			run(
				[
					"pg_dump", "-Fc", "-f", str(default_acl_dump_path),
					DEFAULT_ACL_TAMPER_DATABASE,
				],
				env,
			)
			create_database(DEFAULT_ACL_RESTORE_DATABASE, env)
			run(
				[
					"pg_restore", "--exit-on-error", "-d",
					DEFAULT_ACL_RESTORE_DATABASE, str(default_acl_dump_path),
				],
				env,
			)
			if psql(DEFAULT_ACL_RESTORE_DATABASE, default_acl_probe, env) != "1":
				raise TestFailure("populated restore lost the schema-scoped PUBLIC default ACL")
			set_contract_urls(
				env, socket_dir, port, DEFAULT_ACL_RESTORE_DATABASE, RUNTIME_ROLE
			)
			default_acl_restore_output = run(
				[
					"cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
					"postgres_store", "--run-ignored", "all", "--",
					"postgres_store_rejects_schema_scoped_default_acl_restore", "--exact",
				],
				env,
			)
			return "\n".join((default_acl_live_doctor_output, default_acl_restore_output))
		run_stage(
			orchestrator,
			"default_acl_restore_suite",
			default_acl_restore_suite,
			depends_on=("primary_restore_suite",),
		)

		authority_drift_cases = [
			(
				f"ALTER ROLE {RUNTIME_ROLE} SET {role_setting_canary_guc} = "
				f"'{role_setting_secret_canary}'",
				f"ALTER ROLE {RUNTIME_ROLE} RESET {role_setting_canary_guc}",
				"credential-setting-redaction",
				True,
			),
			(
				f"GRANT CONNECT ON DATABASE {DATABASE} TO {MISSING_SELECT_ROLE}",
				f"REVOKE CONNECT ON DATABASE {DATABASE} FROM {MISSING_SELECT_ROLE}",
				"database-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER DATABASE {DATABASE} OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER DATABASE {DATABASE} OWNER TO {MIGRATION_ROLE}",
				"database-owner",
				True,
			),
			(
				f"GRANT USAGE ON SCHEMA decodex TO {MISSING_SELECT_ROLE}",
				f"REVOKE USAGE ON SCHEMA decodex FROM {MISSING_SELECT_ROLE}",
				"namespace-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER SCHEMA decodex OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER SCHEMA decodex OWNER TO {MIGRATION_ROLE}",
				"namespace-owner",
				True,
			),
			(
				f"GRANT SELECT ON TABLE decodex.accounts TO {MISSING_SELECT_ROLE}",
				f"REVOKE SELECT ON TABLE decodex.accounts FROM {MISSING_SELECT_ROLE}",
				"relation-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER TABLE decodex.accounts OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER TABLE decodex.accounts OWNER TO {MIGRATION_ROLE}",
				"relation-owner",
				True,
			),
			(
				f"GRANT SELECT (account_id) ON TABLE decodex.accounts TO {MISSING_SELECT_ROLE}",
				f"REVOKE SELECT (account_id) ON TABLE decodex.accounts FROM {MISSING_SELECT_ROLE}",
				"column-acl",
				False,
			),
			(
				f"GRANT USAGE ON SEQUENCE decodex.activity_sequence_seq TO {MISSING_SELECT_ROLE}",
				f"REVOKE USAGE ON SEQUENCE decodex.activity_sequence_seq FROM {MISSING_SELECT_ROLE}",
				"sequence-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER TABLE decodex.activity OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER TABLE decodex.activity OWNER TO {MIGRATION_ROLE}",
				"identity-sequence-owner-via-table",
				True,
			),
			(
				f"GRANT USAGE ON TYPE decodex.account_state TO {MISSING_SELECT_ROLE}",
				f"REVOKE USAGE ON TYPE decodex.account_state FROM {MISSING_SELECT_ROLE}",
				"type-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER TYPE decodex.account_state OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER TYPE decodex.account_state OWNER TO {MIGRATION_ROLE}",
				"type-owner",
				True,
			),
			(
				f"GRANT EXECUTE ON FUNCTION decodex.is_canonical_media_type(text) TO {MISSING_SELECT_ROLE}",
				f"REVOKE EXECUTE ON FUNCTION decodex.is_canonical_media_type(text) FROM {MISSING_SELECT_ROLE}",
				"function-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER FUNCTION decodex.is_canonical_media_type(text) OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER FUNCTION decodex.is_canonical_media_type(text) OWNER TO {MIGRATION_ROLE}",
				"function-owner",
				True,
			),
			(
				f"GRANT SELECT ON TABLE public.refinery_schema_history TO {MISSING_SELECT_ROLE}",
				f"REVOKE SELECT ON TABLE public.refinery_schema_history FROM {MISSING_SELECT_ROLE}",
				"migration-ledger-equivalent-grantee",
				False,
			),
			(
				f"GRANT SELECT (version) ON TABLE public.refinery_schema_history "
				f"TO {MISSING_SELECT_ROLE}",
				f"REVOKE SELECT (version) ON TABLE public.refinery_schema_history "
				f"FROM {MISSING_SELECT_ROLE}",
				"migration-ledger-column-acl-equivalent-grantee",
				False,
			),
			(
				f"ALTER TABLE public.refinery_schema_history OWNER TO {FUNCTION_OWNER_ROLE}",
				f"ALTER TABLE public.refinery_schema_history OWNER TO {MIGRATION_ROLE}",
				"migration-ledger-owner",
				True,
			),
			(
				f"ALTER DEFAULT PRIVILEGES FOR ROLE {MIGRATION_ROLE} IN SCHEMA decodex "
				f"GRANT SELECT ON TABLES TO {MISSING_SELECT_ROLE}",
				f"ALTER DEFAULT PRIVILEGES FOR ROLE {MIGRATION_ROLE} IN SCHEMA decodex "
				f"REVOKE SELECT ON TABLES FROM {MISSING_SELECT_ROLE}",
				"default-acl-equivalent-grantee",
				False,
			),
			(
				"CREATE RULE xy1272_unexpected_rule AS ON INSERT TO decodex.accounts "
				"DO ALSO NOTHING",
				"DROP RULE xy1272_unexpected_rule ON decodex.accounts",
				"rule-definition",
				False,
			),
			(
				"CREATE POLICY xy1272_unexpected_policy ON decodex.accounts TO PUBLIC USING (true)",
				"DROP POLICY xy1272_unexpected_policy ON decodex.accounts",
				"policy-definition",
				False,
			),
			(
				f"GRANT {FUNCTION_OWNER_ROLE} TO {RUNTIME_ROLE} "
				"WITH ADMIN FALSE, INHERIT FALSE, SET FALSE",
				f"REVOKE {FUNCTION_OWNER_ROLE} FROM {RUNTIME_ROLE}",
				"membership-no-options",
				True,
			),
			(
				f"GRANT {FUNCTION_OWNER_ROLE} TO {RUNTIME_ROLE} "
				"WITH ADMIN TRUE, INHERIT TRUE, SET TRUE",
				f"REVOKE {FUNCTION_OWNER_ROLE} FROM {RUNTIME_ROLE}",
				"membership-admin-inherit-set",
				True,
			),
			(
				f"ALTER ROLE {MIGRATION_ROLE} RENAME TO decodex_migration_renamed",
				f"ALTER ROLE decodex_migration_renamed RENAME TO {MIGRATION_ROLE}",
				"configured-migration-rename",
				True,
			),
			(
				f"ALTER ROLE {RUNTIME_ROLE} RENAME TO decodex_runtime_renamed",
				f"ALTER ROLE decodex_runtime_renamed RENAME TO {RUNTIME_ROLE}",
				"configured-runtime-rename",
				True,
			),
		]
		for role in (MIGRATION_ROLE, RUNTIME_ROLE):
			for suffix, mutation, restore in (
				("superuser", "SUPERUSER", "NOSUPERUSER"),
				("inherit", "INHERIT", "NOINHERIT"),
				("create-role", "CREATEROLE", "NOCREATEROLE"),
				("create-database", "CREATEDB", "NOCREATEDB"),
				("login", "NOLOGIN", "LOGIN"),
				("replication", "REPLICATION", "NOREPLICATION"),
				("bypass-rls", "BYPASSRLS", "NOBYPASSRLS"),
				("connection-limit", "CONNECTION LIMIT 7", "CONNECTION LIMIT -1"),
				(
					"validity",
					"VALID UNTIL '2030-01-01 00:00:00+00'",
					"VALID UNTIL 'infinity'",
				),
			):
				authority_drift_cases.append((
					f"ALTER ROLE {role} {mutation}",
					f"ALTER ROLE {role} {restore}",
					f"{role}-{suffix}",
					True,
				))
			authority_drift_cases.extend((
				(
					f"ALTER ROLE {role} SET statement_timeout = '1s'",
					f"ALTER ROLE {role} RESET statement_timeout",
					f"{role}-global-setting",
					True,
				),
				(
					f"ALTER ROLE {role} IN DATABASE {DATABASE} "
					"SET search_path = hostile, public, pg_catalog",
					f"ALTER ROLE {role} IN DATABASE {DATABASE} RESET search_path",
					f"{role}-database-setting",
					True,
				),
			))
		previous_restoration: str | None = None
		for mutation, restore, case, cluster_authority in authority_drift_cases:
			secret_sql = case == "credential-setting-redaction"
			mutation_probe = None
			if case == "migration-ledger-column-acl-equivalent-grantee":
				mutation_probe = (
					f"SELECT pg_catalog.has_column_privilege('{MISSING_SELECT_ROLE}', "
					"'public.refinery_schema_history', 'version', 'SELECT') "
					f"AND NOT pg_catalog.has_table_privilege('{MISSING_SELECT_ROLE}', "
					"'public.refinery_schema_history', 'SELECT')"
				)
			mutation_attempted = False
			def authority_mutation_probe() -> str:
				nonlocal mutation_attempted
				mutation_attempted = True
				output = run_live_doctor_mutation(
					bootstrap_root,
					DATABASE,
					mutation,
					case,
					work,
					env,
					unsafe_authority=True,
					cluster_authority=cluster_authority,
					secret_sql=secret_sql,
					mutation_probe=mutation_probe,
				)
				if secret_sql and (
					role_setting_secret_canary in output
					or role_setting_canary_guc in output
				):
					raise HarnessCorruption("doctor output redaction failed")
				return output
			probe_stage = f"authority_drift::{case}::mutation_probe"
			probe_dependencies = ("primary_foundation", "bootstrap_configuration")
			if previous_restoration is not None:
				probe_dependencies += (previous_restoration,)
			run_stage(
				orchestrator,
				probe_stage,
				authority_mutation_probe,
				depends_on=probe_dependencies,
			)
			restoration_stage = f"authority_drift::{case}::restoration"
			if mutation_attempted:
				def restore_authority_fixture() -> None:
					if cluster_authority:
						if secret_sql:
							psql_secret(DATABASE, restore, env)
						else:
							psql(DATABASE, restore, env)
					else:
						psql_as(MIGRATION_ROLE, DATABASE, restore, env)
				run_stage(
					orchestrator,
					restoration_stage,
					restore_authority_fixture,
					always_run=True,
				)
			else:
				orchestrator.stages[restoration_stage] = {
					"status": "blocked",
					"blocked_by": [probe_stage],
				}
			previous_restoration = restoration_stage
		run_stage(
			orchestrator,
			"authority_drift_redaction",
			lambda: assert_postgres_logs_redact((log_path,), canary_markers),
			depends_on=((previous_restoration,) if previous_restoration else ()),
		)
		def final_acceptance_evidence() -> str:
			final_source_binding = frozen_source_binding()
			if final_source_binding != source_binding:
				raise TestFailure("frozen PostgreSQL gate source binding changed during execution")
			restore_failures = finalize_restore_report(restore_report)
			checkpoints = restore_report["checkpoints"]
			assert isinstance(checkpoints, dict)
			live_document = checkpoints["primary_post_command"]
			restored_document = checkpoints["primary_restored"]
			assert isinstance(live_document, dict) and isinstance(restored_document, dict)
			live_schema = component_manifest(live_document, "schema")
			live_authority = component_manifest(live_document, "authority")
			restored_schema = component_manifest(restored_document, "schema")
			restored_authority = component_manifest(restored_document, "authority")
			expected_digests = {
				"schema": rust_digest_constant("SCHEMA_CONTRACT_SHA256"),
				"authority": rust_digest_constant("CONFIGURED_AUTHORITY_SHA256"),
			}
			if None in (live_schema, live_authority, restored_schema, restored_authority):
				acceptance_failures.append(
					"complete PostgreSQL artifacts are unavailable after restore finalization"
				)
			live_components = {
				"schema": live_schema if isinstance(live_schema, str) else "",
				"authority": live_authority if isinstance(live_authority, str) else "",
			}
			restored_components = {
				"schema": restored_schema if isinstance(restored_schema, str) else "",
				"authority": restored_authority if isinstance(restored_authority, str) else "",
			}
			actual_digests = {
				component: hashlib.sha256(live_components[component].encode("utf-8")).hexdigest()
				for component in ("schema", "authority")
			}
			restored_digests = {
				component: hashlib.sha256(
					restored_components[component].encode("utf-8")
				).hexdigest()
				for component in ("schema", "authority")
			}
			for component in ("schema", "authority"):
				if actual_digests[component] != expected_digests[component]:
					acceptance_failures.append(
						f"live {component} digest mismatch: expected "
						f"{expected_digests[component]}, actual {actual_digests[component]}"
					)
				if restored_digests[component] != expected_digests[component]:
					acceptance_failures.append(
						f"restored {component} digest mismatch: expected "
						f"{expected_digests[component]}, actual {restored_digests[component]}"
					)
				if live_components[component] != restored_components[component]:
					acceptance_failures.append(
						f"{component} manifest differs between live and restored clusters"
					)
			migration_inventory = json.loads(psql(
				DATABASE,
				"SELECT pg_catalog.json_agg(pg_catalog.json_build_object("
				"'version', version, 'name', name, 'checksum', checksum) ORDER BY version)::text "
				"FROM public.refinery_schema_history",
				env,
			))
			artifact_evidence = {
				"schema": "decodex/xy-1353-frozen-postgres-evidence/1",
				"source": source_binding,
				"database_bindings": {
					name: document.get("binding")
					for name, document in checkpoints.items()
					if isinstance(document, dict)
				},
				"migration_inventory": migration_inventory,
				"configured_authority_inventory_rows": (
					len(json.loads(live_components["authority"]))
					if live_components["authority"] else None
				),
				"schema_manifest_rows": (
					len(json.loads(live_components["schema"]))
					if live_components["schema"] else None
				),
				"expected_digests": expected_digests,
				"actual_digests": actual_digests,
				"restored_digests": restored_digests,
				"sequence_state_receipt": live_document["sequence_state"],
				"production_restore_checks": restore_report["production_checks"],
				"restore_parity": all(
					live_components[component] == restored_components[component]
					for component in ("schema", "authority")
				) and live_document["sequence_state"] == restored_document["sequence_state"],
			}
			acceptance_failures.extend(
				f"structured restore: {failure}" for failure in restore_failures
			)
			if acceptance_failures:
				diagnostics = {
					"failures": acceptance_failures,
					"expected_digests": expected_digests,
					"actual_digests": actual_digests,
					"restored_digests": restored_digests,
					"checkpoint_state": restore_report,
				}
				raise TestFailure(
					"aggregate V14-V20 PostgreSQL acceptance failure:\n"
					+ json.dumps(diagnostics, sort_keys=True)
				)
			return json.dumps(artifact_evidence, sort_keys=True)
		final_dependencies = (
			"primary_restore_suite",
			"redaction_canary_suite",
			"default_acl_restore_suite",
			"authority_drift_redaction",
		)
		run_stage(
			orchestrator,
			"final_acceptance_evidence",
			final_acceptance_evidence,
			depends_on=final_dependencies,
		)
		for output in orchestrator.outputs:
			print(output)
		if orchestrator.primary_failure is not None:
			raise orchestrator.primary_failure
		return 0
	finally:
		active_error = sys.exc_info()[1]
		primary_failure = active_error is not None
		if isinstance(active_error, Exception) and orchestrator.primary_failure is None:
			orchestrator.primary_failure = active_error
			orchestrator.corruption = corruption_failure(active_error) or HarnessCorruption(
				f"unexpected {type(active_error).__name__}: {active_error}"
			)
			orchestrator.scheduling_stopped = True
			orchestrator.stages["unhandled_harness_corruption"] = {
				"status": "failed",
				"classification": "harness_corruption",
				"error": str(orchestrator.corruption),
			}
		stop_error: Exception | None = None
		teardown_error: Exception | None = None
		report_error: Exception | None = None
		stop_failures: list[str] = []
		def teardown_status() -> ClusterStatus:
			try:
				return (
					postgres_status(data_dir, env)
					if data_dir.exists() else ClusterStatus.STOPPED
				)
			except Exception as error:
				stop_failures.append(f"PostgreSQL status failed:\n{error}")
				return ClusterStatus.UNKNOWN
		status = teardown_status()
		if status is ClusterStatus.RUNNING:
			try:
				run(["pg_ctl", "-D", str(data_dir), "-m", "fast", "-w", "stop"], env)
			except Exception as error:
				stop_failures.append(f"fast shutdown failed:\n{error}")
			status = teardown_status()
		if status is ClusterStatus.RUNNING:
			try:
				run(["pg_ctl", "-D", str(data_dir), "-m", "immediate", "-w", "stop"], env)
			except Exception as error:
				stop_failures.append(f"immediate shutdown failed:\n{error}")
			status = teardown_status()
		stop_diagnostics = "\n\n".join(stop_failures)
		if stop_diagnostics:
			stop_diagnostics = f"\n\nShutdown diagnostics:\n{stop_diagnostics}"
		if status is ClusterStatus.RUNNING:
			stop_error = TestFailure(
				f"PostgreSQL is still running; retained isolated cluster at {work}"
				f"{stop_diagnostics}"
			)
		elif status is ClusterStatus.UNKNOWN:
			stop_error = TestFailure(
				f"PostgreSQL status is unknown; retained isolated cluster at {work}"
				f"{stop_diagnostics}"
			)
		if status is ClusterStatus.STOPPED:
			if stop_diagnostics:
				print(
					f"PostgreSQL shutdown recovered after an error:{stop_diagnostics}",
					file=sys.stderr,
				)
			try:
				shutil.rmtree(work)
			except Exception as error:
				teardown_error = TestFailure(
					f"failed to remove stopped task-owned PostgreSQL directory {work}: {error}"
				)
		elif stop_error is not None:
			teardown_error = stop_error
		if orchestrator.cluster_started:
			orchestrator.stages["teardown"] = (
				{"status": "passed"} if teardown_error is None
				else {"status": "failed", "error": str(teardown_error)}
			)
			reported_primary = orchestrator.primary_failure
			if reported_primary is None and isinstance(active_error, Exception):
				reported_primary = active_error
			if reported_primary is None:
				reported_primary = teardown_error
			orchestrator.stages["final_report"] = {"status": "passed"}
			report = {
				"schema": "decodex/postgres-aggregate-stage-report/1",
				"primary_failure": (
					None if reported_primary is None else str(reported_primary)
				),
				"stages": orchestrator.stages,
			}
			try:
				print(json.dumps(report, sort_keys=True), flush=True)
			except Exception as error:
				report_error = HarnessCorruption(
					f"final aggregate report emission failed: {error}"
				)
				orchestrator.stages["final_report"] = {
					"status": "failed",
					"classification": "harness_corruption",
					"error": str(report_error),
				}
		if teardown_error is not None:
			if primary_failure:
				print(teardown_error, file=sys.stderr)
			else:
				raise teardown_error
		if report_error is not None and not primary_failure and teardown_error is None:
			raise report_error


def publish_completed_authority_candidate(
	publication: AuthorityCandidatePublication,
) -> None:
	source_binding = publication.receipt.get("source_binding")
	if not isinstance(source_binding, dict):
		raise TestFailure("authority candidate receipt has no source binding")
	final_binding = frozen_source_binding()
	if (
		source_binding.get("start") != final_binding
		or source_binding.get("end") != final_binding
	):
		raise TestFailure("authority candidate source binding changed before publication")
	publish_authority_candidate(publication.output_path, publication.receipt)


if __name__ == "__main__":
	result = main()
	if isinstance(result, AuthorityCandidatePublication):
		publish_completed_authority_candidate(result)
		raise SystemExit(0)
	raise SystemExit(result)
