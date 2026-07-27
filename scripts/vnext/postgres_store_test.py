#!/usr/bin/env python3
"""Run XY-1267 integration tests in a disposable PostgreSQL 18 cluster."""

from __future__ import annotations

from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass, field
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
RESTORE_PREREQUISITE_SOURCE_DATABASE = "decodex_xy1421_restore_prerequisite_source"
RESTORE_PREREQUISITE_R1_DATABASE = "decodex_xy1421_restore_prerequisite_r1"
AUTHORITY_CAPTURE_RESTORE_EDGES = (
	("source_to_restored_once", "source", "restored_once"),
	("restored_once_to_restored_twice", "restored_once", "restored_twice"),
)
AUTHORITY_CANDIDATE_SCHEMA = "decodex/postgres-authority-candidate/3"
AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES = 128 * 1024
POSTGRES_TOOL_NAMES = ("initdb", "pg_ctl", "psql", "pg_dump", "pg_restore")
POSTGRES_TOOL_VERSION_MAX_BYTES = 512
POSTGRES_ARCHIVE_TOC_MAX_BYTES = 8 * 1024 * 1024
POSTGRES_PRIVATE_COMMAND_TIMEOUT_SECONDS = 30.0
RESTORE_PREREQUISITE_CLI = "--capture-authority-restore-prerequisite-v2"
RESTORE_PREREQUISITE_GATE_SCHEMA = (
	"decodex/postgres-restore-prerequisite-r1-gate/2"
)
RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA = (
	"decodex/postgres-restore-prerequisite-r1-diagnostic/2"
)
RESTORE_PREREQUISITE_DEFINITION_SCHEMA = (
	"decodex/postgres-restore-prerequisite-r1-definition/2"
)
RESTORE_PREREQUISITE_ARCHIVE_GRAMMAR = (
	"decodex/postgresql-18-pg-restore-list/1"
)
RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT = (
	"53bb20b8e43a6199c3aa578269cee8b941ed549fd8f10db0dce361a03016524a"
)
RESTORE_PREREQUISITE_SQL = (
	"CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public VERSION '1.4';"
)
RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS = (
	"cli",
	"output_contract",
	"source_binding_preflight",
	"temporary_root",
	"tool_discovery",
	"toolchain_preflight",
	"private_work",
	"cluster_init",
	"cluster_start",
	"role_setup",
	"source_binding_gate_start",
	"toolchain_gate_start",
	"server_version",
	"definition_binding",
	"source_database_created",
	"source_migrated",
	"source_provisioned",
	"source_populated",
	"source_semantic_authority",
	"source_archive_created",
	"archive_declaration_guarded",
	"restore_database_fresh_template0",
	"restore_pgcrypto_absent",
	"restore_prerequisite_created",
	"restored_once",
	"restored_once_semantic_authority",
	"semantic_authority_equal",
	"invocation_policy",
	"source_binding_gate_end",
	"toolchain_gate_end",
	"privacy_validation",
	"stopped_after_restored_once",
)
RESTORE_PREREQUISITE_CLEANUP_OWNERS = (
	"cluster_stop",
	"private_work_cleanup",
)
RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER = "cleanup_finalization"
RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES = (
	(),
	("private_work_cleanup",),
	("cluster_stop", "private_work_cleanup"),
)
RESTORE_PREREQUISITE_CLEANUP_OWNER_STATES = (
	"pending", "active", "completed",
)
RESTORE_PREREQUISITE_CLEANUP_FAULT_POINTS = (
	"before_first_cleanup_owner",
	"after_cluster_stop_action_before_transition",
	"between_cluster_stop_and_private_work_cleanup",
	"after_private_work_cleanup_action_before_transition",
	"during_cleanup_finalization",
)
RESTORE_PREREQUISITE_RECEIPT_LIFECYCLE_CHECKPOINTS = (
	"receipt_validation",
	"receipt_source_binding",
	"receipt_publication",
)
RESTORE_PREREQUISITE_LIFECYCLE_CHECKPOINTS = (
	*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
	RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
	*RESTORE_PREREQUISITE_RECEIPT_LIFECYCLE_CHECKPOINTS,
)
RESTORE_PREREQUISITE_INVOCATION_POLICIES = (
	"source_database_once",
	"source_migration_once",
	"source_provisioning_once",
	"source_population_once",
	"source_semantic_once",
	"source_dump_once",
	"archive_guard_once",
	"restore_database_once",
	"pgcrypto_absence_check_once",
	"restore_prerequisite_once",
	"restore_once",
	"restored_semantic_once",
)
RESTORE_PREREQUISITE_DIAGNOSTIC_REASONS = (
	"contract_invalid",
	"authority_unavailable",
	"changed",
	"operation_failed",
	"archive_declaration_invalid",
	"target_not_fresh",
	"duplicate_invocation",
	"invocation_policy_failed",
	"semantic_authority_changed",
	"cleanup_failed",
	"receipt_invalid",
	"publication_failed",
	"interrupted",
	"harness_corruption",
)
RESTORE_PREREQUISITE_EXPECTED_REASONS = {
	"cli": ("contract_invalid",),
	"output_contract": ("contract_invalid",),
	"source_binding_preflight": ("authority_unavailable",),
	"temporary_root": ("contract_invalid",),
	"tool_discovery": ("authority_unavailable",),
	"toolchain_preflight": ("authority_unavailable",),
	"private_work": ("operation_failed",),
	"cluster_init": ("operation_failed",),
	"cluster_start": ("operation_failed",),
	"role_setup": ("operation_failed",),
	"source_binding_gate_start": ("authority_unavailable", "changed"),
	"toolchain_gate_start": ("authority_unavailable", "changed"),
	"server_version": ("authority_unavailable",),
	"definition_binding": ("contract_invalid",),
	"source_database_created": ("operation_failed", "duplicate_invocation"),
	"source_migrated": ("operation_failed", "duplicate_invocation"),
	"source_provisioned": ("operation_failed", "duplicate_invocation"),
	"source_populated": ("operation_failed", "duplicate_invocation"),
	"source_semantic_authority": ("operation_failed", "duplicate_invocation"),
	"source_archive_created": ("operation_failed", "duplicate_invocation"),
	"archive_declaration_guarded": (
		"archive_declaration_invalid", "duplicate_invocation",
	),
	"restore_database_fresh_template0": ("operation_failed",),
	"restore_pgcrypto_absent": ("operation_failed", "target_not_fresh"),
	"restore_prerequisite_created": ("operation_failed",),
	"restored_once": ("operation_failed",),
	"restored_once_semantic_authority": (
		"operation_failed", "duplicate_invocation",
	),
	"semantic_authority_equal": ("semantic_authority_changed",),
	"invocation_policy": ("duplicate_invocation", "invocation_policy_failed"),
	"source_binding_gate_end": ("authority_unavailable", "changed"),
	"toolchain_gate_end": ("authority_unavailable", "changed"),
	"privacy_validation": ("contract_invalid",),
	"stopped_after_restored_once": ("contract_invalid",),
	"cluster_stop": ("cleanup_failed",),
	"private_work_cleanup": ("cleanup_failed",),
	"cleanup_finalization": (),
	"receipt_validation": ("receipt_invalid",),
	"receipt_source_binding": ("authority_unavailable", "changed"),
	"receipt_publication": ("publication_failed",),
}
RESTORE_PREREQUISITE_REASON_MATRIX = tuple(
	(
		checkpoint,
		(*RESTORE_PREREQUISITE_EXPECTED_REASONS[checkpoint],
			"interrupted", "harness_corruption"),
	)
	for checkpoint in (
		*RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS,
		*RESTORE_PREREQUISITE_LIFECYCLE_CHECKPOINTS,
	)
)
RESTORE_PREREQUISITE_ALLOWED_REASONS = dict(
	RESTORE_PREREQUISITE_REASON_MATRIX
)
RESTORE_PREREQUISITE_CLEANUP_STATUSES = (
	"not_required", "passed", "failed",
)
AUTHORITY_RESTORE_TARGET_ALLOWED_REASONS = {
	"archive_declaration_guarded": (
		"archive_declaration_invalid", "stage_failed",
	),
	"restore_database_fresh_template0": (
		"duplicate_invocation", "stage_failed",
	),
	"restore_pgcrypto_absent": ("stage_failed", "target_not_fresh"),
	"restore_prerequisite_created": ("stage_failed",),
	"restored_once": ("stage_failed",),
	"gate": ("invocation_policy_failed",),
}
PG18_TOC_ENTRY_RE = re.compile(
	r"(?P<dump_id>[1-9][0-9]*); "
	r"(?P<table_oid>0|[1-9][0-9]*) "
	r"(?P<object_oid>0|[1-9][0-9]*) "
	r"(?P<body>[ -~]+)"
)
PG18_TOC_EXTENSION_RE = re.compile(
	r"EXTENSION (?P<namespace>[^ ]+) (?P<tag>[^ ]+) (?P<owner>[^ ]*)"
)
PG18_TOC_DUMP_VERSION_RE = re.compile(
	r";     Dumped by pg_dump version: 18(?:\.[0-9]+)*(?: [ -~]+)?"
)
POSTGRES_START_LOG_EXCERPT_MAX_BYTES = 4 * 1024
# Darwin's sockaddr_un.sun_path is 104 bytes, including the terminating NUL.
PORTABLE_UNIX_SOCKET_PATH_MAX_BYTES = 104
GIT_READ_TIMEOUT_SECONDS = 5.0
GIT_METADATA_MAX_BYTES = 4 * 1024
GIT_COMMIT_MAX_BYTES = 64 * 1024
GIT_STATUS_MAX_BYTES = 64 * 1024
GIT_PATH_LIST_MAX_BYTES = 64 * 1024
GIT_AUTHORITY_SOURCE_MAX_BYTES = 512 * 1024
SECRET_LOGGING_READY_MARKER = "XY1272_SECRET_LOGGING_READY"
SECRET_SQL_DONE_MARKER = "XY1272_SECRET_SQL_DONE"
SECRET_LOGGING_READY_TIMEOUT_SECONDS = 10.0
SECRET_LOGGING_FRAME_MAX_BYTES = 256
SECRET_LOGGING_HANDSHAKE_MAX_BYTES = 512
SECRET_LOGGING_EXPECTED_FRAMES = (
	"panic|panic|none|off|-1|-1|0|0|0|0|off|off|off|off|off|off|off|off|stderr",
	SECRET_LOGGING_READY_MARKER,
)
SECRET_LOGGING_PRELUDE = (
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
	f"\\echo {SECRET_LOGGING_READY_MARKER}\n"
).encode("ascii")
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
DIRECT_MISSING_EXTENSION_DATABASE = "decodex_xy1364_direct_missing_extension"
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
MANAGED_RUN_DATABASE = "decodex_xy1417_managed_run_v26"
MANAGED_RUN_RESTORE_DATABASE = "decodex_xy1417_managed_run_v26_restore"
MANAGED_REPOSITORY_DATABASE = "decodex_xy1364_managed_repositories"
POSTGRES_PREPARATION_DATABASE = "decodex_xy1422_preparation"
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
	"decodex.replace_routing_policy_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog._uuid,pg_catalog._int8,decodex._routing_member_disposition,decodex._codex_capability)",
	"decodex.publish_routing_evidence_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,decodex.role_profile_role,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,decodex._codex_capability,decodex._capability_evidence_state)",
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.prepare_codex_experiment_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.mark_codex_experiment_creation_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.bind_codex_experiment_start_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.text)",
	"decodex.read_codex_experiment_start_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.mark_codex_experiment_title_set_possible_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text)",
	"decodex.attest_codex_experiment_retained_title_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.record_attested_codex_experiment_observation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.codex_experiment_observation_kind,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_continuation_plan_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_execution_decision_exact(pg_catalog.uuid)",
	"decodex.read_managed_run_execution_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_waiting_usage_wake_transition_exact(pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.register_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.claim_due_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.fire_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.cancel_waiting_usage_wake_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.read_account_registry_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_reset_card_account_admission_exact(pg_catalog.uuid,pg_catalog.text)",
	"decodex.prepare_account_operation_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.account_operation_kind,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text)",
	"decodex.set_account_operation_target_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
	"decodex.advance_account_operation_exact(pg_catalog.uuid,decodex.account_operation_phase,decodex.account_operation_phase,pg_catalog.text)",
	"decodex.read_unsettled_account_operations_exact(pg_catalog.int8)",
	"decodex.read_account_operation_exact(pg_catalog.uuid)",
	"decodex.update_account_administration_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.bool)",
	"decodex.replace_account_routing_control_exact(pg_catalog.int8,decodex.account_selection_mode,pg_catalog.uuid,pg_catalog._uuid)",
	"decodex.read_account_routing_control_exact()",
	"decodex.observe_account_quota_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int4,pg_catalog.int8,pg_catalog.int8)",
	"decodex.observe_account_quota_error_exact(pg_catalog.uuid,pg_catalog.int4,decodex.account_quota_observation_error,pg_catalog.int8)",
	"decodex.observe_account_store_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,decodex.account_store_observation)",
	"decodex.attest_codex_account_capability_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.bool)",
	"decodex.record_account_migration_receipt_exact(pg_catalog.text,pg_catalog.jsonb,pg_catalog.jsonb,pg_catalog.jsonb,pg_catalog.int4)",
	"decodex.prepare_process_generation_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.bind_process_generation_identity_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8)",
	"decodex.mark_process_generation_ready_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_stopping_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_process_generation_death_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.process_generation_loss_reason)",
	"decodex.record_process_generation_death_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,decodex.process_generation_death_evidence_kind,pg_catalog.text,pg_catalog.int8,pg_catalog.text,pg_catalog.int8,pg_catalog.int8,pg_catalog.text)",
	"decodex.project_process_generations_after_supervisor_loss_exact()",
	"decodex.read_process_generations_exact(pg_catalog.uuid,pg_catalog.bool,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text)",
	"decodex.authorize_provider_attempt_dispatch_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.cancel_provider_attempt_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.mark_provider_attempt_unknown_exact(pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_unknown_reason)",
	"decodex.record_provider_attempt_positive_evidence_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text)",
	"decodex.project_provider_attempts_after_supervisor_loss_exact()",
	"decodex.read_provider_attempts_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.provider_attempt_state,pg_catalog.uuid,pg_catalog.int8)",
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
	"decodex.enforce_process_generation_transition()",
	"decodex.record_process_generation_transition()",
	"decodex.forbid_process_generation_history_mutation()",
	"decodex.enforce_provider_attempt_transition()",
	"decodex.enforce_provider_attempt_binding()",
	"decodex.record_provider_attempt_transition()",
	"decodex.enforce_provider_attempt_turn_materialization()",
	"decodex.forbid_provider_attempt_history_mutation()",
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
	"decodex.process_generation_state",
	"decodex.process_generation_control_kind",
	"decodex.process_generation_isolation_kind",
	"decodex.process_generation_loss_reason",
	"decodex.process_generation_death_evidence_kind",
	"decodex.provider_attempt_state",
	"decodex.provider_attempt_consumer_kind",
	"decodex.provider_attempt_unknown_reason",
	"decodex.provider_attempt_evidence_source",
	"decodex.provider_attempt_terminal_outcome",
	"decodex.account_provider_kind",
	"decodex.account_operation_kind",
	"decodex.account_operation_phase",
	"decodex.account_selection_mode",
	"decodex.account_store_observation",
	"decodex.account_quota_observation_error",
)
AUTHORITY_ANCHOR_SIGNATURE = (
	"decodex.prepare_provider_attempt_exact(pg_catalog.uuid,"
	"decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,"
	"pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,"
	"pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,"
	"pg_catalog.uuid,pg_catalog.text)"
)
UPGRADE_RUNTIME_EXECUTE_SIGNATURES = (
	"decodex.resolve_routing_snapshot_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.route_account_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid)",
	"decodex.plan_continuation_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.bytea,pg_catalog.text,pg_catalog.text,pg_catalog.int4,pg_catalog.int4,pg_catalog.text,pg_catalog.bool,pg_catalog.int4,pg_catalog._text,pg_catalog._text,pg_catalog._int8,pg_catalog._text,pg_catalog._int8,pg_catalog._int8,pg_catalog._text,pg_catalog._text,pg_catalog._text,pg_catalog._int8)",
	"decodex.read_execution_decision_exact(pg_catalog.uuid)",
	"decodex.read_managed_run_execution_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_account_registry_exact(pg_catalog.uuid,pg_catalog.int8)",
	"decodex.read_reset_card_account_admission_exact(pg_catalog.uuid,pg_catalog.text)",
	"decodex.prepare_account_operation_exact(pg_catalog.uuid,pg_catalog.uuid,decodex.account_operation_kind,pg_catalog.text,pg_catalog.bool,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text)",
	"decodex.set_account_operation_target_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid)",
	"decodex.advance_account_operation_exact(pg_catalog.uuid,decodex.account_operation_phase,decodex.account_operation_phase,pg_catalog.text)",
	"decodex.read_unsettled_account_operations_exact(pg_catalog.int8)",
	"decodex.read_account_operation_exact(pg_catalog.uuid)",
	"decodex.update_account_administration_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.text,pg_catalog.bool)",
	"decodex.replace_account_routing_control_exact(pg_catalog.int8,decodex.account_selection_mode,pg_catalog.uuid,pg_catalog._uuid)",
	"decodex.read_account_routing_control_exact()",
	"decodex.observe_account_quota_exact(pg_catalog.uuid,pg_catalog.int4,pg_catalog.int4,pg_catalog.int8,pg_catalog.int8)",
	"decodex.observe_account_quota_error_exact(pg_catalog.uuid,pg_catalog.int4,decodex.account_quota_observation_error,pg_catalog.int8)",
	"decodex.observe_account_store_exact(pg_catalog.uuid,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,decodex.account_store_observation)",
	"decodex.attest_codex_account_capability_exact(pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.bool,pg_catalog.bool)",
	"decodex.record_account_migration_receipt_exact(pg_catalog.text,pg_catalog.jsonb,pg_catalog.jsonb,pg_catalog.jsonb,pg_catalog.int4)",
	"decodex.prepare_process_generation_exact(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,decodex.process_generation_control_kind,decodex.process_generation_isolation_kind,pg_catalog.int8,pg_catalog.int4,pg_catalog.int8,pg_catalog.text,pg_catalog.uuid,decodex.account_provider_kind,pg_catalog.text,pg_catalog.text,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid)",
	"decodex.read_process_generations_exact(pg_catalog.uuid,pg_catalog.bool,pg_catalog.uuid,pg_catalog.int8)",
)
PRE_V27_RUNTIME_TYPE_NAMES = (
	"decodex.provider_attempt_state",
	"decodex.provider_attempt_consumer_kind",
	"decodex.provider_attempt_unknown_reason",
	"decodex.provider_attempt_evidence_source",
	"decodex.provider_attempt_terminal_outcome",
)
UPGRADE_RUNTIME_TYPE_NAMES = (
	"decodex.account_provider_kind",
	"decodex.account_operation_kind",
	"decodex.account_operation_phase",
	"decodex.account_selection_mode",
	"decodex.account_store_observation",
	"decodex.account_quota_observation_error",
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
SEMANTIC_AUTHORITY_SCHEMA = "decodex/postgres-semantic-authority/2"
SEMANTIC_AUTHORITY_DEFINITION_SCHEMA = (
	"decodex/postgres-semantic-authority-definition/1"
)
SEMANTIC_AUTHORITY_FINGERPRINT_DOMAIN = (
	b"decodex/postgres-semantic-authority-fingerprint/1\0"
)
SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT = (
	"e78835be102ead879faa4f54569bb8c747ef03d014f899f68f75feaaf5f1a77f"
)
SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA = "decodex/postgres-semantic-authority-diagnostic/2"
SEMANTIC_AUTHORITY_DIAGNOSTIC_CHECKPOINTS = frozenset((
	"source",
	"restored_once",
	"restored_twice",
))
SEMANTIC_AUTHORITY_MAX_PREDICATES = 128
SEMANTIC_AUTHORITY_FAILURE_POLICIES = frozenset((
	"unsafe",
	"incompatible",
	"unsafe_if_excess_otherwise_incompatible",
))
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


class SemanticAuthorityDiagnostic(TestFailure):
	"""Carry the existing closed semantic-authority diagnostic without reparsing it."""

	def __init__(self, serialized: str) -> None:
		self.serialized = serialized
		super().__init__("authority candidate semantic diagnostic: " + serialized)


class RestorePrerequisiteGateAbort(TestFailure):
	"""Stop the v2 gate after its fixed state owner records a failure."""

	def __init__(self) -> None:
		super().__init__("restore prerequisite gate stopped")


class RestorePrerequisiteExpectedFailure(TestFailure):
	"""Select one fixed reason at the active v2 gate owner."""

	def __init__(self, reason: str) -> None:
		self.reason = reason
		super().__init__("restore prerequisite operation failed")


class AuthorityRestoreTargetFailure(TestFailure):
	"""Carry one fixed restore-target checkpoint and classification."""

	def __init__(self, checkpoint: str, reason: str) -> None:
		if reason not in AUTHORITY_RESTORE_TARGET_ALLOWED_REASONS.get(checkpoint, ()):
			raise HarnessCorruption("restore-target failure classification is invalid")
		self.checkpoint = checkpoint
		self.reason = reason
		super().__init__(f"authority restore target failed: {checkpoint}:{reason}")


@dataclass
class RestorePrerequisiteGateState:
	"""Own v2 gate progress, fixed failure projection, cleanup, and publication."""

	_output_path: Path | None = None
	_source_binding: dict[str, str] | None = None
	_toolchain_fingerprint: str | None = None
	_invocation_policy: dict[str, bool] | None = None
	_secret_markers: tuple[str, ...] = ()
	_completed_checkpoints: list[str] = field(default_factory=list)
	_primary_checkpoint: str | None = None
	_primary_reason: str | None = None
	_semantic_authority_diagnostic: dict[str, object] | None = None
	_current_checkpoint: str | None = None
	_output_contract_validated: bool = False
	_cleanup_started: bool = False
	_cleanup_finalized: bool = False
	_cleanup_required_owners: tuple[str, ...] = ()
	_cleanup_completed_owners: list[str] = field(default_factory=list)
	_cleanup_owner_status: dict[str, str] = field(default_factory=dict)
	_cleanup_pending_owner: str | None = None
	_cleanup_finalization_status: str = "not_started"
	_cleanup_failed: bool = False
	_cleanup_status: str = "not_required"
	_secondary_cleanup_reason: str | None = None
	_failure_document_repaired: bool = False
	_lifecycle_status: dict[str, str] = field(default_factory=dict)

	@property
	def output_path(self) -> Path | None:
		return self._output_path

	@property
	def output_contract_validated(self) -> bool:
		return self._output_contract_validated

	@property
	def source_binding(self) -> dict[str, str] | None:
		return None if self._source_binding is None else dict(self._source_binding)

	@property
	def toolchain_fingerprint(self) -> str | None:
		return self._toolchain_fingerprint

	@property
	def invocation_policy(self) -> dict[str, bool] | None:
		return (
			None if self._invocation_policy is None
			else dict(self._invocation_policy)
		)

	@property
	def completed_checkpoints(self) -> tuple[str, ...]:
		return tuple(self._completed_checkpoints)

	@property
	def primary_checkpoint(self) -> str | None:
		return self._primary_checkpoint

	@property
	def primary_reason(self) -> str | None:
		return self._primary_reason

	@property
	def cleanup_finalized(self) -> bool:
		return self._cleanup_finalized

	@property
	def cleanup_status(self) -> str:
		self._require_cleanup_proof()
		return self._cleanup_status

	@property
	def required_cleanup_owners(self) -> tuple[str, ...]:
		return self._cleanup_required_owners

	@property
	def completed_cleanup_owners(self) -> tuple[str, ...]:
		return tuple(self._cleanup_completed_owners)

	@property
	def cleanup_finalization_completed(self) -> bool:
		return (
			self._cleanup_finalized
			and self._cleanup_finalization_status == "completed"
		)

	def bind_output_path(self, path: Path) -> None:
		if self._current_checkpoint != "output_contract" or self._output_path is not None:
			raise HarnessCorruption("restore prerequisite output binding is invalid")
		self._output_path = path
		self._output_contract_validated = True

	def bind_source(self, binding: object) -> dict[str, str]:
		if (
			self._current_checkpoint != "source_binding_preflight"
			or self._source_binding is not None
		):
			raise HarnessCorruption("restore prerequisite source binding is invalid")
		validated = require_source_binding(
			binding, "restore prerequisite source binding is invalid"
		)
		self._source_binding = validated
		return dict(validated)

	def bind_toolchain(self, fingerprint: object) -> str:
		if (
			self._current_checkpoint != "toolchain_preflight"
			or self._toolchain_fingerprint is not None
			or not isinstance(fingerprint, str)
			or re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None
		):
			raise HarnessCorruption("restore prerequisite toolchain binding is invalid")
		self._toolchain_fingerprint = fingerprint
		return fingerprint

	def bind_invocation_policy(self, policy: object) -> dict[str, bool]:
		if (
			self._current_checkpoint != "invocation_policy"
			or self._invocation_policy is not None
			or not isinstance(policy, dict)
			or set(policy) != set(RESTORE_PREREQUISITE_INVOCATION_POLICIES)
			or any(value is not True for value in policy.values())
		):
			raise RestorePrerequisiteExpectedFailure("invocation_policy_failed")
		validated = {name: True for name in RESTORE_PREREQUISITE_INVOCATION_POLICIES}
		self._invocation_policy = validated
		return dict(validated)

	def bind_secret_markers(self, markers: tuple[str, ...]) -> None:
		if (
			self._current_checkpoint != "private_work"
			or self._secret_markers
			or not markers
			or any(not isinstance(marker, str) or not marker for marker in markers)
		):
			raise HarnessCorruption("restore prerequisite privacy state is invalid")
		self._secret_markers = markers

	def _next_execution_checkpoint(self) -> str | None:
		index = len(self._completed_checkpoints)
		if index == len(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS):
			return None
		return RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS[index]

	def _record_primary(
		self,
		checkpoint: str,
		reason: str,
		semantic_diagnostic: dict[str, object] | None = None,
	) -> None:
		if reason not in RESTORE_PREREQUISITE_ALLOWED_REASONS.get(checkpoint, ()):
			checkpoint = self._current_checkpoint or self._next_execution_checkpoint() or (
				"receipt_validation"
			)
			reason = "harness_corruption"
			semantic_diagnostic = None
		if self._primary_checkpoint is None:
			self._primary_checkpoint = checkpoint
			self._primary_reason = reason
			self._semantic_authority_diagnostic = semantic_diagnostic

	def _classify_failure(
		self, checkpoint: str, error: BaseException
	) -> tuple[str, dict[str, object] | None]:
		if isinstance(error, (KeyboardInterrupt, SystemExit)):
			return "interrupted", None
		if isinstance(error, SemanticAuthorityDiagnostic):
			if any(marker in error.serialized for marker in self._secret_markers):
				return "harness_corruption", None
			try:
				diagnostic = parse_restore_prerequisite_semantic_diagnostic(
					error.serialized, checkpoint, self._source_binding
				)
			except (TestFailure, TypeError, ValueError):
				return "harness_corruption", None
			return "operation_failed", diagnostic
		if isinstance(error, AuthorityRestoreTargetFailure):
			if (
				error.checkpoint == checkpoint
				and error.reason in RESTORE_PREREQUISITE_ALLOWED_REASONS.get(
					checkpoint, ()
				)
			):
				return error.reason, None
			return "harness_corruption", None
		if isinstance(error, RestorePrerequisiteExpectedFailure):
			if error.reason in RESTORE_PREREQUISITE_ALLOWED_REASONS.get(checkpoint, ()):
				return error.reason, None
			return "harness_corruption", None
		if isinstance(error, (
			HarnessCorruption, AssertionError, AttributeError, IndexError, KeyError,
			TypeError, ValueError,
		)):
			return "harness_corruption", None
		if isinstance(error, (TestFailure, OSError, subprocess.SubprocessError)):
			expected_reasons = RESTORE_PREREQUISITE_EXPECTED_REASONS[checkpoint]
			return (
				expected_reasons[0] if expected_reasons else "harness_corruption",
				None,
			)
		return "harness_corruption", None

	def _invoke(
		self,
		checkpoint: str,
		action: Callable[[], object],
		*,
		execution: bool,
		allow_after_primary: bool,
	) -> object:
		if self._current_checkpoint is not None:
			self._record_primary(self._current_checkpoint, "harness_corruption")
			raise RestorePrerequisiteGateAbort() from None
		if self._primary_checkpoint is not None and not allow_after_primary:
			raise RestorePrerequisiteGateAbort() from None
		if execution:
			expected = self._next_execution_checkpoint()
			if checkpoint != expected:
				if expected is not None:
					self._record_primary(expected, "harness_corruption")
				raise RestorePrerequisiteGateAbort() from None
		elif checkpoint not in RESTORE_PREREQUISITE_LIFECYCLE_CHECKPOINTS:
			self._record_primary(
				self._next_execution_checkpoint() or "receipt_validation",
				"harness_corruption",
			)
			raise RestorePrerequisiteGateAbort() from None
		self._current_checkpoint = checkpoint
		try:
			result = action()
		except RestorePrerequisiteGateAbort:
			raise
		except BaseException as error:
			reason, semantic_diagnostic = self._classify_failure(checkpoint, error)
			self._record_primary(checkpoint, reason, semantic_diagnostic)
			raise RestorePrerequisiteGateAbort() from None
		finally:
			self._current_checkpoint = None
		if execution:
			self._completed_checkpoints.append(checkpoint)
		return result

	def run(self, checkpoint: str, action: Callable[[], object]) -> object:
		return self._invoke(
			checkpoint, action, execution=True, allow_after_primary=False
		)

	def _require_cleanup_proof(self) -> None:
		required = self._cleanup_required_owners
		completed = tuple(self._cleanup_completed_owners)
		if (
			not self._cleanup_started
			or not self._cleanup_finalized
			or self._cleanup_finalization_status != "completed"
			or self._cleanup_pending_owner is not None
			or self._current_checkpoint in (
				*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
				RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
			)
			or required not in RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES
			or completed != required[:len(completed)]
			or set(self._cleanup_owner_status) != set(required)
			or any(
				self._cleanup_owner_status.get(owner)
				not in RESTORE_PREREQUISITE_CLEANUP_OWNER_STATES
				for owner in required
			)
			or any(
				self._cleanup_owner_status.get(owner) != "completed"
				for owner in completed
			)
			or any(
				(self._cleanup_owner_status.get(owner) == "completed")
				!= (owner in completed)
				for owner in required
			)
			or sum(
				self._cleanup_owner_status.get(owner) == "active"
				for owner in required
			) > 1
		):
			raise HarnessCorruption("restore prerequisite cleanup proof is invalid")
		if self._cleanup_status == "not_required":
			valid = not required and not completed and not self._cleanup_failed
		elif self._cleanup_status == "passed":
			valid = bool(required) and completed == required and not self._cleanup_failed
		elif self._cleanup_status == "failed":
			valid = self._cleanup_failed
		else:
			valid = False
		if not valid:
			raise HarnessCorruption("restore prerequisite cleanup result is invalid")

	def begin_cleanup(
		self,
		cluster_stop_applicable: bool,
		private_work_exists: bool,
	) -> None:
		if (
			self._cleanup_started
			or self._cleanup_finalized
			or type(cluster_stop_applicable) is not bool
			or type(private_work_exists) is not bool
			or cluster_stop_applicable and not private_work_exists
		):
			self._record_primary(
				RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
				"harness_corruption",
			)
			raise RestorePrerequisiteGateAbort() from None
		required = (
			RESTORE_PREREQUISITE_CLEANUP_OWNERS
			if cluster_stop_applicable else
			("private_work_cleanup",) if private_work_exists else
			()
		)
		self._cleanup_started = True
		self._cleanup_required_owners = required
		self._cleanup_owner_status = {owner: "pending" for owner in required}
		self._cleanup_pending_owner = (
			required[0] if required
			else RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
		)
		if not required:
			self._cleanup_finalization_status = "pending"

	def run_cleanup(
		self, checkpoint: str, action: Callable[[], object]
	) -> object:
		if (
			checkpoint not in RESTORE_PREREQUISITE_CLEANUP_OWNERS
			or not self._cleanup_started
			or self._cleanup_finalized
			or self._cleanup_pending_owner != checkpoint
			or self._cleanup_owner_status.get(checkpoint) != "pending"
			or self._current_checkpoint is not None
		):
			raise HarnessCorruption("restore prerequisite cleanup owner is invalid")
		self._cleanup_owner_status[checkpoint] = "active"
		self._current_checkpoint = checkpoint
		return action()

	def complete_cleanup_owner(self, checkpoint: str) -> None:
		if (
			self._current_checkpoint != checkpoint
			or self._cleanup_pending_owner != checkpoint
			or self._cleanup_owner_status.get(checkpoint) != "active"
			or tuple(self._cleanup_completed_owners)
			!= self._cleanup_required_owners[:len(self._cleanup_completed_owners)]
			or len(self._cleanup_completed_owners) >= len(self._cleanup_required_owners)
			or self._cleanup_required_owners[len(self._cleanup_completed_owners)]
			!= checkpoint
		):
			raise HarnessCorruption("restore prerequisite cleanup transition is invalid")
		self._cleanup_owner_status[checkpoint] = "completed"
		self._cleanup_completed_owners.append(checkpoint)
		self._current_checkpoint = None
		next_index = len(self._cleanup_completed_owners)
		if next_index < len(self._cleanup_required_owners):
			self._cleanup_pending_owner = self._cleanup_required_owners[next_index]
		else:
			self._cleanup_pending_owner = (
				RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
			)
			self._cleanup_finalization_status = "pending"

	def begin_cleanup_finalization(self) -> None:
		if (
			not self._cleanup_started
			or self._cleanup_finalized
			or self._cleanup_failed
			or self._current_checkpoint is not None
			or self._cleanup_pending_owner
			!= RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
			or self._cleanup_finalization_status != "pending"
			or tuple(self._cleanup_completed_owners)
			!= self._cleanup_required_owners
		):
			raise HarnessCorruption("restore prerequisite cleanup finalization is invalid")
		self._cleanup_finalization_status = "active"
		self._current_checkpoint = RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER

	def finish_cleanup(self) -> None:
		if (
			not self._cleanup_started
			or self._cleanup_finalized
			or self._cleanup_failed
			or self._current_checkpoint
			!= RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
			or self._cleanup_pending_owner
			!= RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
			or self._cleanup_finalization_status != "active"
			or tuple(self._cleanup_completed_owners)
			!= self._cleanup_required_owners
		):
			raise HarnessCorruption("restore prerequisite cleanup finalization is invalid")
		self._cleanup_status = (
			"passed" if self._cleanup_required_owners else "not_required"
		)
		self._cleanup_finalization_status = "completed"
		self._cleanup_pending_owner = None
		self._current_checkpoint = None
		self._cleanup_finalized = True
		self._require_cleanup_proof()

	def _cleanup_failure_owner(self) -> str:
		if self._current_checkpoint in (
			*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
			RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
		):
			return self._current_checkpoint
		if self._cleanup_pending_owner in (
			*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
			RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
		):
			return self._cleanup_pending_owner
		completed = tuple(self._cleanup_completed_owners)
		if (
			self._cleanup_required_owners
			in RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES
			and completed
			== self._cleanup_required_owners[:len(completed)]
			and len(completed) < len(self._cleanup_required_owners)
		):
			return self._cleanup_required_owners[len(completed)]
		return RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER

	def capture_cleanup_failure(self, error: BaseException) -> None:
		owner = self._cleanup_failure_owner()
		had_execution_primary = (
			self._primary_checkpoint in RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS
		)
		reason, _ = self._classify_failure(owner, error)
		self._record_primary(owner, reason)
		self._cleanup_failed = True
		if had_execution_primary:
			self._secondary_cleanup_reason = "cleanup_failed"
		self._cleanup_status = "failed"
		self._cleanup_finalization_status = "completed"
		self._cleanup_pending_owner = None
		self._current_checkpoint = None
		self._cleanup_started = True
		self._cleanup_finalized = True

	def _repair_cleanup_for_failure_document(self) -> None:
		required = self._cleanup_required_owners
		if required not in RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES:
			required = ()
		completed: list[str] = []
		for expected, actual in zip(required, self._cleanup_completed_owners):
			if actual != expected:
				break
			completed.append(actual)
		owner = (
			required[len(completed)]
			if len(completed) < len(required) else
			RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER
		)
		had_non_cleanup_primary = (
			self._primary_checkpoint is not None
			and self._primary_checkpoint not in (
				*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
				RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
			)
		)
		self._record_primary(owner, "harness_corruption")
		self._cleanup_started = True
		self._cleanup_required_owners = required
		self._cleanup_completed_owners = completed
		self._cleanup_owner_status = {
			cleanup_owner: (
				"completed" if cleanup_owner in completed else "pending"
			)
			for cleanup_owner in required
		}
		self._cleanup_pending_owner = None
		self._cleanup_finalization_status = "completed"
		self._cleanup_failed = True
		self._cleanup_status = "failed"
		if had_non_cleanup_primary:
			self._secondary_cleanup_reason = "cleanup_failed"
		self._cleanup_finalized = True

	def ensure_cleanup_finalized_without_work(self) -> None:
		if not self._cleanup_started:
			try:
				self.begin_cleanup(False, False)
				self.begin_cleanup_finalization()
				self.finish_cleanup()
			except BaseException as error:
				self.capture_cleanup_failure(error)
		elif not self._cleanup_finalized:
			self.capture_cleanup_failure(HarnessCorruption(
				"restore prerequisite cleanup did not finalize"
			))
		else:
			try:
				self._require_cleanup_proof()
			except BaseException:
				self._failure_document_repaired = True
				self._repair_cleanup_for_failure_document()

	def run_receipt_lifecycle(
		self,
		checkpoint: str,
		action: Callable[[], object],
		*,
		recovery: bool = False,
	) -> object:
		if checkpoint not in {
			"receipt_validation", "receipt_source_binding", "receipt_publication",
		}:
			self._record_primary("receipt_validation", "harness_corruption")
			raise RestorePrerequisiteGateAbort() from None
		def owned_action() -> object:
			if not recovery:
				self._require_cleanup_proof()
			if (
				self._primary_checkpoint is None
				and tuple(self._completed_checkpoints)
				!= RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS
				and not recovery
			):
				raise HarnessCorruption("restore prerequisite execution is incomplete")
			previous = {
				"receipt_validation": None,
				"receipt_source_binding": "receipt_validation",
				"receipt_publication": "receipt_source_binding",
			}[checkpoint]
			if previous is not None and self._lifecycle_status.get(previous) != "passed":
				raise HarnessCorruption("restore prerequisite receipt order is invalid")
			status = self._lifecycle_status.get(checkpoint)
			if (
				status == "passed"
				and not (recovery and checkpoint == "receipt_validation")
				or status == "failed" and not recovery
			):
				raise HarnessCorruption("restore prerequisite receipt owner repeated")
			return action()
		try:
			result = self._invoke(
				checkpoint, owned_action, execution=False, allow_after_primary=True
			)
		except RestorePrerequisiteGateAbort:
			self._lifecycle_status[checkpoint] = "failed"
			raise
		self._lifecycle_status[checkpoint] = "passed"
		return result

	def lifecycle_passed(self, checkpoint: str) -> bool:
		return self._lifecycle_status.get(checkpoint) == "passed"

	def capture_unhandled(self, error: BaseException) -> None:
		if self._cleanup_started and not self._cleanup_finalized:
			self.capture_cleanup_failure(error)
			return
		if isinstance(error, RestorePrerequisiteGateAbort):
			return
		if self._primary_checkpoint is not None:
			return
		checkpoint = self._current_checkpoint or self._next_execution_checkpoint()
		if checkpoint is None:
			checkpoint = next(
				(
					owner for owner in (
						"receipt_validation", "receipt_source_binding",
						"receipt_publication",
					)
					if self._lifecycle_status.get(owner) != "passed"
				),
				"receipt_publication",
			)
		reason, semantic_diagnostic = self._classify_failure(checkpoint, error)
		self._record_primary(checkpoint, reason, semantic_diagnostic)

	def _failure_document(self) -> dict[str, object]:
		if (
			self._primary_checkpoint is None
			or self._primary_reason is None
		):
			raise HarnessCorruption("restore prerequisite failure state is incomplete")
		cleanup_status = self.cleanup_status
		document = {
			"acceptance": False,
			"cleanup_finalized": self.cleanup_finalization_completed,
			"cleanup_status": cleanup_status,
			"completed_cleanup_owners": list(self._cleanup_completed_owners),
			"completed_checkpoints": list(self._completed_checkpoints),
			"definition_fingerprint": RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT,
			"failure_document_repaired": self._failure_document_repaired,
			"passed": False,
			"primary_checkpoint": self._primary_checkpoint,
			"primary_reason": self._primary_reason,
			"required_cleanup_owners": list(self._cleanup_required_owners),
			"schema": RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA,
			"secondary_cleanup_reason": self._secondary_cleanup_reason,
			"semantic_authority_diagnostic": self._semantic_authority_diagnostic,
			"source_binding": (
				None if self._source_binding is None else dict(self._source_binding)
			),
		}
		return validate_restore_prerequisite_gate_diagnostic(document)

	def failure_document(self) -> dict[str, object]:
		return self._failure_document()

	def failure_document_with_fixed_fallback(self) -> dict[str, object]:
		if self._current_checkpoint != "receipt_validation":
			raise HarnessCorruption(
				"restore prerequisite failure document has no lifecycle owner"
			)
		try:
			return self._failure_document()
		except BaseException:
			self._failure_document_repaired = True
			try:
				self._require_cleanup_proof()
			except BaseException:
				self._repair_cleanup_for_failure_document()
			if self._primary_checkpoint is None or self._primary_reason is None:
				checkpoint = self._next_execution_checkpoint() or "receipt_validation"
				self._record_primary(checkpoint, "harness_corruption")
			return self._failure_document()


class StageActionFailure(RuntimeError):
	"""Carry one authoritative stage failure plus subordinate cleanup failures."""

	def __init__(self, primary: Exception, secondary: tuple[Exception, ...]) -> None:
		super().__init__(str(primary))
		self.primary = primary
		self.secondary = secondary


@dataclass
class StageOrchestrator:
	"""Local aggregate scheduler for meaningful PostgreSQL harness stages."""

	stages: dict[str, dict[str, object]]
	outputs: list[str]
	primary_failure: Exception | None = None
	corruption: Exception | None = None
	scheduling_stopped: bool = False


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
	except Exception as caught:
		error = caught.primary if isinstance(caught, StageActionFailure) else caught
		secondary_errors = caught.secondary if isinstance(caught, StageActionFailure) else ()
		corruption = corruption_failure(error)
		if corruption is None and not isinstance(error, TestFailure):
			corruption = HarnessCorruption(
				f"unexpected {type(error).__name__}: {error}"
			)
		failure = corruption or error
		stage_result: dict[str, object] = {
			"status": "failed",
			"classification": "harness_corruption" if corruption else "test_failure",
			"error": str(failure),
		}
		secondary_corruptions: list[HarnessCorruption] = []
		if secondary_errors:
			secondary_results: list[dict[str, str]] = []
			for secondary_error in secondary_errors:
				secondary_corruption = corruption_failure(secondary_error)
				if secondary_corruption is None:
					secondary_corruption = HarnessCorruption(
						f"unexpected secondary {type(secondary_error).__name__}: "
						f"{secondary_error}"
					)
				secondary_corruptions.append(secondary_corruption)
				secondary_results.append({
					"classification": "harness_corruption",
					"error": str(secondary_corruption),
				})
			stage_result["secondary_failures"] = secondary_results
		orchestrator.stages[name] = stage_result
		if orchestrator.primary_failure is None:
			orchestrator.primary_failure = failure
		if corruption is not None or secondary_corruptions:
			orchestrator.corruption = (
				orchestrator.corruption or corruption or secondary_corruptions[0]
			)
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
class RestorePrerequisiteGatePublication:
	"""An exception-free gate result awaiting post-cleanup atomic publication."""

	output_path: Path
	state: RestorePrerequisiteGateState


@dataclass
class AuthorityRestoreInvocations:
	"""Prevent duplicate guard, prerequisite, and restore calls in one process."""

	archive_guard: int = 0
	database_create: int = 0
	pgcrypto_absence_check: int = 0
	prerequisite_create: int = 0
	restore: int = 0
	targets: list[str] = field(default_factory=list)

	def begin_target(self, target: str) -> None:
		if target in self.targets:
			raise AuthorityRestoreTargetFailure(
				"restore_database_fresh_template0", "duplicate_invocation"
			)
		self.targets.append(target)

	def policy_results(self, expected_targets: tuple[str, ...]) -> dict[str, bool]:
		expected_count = len(expected_targets)
		return {
			"archive_guard_once": (
				self.archive_guard == expected_count
				and self.targets == list(expected_targets)
			),
			"restore_database_once": self.database_create == expected_count,
			"pgcrypto_absence_check_once": (
				self.pgcrypto_absence_check == expected_count
			),
			"restore_prerequisite_once": self.prerequisite_create == expected_count,
			"restore_once": self.restore == expected_count,
		}


@dataclass
class RestorePrerequisiteGateInvocations:
	"""Record each gate-owned source and semantic invocation exactly once."""

	source_database: int = 0
	source_migration: int = 0
	source_provisioning: int = 0
	source_population: int = 0
	source_semantic: int = 0
	source_dump: int = 0
	restored_semantic: int = 0

	def record(self, name: str) -> None:
		checkpoints = {
			"source_database": "source_database_created",
			"source_migration": "source_migrated",
			"source_provisioning": "source_provisioned",
			"source_population": "source_populated",
			"source_semantic": "source_semantic_authority",
			"source_dump": "source_archive_created",
			"restored_semantic": "restored_once_semantic_authority",
		}
		if name not in checkpoints:
			raise HarnessCorruption("restore prerequisite invocation name is invalid")
		if getattr(self, name) != 0:
			raise RestorePrerequisiteExpectedFailure("duplicate_invocation")
		setattr(self, name, 1)

	def policy_results(self) -> dict[str, bool]:
		return {
			"source_database_once": self.source_database == 1,
			"source_migration_once": self.source_migration == 1,
			"source_provisioning_once": self.source_provisioning == 1,
			"source_population_once": self.source_population == 1,
			"source_semantic_once": self.source_semantic == 1,
			"source_dump_once": self.source_dump == 1,
			"restored_semantic_once": self.restored_semantic == 1,
		}


@dataclass(frozen=True)
class PhaseAAuthorityReceipt:
	"""Validated immutable derivation evidence consumed only by Phase B."""

	document: dict[str, object]
	sha256: str


class AuthorityClassification(Enum):
	"""Production PostgreSQL classification expected from one named scenario."""

	UNSAFE_DATABASE_AUTHORITY = "unsafe_database_authority"
	DATABASE_INCOMPATIBLE = "database_incompatible"


class AuthorityStoreError(Enum):
	"""Exact concrete StoreError expected before its bootstrap projection."""

	UNSAFE_AUTHORITY = "unsafe_authority"
	INCOMPATIBLE = "incompatible"
	MIGRATION = "migration"


@dataclass(frozen=True)
class AuthorityScenario:
	"""One independent case database verified before its semantic adversarial scenario."""

	case_id: str
	expected_store_error: AuthorityStoreError
	expected: AuthorityClassification
	baseline_migration_url: str
	baseline_runtime_url: str
	case_migration_url: str
	case_runtime_url: str
	admin_url: str
	mutation_sql: str
	precondition_sql: str
	postcondition_sql: str
	pre_runtime_rejected_sql: str | None = None
	pre_runtime_rejected_sqlstate: str | None = None
	post_runtime_rejected_sql: str | None = None
	post_runtime_rejected_sqlstate: str | None = None
	runtime_effect_sql: str | None = None
	restore_sql: str | None = None
	restore_postcondition_sql: str | None = None
	invariant_sql: str | None = None


def authority_scenario_payload(scenarios: list[AuthorityScenario]) -> str:
	case_ids = [scenario.case_id for scenario in scenarios]
	case_urls = [scenario.case_migration_url for scenario in scenarios]
	if (
		len(case_ids) != len(set(case_ids))
		or len(case_urls) != len(set(case_urls))
	):
		raise HarnessCorruption(
			"authority scenarios must have unique identities and independent databases"
		)
	return json.dumps(
		[
			{
				"admin_url": scenario.admin_url,
				"baseline_migration_url": scenario.baseline_migration_url,
				"baseline_runtime_url": scenario.baseline_runtime_url,
				"case_id": scenario.case_id,
				"case_migration_url": scenario.case_migration_url,
				"case_runtime_url": scenario.case_runtime_url,
				"expected": scenario.expected.value,
				"expected_store_error": scenario.expected_store_error.value,
				"invariant_sql": scenario.invariant_sql,
				"mutation_sql": scenario.mutation_sql,
				"post_runtime_rejected_sql": scenario.post_runtime_rejected_sql,
				"post_runtime_rejected_sqlstate": scenario.post_runtime_rejected_sqlstate,
				"postcondition_sql": scenario.postcondition_sql,
				"pre_runtime_rejected_sql": scenario.pre_runtime_rejected_sql,
				"pre_runtime_rejected_sqlstate": scenario.pre_runtime_rejected_sqlstate,
				"precondition_sql": scenario.precondition_sql,
				"restore_sql": scenario.restore_sql,
				"restore_postcondition_sql": scenario.restore_postcondition_sql,
				"runtime_effect_sql": scenario.runtime_effect_sql,
			}
			for scenario in scenarios
		],
		sort_keys=True,
		separators=(",", ":"),
	)


def register_authority_scenarios(add: object) -> None:
	"""Declare the complete named matrix; execution and classification live in Rust."""
	if not callable(add):
		raise HarnessCorruption("authority scenario registrar is not callable")
	unsafe = AuthorityClassification.UNSAFE_DATABASE_AUTHORITY
	incompatible = AuthorityClassification.DATABASE_INCOMPATIBLE
	unsafe_store = AuthorityStoreError.UNSAFE_AUTHORITY
	incompatible_store = AuthorityStoreError.INCOMPATIBLE
	migration_store = AuthorityStoreError.MIGRATION

	role_scenarios = (
		("table-owner", "ALTER TABLE decodex.accounts OWNER TO $RUNTIME_ROLE",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_class WHERE oid='decodex.accounts'::regclass AND relowner='$RUNTIME_ROLE'::regrole)"),
		("truncate", "GRANT TRUNCATE ON TABLE decodex.outbox TO $RUNTIME_ROLE",
		 "has_table_privilege('$RUNTIME_ROLE','decodex.outbox','TRUNCATE')"),
		("bypassrls", "ALTER ROLE $RUNTIME_ROLE BYPASSRLS",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='$RUNTIME_ROLE' AND rolbypassrls)"),
		("schema-create", "GRANT CREATE ON SCHEMA decodex TO $RUNTIME_ROLE",
		 "has_schema_privilege('$RUNTIME_ROLE','decodex','CREATE')"),
		("trigger-bypass", "GRANT SET ON PARAMETER session_replication_role TO $RUNTIME_ROLE",
		 "has_parameter_privilege('$RUNTIME_ROLE','session_replication_role','SET')"),
		("alter-system-bypass", "GRANT ALTER SYSTEM ON PARAMETER session_replication_role TO $RUNTIME_ROLE",
		 "has_parameter_privilege('$RUNTIME_ROLE','session_replication_role','ALTER SYSTEM')"),
		("login-default-replica", "ALTER ROLE $RUNTIME_ROLE SET session_replication_role=replica",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting WHERE setrole='$RUNTIME_ROLE'::regrole AND setdatabase=0 AND 'session_replication_role=replica'=ANY(setconfig))"),
		("function-owner-membership",
		 f"GRANT CREATE ON SCHEMA decodex TO {FUNCTION_OWNER_ROLE}; "
		 f"ALTER FUNCTION decodex.enforce_outbox_terminal_retention() OWNER TO {FUNCTION_OWNER_ROLE}; "
		 f"GRANT {FUNCTION_OWNER_ROLE} TO $RUNTIME_ROLE WITH INHERIT FALSE, SET TRUE",
		 f"pg_has_role('$RUNTIME_ROLE','{FUNCTION_OWNER_ROLE}','SET') AND "
		 f"has_schema_privilege('{FUNCTION_OWNER_ROLE}','decodex','CREATE') AND "
		 f"EXISTS (SELECT 1 FROM pg_catalog.pg_proc WHERE oid='decodex.enforce_outbox_terminal_retention()'::regprocedure AND proowner='{FUNCTION_OWNER_ROLE}'::regrole)"),
		("migration-history-write", "GRANT UPDATE ON TABLE public.refinery_schema_history TO $RUNTIME_ROLE",
		 "has_table_privilege('$RUNTIME_ROLE','public.refinery_schema_history','UPDATE')"),
		("set-role-bypass",
		 f"GRANT USAGE ON SCHEMA decodex TO {SET_BYPASS_ROLE}; "
		 f"GRANT TRUNCATE ON TABLE decodex.outbox TO {SET_BYPASS_ROLE}; "
		 f"GRANT SET ON PARAMETER session_replication_role TO {SET_BYPASS_ROLE}; "
		 f"GRANT {SET_BYPASS_ROLE} TO $RUNTIME_ROLE WITH INHERIT FALSE, SET TRUE",
		 f"pg_has_role('$RUNTIME_ROLE','{SET_BYPASS_ROLE}','SET') AND "
		 f"has_table_privilege('{SET_BYPASS_ROLE}','decodex.outbox','TRUNCATE') AND "
		 f"has_parameter_privilege('{SET_BYPASS_ROLE}','session_replication_role','SET')"),
		("migration-history-column-grant",
		 "GRANT SELECT (version) ON TABLE public.refinery_schema_history TO $RUNTIME_ROLE WITH GRANT OPTION",
		 "has_column_privilege('$RUNTIME_ROLE','public.refinery_schema_history','version','SELECT WITH GRANT OPTION')"),
		("migration-history-set-write",
		 f"GRANT UPDATE ON TABLE public.refinery_schema_history TO {SET_LEDGER_WRITE_ROLE}; "
		 f"GRANT {SET_LEDGER_WRITE_ROLE} TO $RUNTIME_ROLE WITH INHERIT FALSE, SET TRUE",
		 f"pg_has_role('$RUNTIME_ROLE','{SET_LEDGER_WRITE_ROLE}','SET') AND "
		 f"has_table_privilege('{SET_LEDGER_WRITE_ROLE}','public.refinery_schema_history','UPDATE')"),
		("sequence-update", "GRANT UPDATE ON ALL SEQUENCES IN SCHEMA decodex TO $RUNTIME_ROLE",
		 "has_sequence_privilege('$RUNTIME_ROLE','decodex.outbox_id_seq','UPDATE')"),
		("sequence-set-update",
		 f"GRANT USAGE ON SCHEMA decodex TO {SET_SEQUENCE_UPDATE_ROLE}; "
		 f"GRANT UPDATE ON ALL SEQUENCES IN SCHEMA decodex TO {SET_SEQUENCE_UPDATE_ROLE}; "
		 f"GRANT {SET_SEQUENCE_UPDATE_ROLE} TO $RUNTIME_ROLE WITH INHERIT FALSE, SET TRUE",
		 f"pg_has_role('$RUNTIME_ROLE','{SET_SEQUENCE_UPDATE_ROLE}','SET') AND "
		 f"has_schema_privilege('{SET_SEQUENCE_UPDATE_ROLE}','decodex','USAGE') AND "
		 f"has_sequence_privilege('{SET_SEQUENCE_UPDATE_ROLE}','decodex.outbox_id_seq','UPDATE')"),
		("sequence-grant-option", "GRANT USAGE ON ALL SEQUENCES IN SCHEMA decodex TO $RUNTIME_ROLE WITH GRANT OPTION",
		 "has_sequence_privilege('$RUNTIME_ROLE','decodex.outbox_id_seq','USAGE WITH GRANT OPTION')"),
		("table-grant-option", "GRANT SELECT ON TABLE decodex.accounts TO $RUNTIME_ROLE WITH GRANT OPTION",
		 "has_table_privilege('$RUNTIME_ROLE','decodex.accounts','SELECT WITH GRANT OPTION')"),
		("function-grant-option", "GRANT EXECUTE ON FUNCTION decodex.enforce_lease_operation_time() TO $RUNTIME_ROLE WITH GRANT OPTION",
		 "has_function_privilege('$RUNTIME_ROLE','decodex.enforce_lease_operation_time()','EXECUTE WITH GRANT OPTION')"),
		("collation-owner",
		 "CREATE COLLATION decodex.unsafe_owned_collation FROM pg_catalog.\"C\"; ALTER COLLATION decodex.unsafe_owned_collation OWNER TO $RUNTIME_ROLE",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_collation WHERE oid=pg_catalog.to_regcollation('decodex.unsafe_owned_collation') AND collowner='$RUNTIME_ROLE'::regrole)"),
		("conversion-owner",
		 "CREATE CONVERSION decodex.unsafe_owned_conversion FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1; ALTER CONVERSION decodex.unsafe_owned_conversion OWNER TO $RUNTIME_ROLE",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_conversion c JOIN pg_catalog.pg_namespace n ON n.oid=c.connamespace WHERE n.nspname='decodex' AND c.conname='unsafe_owned_conversion' AND c.conowner='$RUNTIME_ROLE'::regrole)"),
		("operator-owner",
		 "CREATE OPERATOR decodex.=== (FUNCTION=pg_catalog.int4eq, LEFTARG=integer, RIGHTARG=integer); ALTER OPERATOR decodex.=== (integer,integer) OWNER TO $RUNTIME_ROLE",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_operator o JOIN pg_catalog.pg_namespace n ON n.oid=o.oprnamespace WHERE n.nspname='decodex' AND o.oprname='===' AND o.oprleft='integer'::regtype AND o.oprright='integer'::regtype AND o.oprowner='$RUNTIME_ROLE'::regrole)"),
		("text-search-owner",
		 "CREATE TEXT SEARCH CONFIGURATION decodex.unsafe_owned_text_search (COPY=pg_catalog.simple); ALTER TEXT SEARCH CONFIGURATION decodex.unsafe_owned_text_search OWNER TO $RUNTIME_ROLE",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_ts_config c JOIN pg_catalog.pg_namespace n ON n.oid=c.cfgnamespace WHERE n.nspname='decodex' AND c.cfgname='unsafe_owned_text_search' AND c.cfgowner='$RUNTIME_ROLE'::regrole)"),
		("membership-admin",
		 f"GRANT {MEMBERSHIP_ADMIN_ROLE} TO $RUNTIME_ROLE WITH ADMIN TRUE, INHERIT FALSE, SET FALSE",
		 f"EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members WHERE roleid='{MEMBERSHIP_ADMIN_ROLE}'::regrole AND member='$RUNTIME_ROLE'::regrole AND admin_option AND NOT set_option)"),
		("superuser", "ALTER ROLE $RUNTIME_ROLE SUPERUSER",
		 "EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='$RUNTIME_ROLE' AND rolsuper)"),
	)
	if {case_id for case_id, _, _ in role_scenarios} != set(UNSAFE_ROLES):
		raise HarnessCorruption("unsafe role scenarios do not exactly cover the role inventory")
	for case_id, mutation, predicate in role_scenarios:
		kwargs: dict[str, object] = {}
		precondition_sql = f"SELECT NOT ({predicate})"
		postcondition_sql = f"SELECT {predicate}"
		if case_id == "login-default-replica":
			kwargs["runtime_effect_sql"] = (
				"DO $$ BEGIN IF current_setting('session_replication_role') <> 'replica' "
				"THEN RAISE EXCEPTION 'role default ineffective'; END IF; END $$"
			)
		elif case_id == "function-owner-membership":
			kwargs["runtime_effect_sql"] = (
				f"SET ROLE {FUNCTION_OWNER_ROLE}; DO $$ BEGIN IF NOT "
				"has_schema_privilege(current_user,'decodex','CREATE') THEN "
				"RAISE EXCEPTION 'owner role ineffective'; END IF; END $$"
			)
		elif case_id == "set-role-bypass":
			kwargs["runtime_effect_sql"] = (
				f"SET ROLE {SET_BYPASS_ROLE}; DO $$ BEGIN IF NOT "
				"has_table_privilege(current_user,'decodex.outbox','TRUNCATE') OR NOT "
				"has_parameter_privilege(current_user,'session_replication_role','SET') THEN "
				"RAISE EXCEPTION 'SET role authority ineffective'; END IF; END $$"
			)
		elif case_id == "migration-history-set-write":
			kwargs["runtime_effect_sql"] = (
				f"BEGIN; SET ROLE {SET_LEDGER_WRITE_ROLE}; UPDATE "
				"public.refinery_schema_history SET name='xy1364_constant_update_probe'; ROLLBACK"
			)
		elif case_id == "sequence-set-update":
			kwargs["runtime_effect_sql"] = (
				f"SET ROLE {SET_SEQUENCE_UPDATE_ROLE}; SELECT pg_catalog.setval("
				"'decodex.outbox_id_seq', 424242, false)"
			)
			precondition_sql = (
				f"SELECT NOT ({predicate}) AND (SELECT last_value=1 AND NOT is_called "
				"FROM decodex.outbox_id_seq)"
			)
			postcondition_sql = (
				f"SELECT ({predicate}) AND (SELECT last_value=424242 AND NOT is_called "
				"FROM decodex.outbox_id_seq)"
			)
			kwargs["restore_sql"] = (
				"SELECT pg_catalog.setval('decodex.outbox_id_seq', 1, false)"
			)
			kwargs["restore_postcondition_sql"] = (
				"SELECT last_value=1 AND NOT is_called FROM decodex.outbox_id_seq"
			)
		add(case_id, unsafe_store, unsafe, UNSAFE_ROLES[case_id], mutation_sql=mutation,
			precondition_sql=precondition_sql, postcondition_sql=postcondition_sql,
			**kwargs)

	add(
		"trigger-contract", unsafe_store, unsafe, RUNTIME_ROLE,
		mutation_sql=(
			"ALTER TABLE decodex.outbox DISABLE TRIGGER outbox_terminal_retention; "
			"DROP TRIGGER leases_operation_time ON decodex.leases; "
			"CREATE TRIGGER leases_operation_time BEFORE INSERT OR UPDATE ON decodex.leases "
			"FOR EACH ROW EXECUTE FUNCTION decodex.enforce_outbox_operation_time()"
		),
		precondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger WHERE "
			"tgrelid='decodex.outbox'::regclass AND tgname='outbox_terminal_retention' "
			"AND tgenabled='O') AND EXISTS (SELECT 1 "
			"FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid "
			"WHERE t.tgrelid='decodex.leases'::regclass AND t.tgname='leases_operation_time' "
			"AND p.proname='enforce_lease_operation_time' AND t.tgenabled='O')"
		),
		postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger WHERE "
			"tgrelid='decodex.outbox'::regclass AND tgname='outbox_terminal_retention' "
			"AND tgenabled='D') AND EXISTS (SELECT 1 "
			"FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid "
			"WHERE t.tgrelid='decodex.leases'::regclass AND t.tgname='leases_operation_time' "
			"AND p.proname='enforce_outbox_operation_time' AND t.tgenabled='O')"
		),
	)
	add(
		"additional-privileged-function", unsafe_store, unsafe, RUNTIME_ROLE,
		mutation_sql=(
			"CREATE FUNCTION decodex.privileged_runtime_escape() RETURNS boolean LANGUAGE plpgsql "
			"SECURITY DEFINER SET search_path=pg_catalog,decodex AS $$ BEGIN EXECUTE "
			"'ALTER TABLE decodex.outbox DISABLE TRIGGER outbox_terminal_retention'; RETURN false; END $$; "
			"GRANT EXECUTE ON FUNCTION decodex.privileged_runtime_escape() TO $RUNTIME_ROLE"
		),
		precondition_sql="SELECT pg_catalog.to_regprocedure('decodex.privileged_runtime_escape()') IS NULL",
		post_runtime_rejected_sql="ALTER TABLE decodex.outbox DISABLE TRIGGER outbox_terminal_retention",
		post_runtime_rejected_sqlstate="42501",
		runtime_effect_sql="SELECT decodex.privileged_runtime_escape()",
		postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_proc p WHERE "
			"p.oid='decodex.privileged_runtime_escape()'::regprocedure AND p.prosecdef AND "
			"p.proconfig=ARRAY['search_path=pg_catalog, decodex'] AND "
			"has_function_privilege('$RUNTIME_ROLE',p.oid,'EXECUTE')) AND EXISTS "
			"(SELECT 1 FROM pg_catalog.pg_trigger WHERE tgrelid='decodex.outbox'::regclass "
			"AND tgname='outbox_terminal_retention' AND tgenabled='D')"
		),
		restore_sql="ALTER TABLE decodex.outbox ENABLE TRIGGER outbox_terminal_retention",
		restore_postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger WHERE "
			"tgrelid='decodex.outbox'::regclass AND tgname='outbox_terminal_retention' "
			"AND tgenabled='O')"
		),
	)
	decodex_inventory = (
		"SELECT pg_catalog.jsonb_build_object("
		"'relations',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(class.relkind,namespace.nspname,class.relname,"
		"class.relowner::pg_catalog.regrole::pg_catalog.text,class.relacl) "
		"ORDER BY class.relkind,namespace.nspname,class.relname) "
		"FROM pg_catalog.pg_class class JOIN pg_catalog.pg_namespace namespace "
		"ON namespace.oid=class.relnamespace WHERE namespace.nspname='decodex'),"
		"'[]'::pg_catalog.jsonb),"
		"'column_acls',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(class.relname,attribute.attname,"
		"attribute.attacl) ORDER BY class.relname,attribute.attnum) "
		"FROM pg_catalog.pg_attribute attribute JOIN pg_catalog.pg_class class "
		"ON class.oid=attribute.attrelid JOIN pg_catalog.pg_namespace namespace "
		"ON namespace.oid=class.relnamespace WHERE namespace.nspname='decodex' "
		"AND attribute.attnum>0 AND NOT attribute.attisdropped "
		"AND attribute.attacl IS NOT NULL),'[]'::pg_catalog.jsonb),"
		"'routines',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(proc.prokind,namespace.nspname,proc.proname,"
		"pg_catalog.pg_get_function_identity_arguments(proc.oid),"
		"proc.proowner::pg_catalog.regrole::pg_catalog.text,proc.prosecdef,"
		"proc.proconfig,proc.proacl,proc.prosrc) "
		"ORDER BY proc.prokind,namespace.nspname,proc.proname,"
		"pg_catalog.pg_get_function_identity_arguments(proc.oid)) "
		"FROM pg_catalog.pg_proc proc JOIN pg_catalog.pg_namespace namespace "
		"ON namespace.oid=proc.pronamespace WHERE namespace.nspname='decodex'),"
		"'[]'::pg_catalog.jsonb),"
		"'triggers',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(class.relname,trigger.tgname,"
		"trigger.tgfoid::pg_catalog.regprocedure::pg_catalog.text,"
		"trigger.tgenabled,trigger.tgtype) "
		"ORDER BY class.relname,trigger.tgname) FROM pg_catalog.pg_trigger trigger "
		"JOIN pg_catalog.pg_class class ON class.oid=trigger.tgrelid "
		"JOIN pg_catalog.pg_namespace namespace ON namespace.oid=class.relnamespace "
		"WHERE namespace.nspname='decodex' AND NOT trigger.tgisinternal),"
		"'[]'::pg_catalog.jsonb),"
		"'rules',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(class.relname,rewrite.rulename,"
		"rewrite.ev_enabled,rewrite.is_instead,"
		"rewrite.ev_action::pg_catalog.text,rewrite.ev_qual::pg_catalog.text) "
		"ORDER BY class.relname,rewrite.rulename) FROM pg_catalog.pg_rewrite rewrite "
		"JOIN pg_catalog.pg_class class ON class.oid=rewrite.ev_class "
		"JOIN pg_catalog.pg_namespace namespace ON namespace.oid=class.relnamespace "
		"WHERE namespace.nspname='decodex' AND rewrite.rulename<>'_RETURN'),"
		"'[]'::pg_catalog.jsonb),"
		"'policies',COALESCE((SELECT pg_catalog.jsonb_agg("
		"pg_catalog.jsonb_build_array(class.relname,policy.polname,"
		"policy.polcmd,policy.polpermissive,policy.polroles,"
		"policy.polqual::pg_catalog.text,policy.polwithcheck::pg_catalog.text) "
		"ORDER BY class.relname,policy.polname) FROM pg_catalog.pg_policy policy "
		"JOIN pg_catalog.pg_class class ON class.oid=policy.polrelid "
		"JOIN pg_catalog.pg_namespace namespace ON namespace.oid=class.relnamespace "
		"WHERE namespace.nspname='decodex'),'[]'::pg_catalog.jsonb),"
		"'schema',(SELECT pg_catalog.jsonb_build_array("
		"namespace.nspowner::pg_catalog.regrole::pg_catalog.text,namespace.nspacl) "
		"FROM pg_catalog.pg_namespace namespace "
		"WHERE namespace.nspname='decodex'))::pg_catalog.text"
	)
	add(
		"public-managed-run-security-definer", unsafe_store, unsafe, RUNTIME_ROLE,
		mutation_sql=(
			f"SET ROLE {MIGRATION_ROLE}; "
			"SELECT decodex.bootstrap_role_profiles_exact("
			"'xy1417.v1','xy1417-role-profiles',"
			"'gpt-5.6-advisor','medium','priority','Bounded XY-1417 advisor fixture.',"
			"'XY-1417 fixture',"
			"'gpt-5.6-lead','medium','priority','Bounded XY-1417 lead fixture.',"
			"'XY-1417 fixture',"
			"'gpt-5.6-task','medium','priority','Bounded XY-1417 task fixture.',"
			"'XY-1417 fixture',"
			"'gpt-5.6-reviewer','medium','priority','Bounded XY-1417 reviewer fixture.',"
			"'XY-1417 fixture'); "
			"SELECT * FROM decodex.create_project("
			"'e1000000-0000-4000-8000-000000001417','xy1417/authority-fixture',"
			"'/tmp/xy1417-authority-fixture','/tmp/xy1417-authority-fixture','{}',"
			"'e2000000-0000-4000-8000-000000001417'); "
			"INSERT INTO decodex.conversations(conversation_id,title) VALUES "
			"('e3000000-0000-4000-8000-000000001417','XY-1417 authority fixture'); "
			"SELECT decodex.create_runtime_session_exact("
			"'xy1417.v1','xy1417-runtime-session',"
			"'e6000000-0000-4000-8000-000000001417',"
			"'e3000000-0000-4000-8000-000000001417','task',"
			"'e4000000-0000-4000-8000-000000001417',"
			"'e5000000-0000-4000-8000-000000001417',"
			"'XY-1417 runtime account','available',1,"
			"'e7000000-0000-4000-8000-000000001417','active'); "
			"SELECT decodex.create_work_item_exact("
			"'xy1417.v1','xy1417-work-item',"
			"'e8000000-0000-4000-8000-000000001417',"
			"'e1000000-0000-4000-8000-000000001417',"
			"'e2000000-0000-4000-8000-000000001417',NULL,"
			"ARRAY[]::pg_catalog.uuid[],ARRAY[]::pg_catalog.uuid[],"
			"ARRAY[]::pg_catalog.uuid[],'XY-1417 authority fixture',"
			"'Bounded ManagedRun authority fixture.','medium',"
			"ARRAY['Authority remains closed.'],ARRAY['Verifier rejects the path.'],"
			"'e2000000-0000-4000-8000-000000001417',"
			"'e9000000-0000-4000-8000-000000001417','XY-1417 authority fixture'); "
			"WITH operation_time AS MATERIALIZED ("
			"SELECT pg_catalog.clock_timestamp() AS value) "
			"INSERT INTO decodex.managed_runs("
			"managed_run_id,project_id,work_item_id,runtime_session_id,"
			"runtime_session_revision,lifecycle,phase,wait_reason,revision,"
			"diverged,blocked,created_at,updated_at) SELECT "
			"'ea000000-0000-4000-8000-000000001417',"
			"'e1000000-0000-4000-8000-000000001417',"
			"'e8000000-0000-4000-8000-000000001417',"
			"'e6000000-0000-4000-8000-000000001417',1,"
			"'waiting','execute','usage',1,false,true,value,value FROM operation_time; "
			"RESET ROLE; "
			"CREATE FUNCTION public.xy1417_managed_run_escape() RETURNS bigint "
			"LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,decodex "
			"AS $xy1417$ DECLARE next_revision bigint; BEGIN "
			"UPDATE decodex.managed_runs SET revision=revision+1,diverged=true,"
			"updated_at=pg_catalog.clock_timestamp() WHERE managed_run_id="
			"'ea000000-0000-4000-8000-000000001417' RETURNING revision "
			"INTO next_revision; IF next_revision IS NULL THEN RAISE EXCEPTION "
			"'managed run fixture is absent'; END IF; RETURN next_revision; END $xy1417$; "
			f"ALTER FUNCTION public.xy1417_managed_run_escape() OWNER TO {MIGRATION_ROLE}; "
			"GRANT EXECUTE ON FUNCTION public.xy1417_managed_run_escape() TO PUBLIC"
		),
		precondition_sql=(
			"SELECT pg_catalog.to_regprocedure("
			"'public.xy1417_managed_run_escape()') IS NULL AND NOT EXISTS ("
			"SELECT 1 FROM decodex.managed_runs WHERE managed_run_id="
			"'ea000000-0000-4000-8000-000000001417') AND NOT EXISTS ("
			"SELECT 1 FROM decodex.role_profiles)"
		),
		post_runtime_rejected_sql=(
			"UPDATE decodex.managed_runs SET revision=revision+1,diverged=true,"
			"updated_at=pg_catalog.clock_timestamp() WHERE managed_run_id="
			"'ea000000-0000-4000-8000-000000001417'"
		),
		post_runtime_rejected_sqlstate="42501",
		runtime_effect_sql="SELECT public.xy1417_managed_run_escape()",
		postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_proc proc "
			"JOIN pg_catalog.pg_namespace namespace ON namespace.oid=proc.pronamespace "
			"WHERE proc.oid='public.xy1417_managed_run_escape()'::pg_catalog.regprocedure "
			"AND namespace.nspname='public' AND proc.proowner="
			f"'{MIGRATION_ROLE}'::pg_catalog.regrole AND proc.prosecdef "
			"AND proc.proconfig=ARRAY['search_path=pg_catalog, decodex'] "
			"AND EXISTS (SELECT 1 FROM pg_catalog.aclexplode(COALESCE("
			"proc.proacl,pg_catalog.acldefault('f',proc.proowner))) privilege "
			"WHERE privilege.grantee=0 AND privilege.privilege_type='EXECUTE') "
			"AND pg_catalog.has_function_privilege("
			"'$RUNTIME_ROLE',proc.oid,'EXECUTE')) AND EXISTS (SELECT 1 "
			"FROM decodex.managed_runs WHERE managed_run_id="
			"'ea000000-0000-4000-8000-000000001417' AND lifecycle='waiting' "
			"AND phase='execute' AND wait_reason='usage' AND revision=2 "
			"AND diverged AND blocked)"
		),
		invariant_sql=decodex_inventory,
	)
	add(
		"indirect-trigger-owner-effect", unsafe_store, unsafe, RUNTIME_ROLE,
		mutation_sql=(
			"CREATE FUNCTION public.indirect_owner_escape() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER "
			"SET search_path=pg_catalog,decodex AS $$ BEGIN EXECUTE 'ALTER TABLE decodex.outbox "
			"DISABLE TRIGGER outbox_terminal_retention'; RETURN NULL; END $$; "
			"REVOKE ALL ON FUNCTION public.indirect_owner_escape() FROM PUBLIC,$RUNTIME_ROLE; "
			"CREATE TRIGGER accounts_indirect_owner_escape AFTER INSERT ON decodex.accounts "
			"FOR EACH STATEMENT EXECUTE FUNCTION public.indirect_owner_escape()"
		),
		precondition_sql=(
			"SELECT pg_catalog.to_regprocedure('public.indirect_owner_escape()') IS NULL AND NOT EXISTS "
			"(SELECT 1 FROM pg_catalog.pg_trigger WHERE tgrelid='decodex.accounts'::regclass "
			"AND tgname='accounts_indirect_owner_escape')"
		),
		runtime_effect_sql=(
			"INSERT INTO decodex.accounts(account_id,display_label) VALUES "
			"('91000000-0000-4000-8000-000000000001','indirect owner escape')"
		),
		postcondition_sql=(
			"SELECT pg_catalog.to_regprocedure('public.indirect_owner_escape()') IS NOT NULL AND "
			"NOT has_function_privilege('$RUNTIME_ROLE','public.indirect_owner_escape()','EXECUTE') AND "
			"EXISTS (SELECT 1 FROM pg_catalog.pg_trigger WHERE tgrelid='decodex.accounts'::regclass "
			"AND tgname='accounts_indirect_owner_escape') AND EXISTS (SELECT 1 FROM "
			"pg_catalog.pg_trigger WHERE tgrelid='decodex.outbox'::regclass "
			"AND tgname='outbox_terminal_retention' AND tgenabled='D')"
		),
		restore_sql="ALTER TABLE decodex.outbox ENABLE TRIGGER outbox_terminal_retention",
		restore_postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger WHERE "
			"tgrelid='decodex.outbox'::regclass AND tgname='outbox_terminal_retention' "
			"AND tgenabled='O')"
		),
	)
	add(
		"extension-member-control", unsafe_store, unsafe, RUNTIME_ROLE,
		mutation_sql=(
			"GRANT CREATE ON DATABASE $CASE_DATABASE TO $RUNTIME_ROLE; "
			"GRANT CREATE ON SCHEMA public,decodex TO $RUNTIME_ROLE; SET ROLE $RUNTIME_ROLE; "
			"CREATE EXTENSION hstore WITH SCHEMA public; CREATE COLLATION "
			"decodex.extension_control_member FROM pg_catalog.\"C\"; ALTER EXTENSION hstore "
			"ADD COLLATION decodex.extension_control_member; RESET ROLE; ALTER COLLATION "
			f"decodex.extension_control_member OWNER TO {MIGRATION_ROLE}; "
			"REVOKE CREATE ON SCHEMA public,decodex FROM $RUNTIME_ROLE; "
			"REVOKE CREATE ON DATABASE $CASE_DATABASE FROM $RUNTIME_ROLE"
		),
		precondition_sql=(
			"SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname='hstore') AND "
			"pg_catalog.to_regcollation('decodex.extension_control_member') IS NULL"
		),
		runtime_effect_sql="BEGIN; DROP EXTENSION hstore; ROLLBACK",
		postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_depend d "
			"ON d.refclassid='pg_catalog.pg_extension'::regclass AND d.refobjid=e.oid JOIN "
			"pg_catalog.pg_collation c ON d.classid='pg_catalog.pg_collation'::regclass AND "
			"d.objid=c.oid WHERE e.extname='hstore' AND e.extowner='$RUNTIME_ROLE'::regrole AND "
			"c.oid='decodex.extension_control_member'::regcollation AND "
			f"c.collowner='{MIGRATION_ROLE}'::regrole AND d.deptype='e')"
		),
	)

	add(
		"missing-ledger-select", incompatible_store, incompatible, MISSING_SELECT_ROLE,
		mutation_sql="REVOKE SELECT ON TABLE public.refinery_schema_history FROM $RUNTIME_ROLE",
		precondition_sql="SELECT has_table_privilege('$RUNTIME_ROLE','public.refinery_schema_history','SELECT')",
		postcondition_sql="SELECT NOT has_table_privilege('$RUNTIME_ROLE','public.refinery_schema_history','SELECT')",
	)
	function_fingerprint = (
		"SELECT pg_catalog.json_build_object('security_definer',p.prosecdef,'leakproof',p.proleakproof,"
		"'volatility',p.provolatile,'parallel',p.proparallel,'kind',p.prokind,'strict',p.proisstrict,"
		"'returns_set',p.proretset,'return_type',p.prorettype::regtype::text,'language',l.lanname,"
		"'cost',p.procost,'rows',p.prorows,'binary',p.probin,'sql_body',p.prosqlbody,"
		"'support',p.prosupport,'variadic',p.provariadic,'transform_types',p.protrftypes,"
		"'argument_defaults_count',p.pronargdefaults,'argument_defaults',p.proargdefaults,"
		"'config',p.proconfig,'acl',p.proacl,'owner',r.rolname,'arguments',"
		"pg_catalog.pg_get_function_arguments(p.oid),'result',pg_catalog.pg_get_function_result(p.oid))::text "
		"FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_language l ON l.oid=p.prolang JOIN "
		"pg_catalog.pg_roles r ON r.oid=p.proowner WHERE "
		"p.oid='decodex.enforce_outbox_terminal_retention()'::regprocedure"
	)
	add(
		"function-contract", incompatible_store, incompatible, RUNTIME_ROLE,
		mutation_sql=(
			"CREATE OR REPLACE FUNCTION decodex.enforce_outbox_terminal_retention() RETURNS trigger "
			"LANGUAGE plpgsql SET search_path=pg_catalog,decodex AS $$ BEGIN RETURN NEW; END $$"
		),
		precondition_sql=(
			"SELECT prosrc LIKE '%retention pruning%' AND proconfig=ARRAY['search_path=pg_catalog, decodex'] "
			"FROM pg_catalog.pg_proc WHERE oid='decodex.enforce_outbox_terminal_retention()'::regprocedure"
		),
		postcondition_sql=(
			"SELECT prosrc LIKE '%RETURN NEW%' AND prosrc NOT LIKE '%retention pruning%' AND "
			"proconfig=ARRAY['search_path=pg_catalog, decodex'] FROM pg_catalog.pg_proc WHERE "
			"oid='decodex.enforce_outbox_terminal_retention()'::regprocedure"
		),
		invariant_sql=function_fingerprint,
	)
	credential_insert = (
		"INSERT INTO decodex.accounts(account_id,display_label) VALUES "
		"('92000000-0000-4000-8000-000000000001','token=fixture-secret')"
	)
	add(
		"credential-constraint", incompatible_store, incompatible, RUNTIME_ROLE,
		mutation_sql="ALTER TABLE decodex.accounts DROP CONSTRAINT accounts_no_credentials",
		precondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint WHERE conrelid="
			"'decodex.accounts'::regclass AND conname='accounts_no_credentials') AND NOT EXISTS "
			"(SELECT 1 FROM decodex.accounts WHERE account_id='92000000-0000-4000-8000-000000000001')"
		),
		pre_runtime_rejected_sql=credential_insert,
		pre_runtime_rejected_sqlstate="23514",
		runtime_effect_sql=credential_insert,
		postcondition_sql=(
			"SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint WHERE conrelid="
			"'decodex.accounts'::regclass AND conname='accounts_no_credentials') AND EXISTS "
			"(SELECT 1 FROM decodex.accounts WHERE account_id='92000000-0000-4000-8000-000000000001')"
		),
	)
	add(
		"external-cascade", incompatible_store, incompatible, RUNTIME_ROLE,
		mutation_sql=(
			"CREATE TABLE public.external_outbox_child(child_id bigint PRIMARY KEY,outbox_id bigint NOT NULL, "
			"CONSTRAINT external_outbox_child_outbox_fk FOREIGN KEY(outbox_id) "
			"REFERENCES decodex.outbox(id) ON DELETE CASCADE); REVOKE ALL ON TABLE "
			"public.external_outbox_child FROM PUBLIC; INSERT INTO decodex.outbox(id,effect_key,"
			"aggregate_kind,aggregate_id,aggregate_revision,payload,state,effect_state,receipt,"
			"reconciliation,created_at,delivered_at,retain_until) OVERRIDING SYSTEM VALUE "
			"WITH anchor AS MATERIALIZED (SELECT pg_catalog.date_trunc('milliseconds', "
			"pg_catalog.statement_timestamp()) AS at) SELECT 920001,'external-cascade','account',"
			"'fixture',1,'{}','delivered','receipt_recorded','{\"ok\":true}','{\"ok\":true}',"
			"anchor.at-interval '2 days',anchor.at-interval '1 day',anchor.at-interval '1 second' "
			"FROM anchor; "
			"INSERT INTO public.external_outbox_child VALUES (1,920001)"
		),
		precondition_sql=(
			"SELECT pg_catalog.to_regclass('public.external_outbox_child') IS NULL AND NOT EXISTS "
			"(SELECT 1 FROM decodex.outbox WHERE id=920001)"
		),
		post_runtime_rejected_sql="DELETE FROM public.external_outbox_child WHERE child_id=1",
		post_runtime_rejected_sqlstate="42501",
		runtime_effect_sql="DELETE FROM decodex.outbox WHERE id=920001",
		postcondition_sql=(
			"SELECT pg_catalog.to_regclass('public.external_outbox_child') IS NOT NULL AND EXISTS "
			"(SELECT 1 FROM pg_catalog.pg_constraint c JOIN pg_catalog.pg_class child ON "
			"child.oid=c.conrelid JOIN pg_catalog.pg_namespace child_namespace ON "
			"child_namespace.oid=child.relnamespace JOIN pg_catalog.pg_class parent ON "
			"parent.oid=c.confrelid JOIN pg_catalog.pg_namespace parent_namespace ON "
			"parent_namespace.oid=parent.relnamespace WHERE "
			"c.conname='external_outbox_child_outbox_fk' AND c.contype='f' AND c.confdeltype='c' "
			"AND child_namespace.nspname='public' AND child.relname='external_outbox_child' "
			"AND parent_namespace.nspname='decodex' AND parent.relname='outbox' AND c.conkey="
			"ARRAY[(SELECT attribute.attnum FROM pg_catalog.pg_attribute attribute WHERE "
			"attribute.attrelid=child.oid AND attribute.attname='outbox_id' AND NOT "
			"attribute.attisdropped)]::pg_catalog.int2[] AND c.confkey=ARRAY[(SELECT "
			"attribute.attnum FROM pg_catalog.pg_attribute attribute WHERE "
			"attribute.attrelid=parent.oid AND attribute.attname='id' AND NOT "
			"attribute.attisdropped)]::pg_catalog.int2[]) AND NOT "
			"pg_catalog.has_table_privilege('$RUNTIME_ROLE','public.external_outbox_child','DELETE') "
			"AND pg_catalog.has_table_privilege('$RUNTIME_ROLE','decodex.outbox','DELETE') AND NOT EXISTS "
			"(SELECT 1 FROM decodex.outbox WHERE id=920001) AND NOT EXISTS "
			"(SELECT 1 FROM public.external_outbox_child WHERE child_id=1)"
		),
	)
	add(
		"ledger-tamper", migration_store, incompatible, RUNTIME_ROLE,
		mutation_sql="UPDATE public.refinery_schema_history SET name=name||'_tampered' WHERE version=1",
		precondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM public.refinery_schema_history WHERE version=1 "
			"AND name NOT LIKE '%_tampered')"
		),
		postcondition_sql=(
			"SELECT EXISTS (SELECT 1 FROM public.refinery_schema_history WHERE version=1 "
			"AND name LIKE '%_tampered')"
		),
	)
	add(
		"missing-pgcrypto", incompatible_store, incompatible, RUNTIME_ROLE,
		mutation_sql="DROP EXTENSION pgcrypto CASCADE",
		precondition_sql="SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname='pgcrypto')",
		postcondition_sql="SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname='pgcrypto')",
	)


class ClusterStatus(Enum):
	"""Tri-state result from pg_ctl status."""

	RUNNING = "running"
	STOPPED = "stopped"
	UNKNOWN = "unknown"


class LiveDoctorProbeProgress(Enum):
	"""Observable progress of the live-doctor side of one mutation attempt."""

	NOT_STARTED = "not_started"
	STARTED = "started"
	READINESS_REACHED = "readiness_reached"


class MutationSqlDispatch(Enum):
	"""Conservative delivery facts owned only by the mutation process owner."""

	NOT_DISPATCHED = "not_dispatched"
	DELIVERY_POSSIBLE = "delivery_possible"
	MAY_HAVE_DISPATCHED = "may_have_dispatched"


class MutationCommandCompletion(Enum):
	"""Observable command acknowledgement, independent of mutation postconditions."""

	PENDING = "pending"
	COMMAND_ACKNOWLEDGED = "command_acknowledged"


class RestorationClaim(Enum):
	"""The single scheduler claim derived from conservative dispatch evidence."""

	BLOCKED_BEFORE_DISPATCH = "blocked_before_dispatch"
	ELIGIBLE_AFTER_DISPATCH = "eligible_after_dispatch"


@dataclass
class LiveDoctorMutationAttempt:
	"""Keep orthogonal live-doctor, SQL-delivery, and completion facts."""

	probe: LiveDoctorProbeProgress = LiveDoctorProbeProgress.NOT_STARTED
	dispatch: MutationSqlDispatch = MutationSqlDispatch.NOT_DISPATCHED
	completion: MutationCommandCompletion = MutationCommandCompletion.PENDING
	_restoration_claim_consumed: bool = False

	def mark_probe_started(self) -> None:
		if self.probe is not LiveDoctorProbeProgress.NOT_STARTED:
			raise HarnessCorruption("live doctor probe was started more than once")
		self.probe = LiveDoctorProbeProgress.STARTED

	def mark_readiness_reached(self) -> None:
		if self.probe is not LiveDoctorProbeProgress.STARTED:
			raise HarnessCorruption("live doctor readiness was recorded out of order")
		self.probe = LiveDoctorProbeProgress.READINESS_REACHED

	def _mark_delivery_possible(self) -> None:
		if self.dispatch is not MutationSqlDispatch.NOT_DISPATCHED:
			raise HarnessCorruption("ordinary mutation SQL dispatch was recorded more than once")
		self.dispatch = MutationSqlDispatch.DELIVERY_POSSIBLE

	def _mark_may_have_dispatched(self) -> None:
		if self.dispatch is not MutationSqlDispatch.NOT_DISPATCHED:
			raise HarnessCorruption("secret mutation SQL dispatch was recorded more than once")
		self.dispatch = MutationSqlDispatch.MAY_HAVE_DISPATCHED

	def _mark_command_acknowledged(self) -> None:
		if (
			self.dispatch is MutationSqlDispatch.NOT_DISPATCHED
			or self.completion is not MutationCommandCompletion.PENDING
		):
			raise HarnessCorruption("mutation command acknowledgement is inconsistent")
		self.completion = MutationCommandCompletion.COMMAND_ACKNOWLEDGED

	def consume_restoration_claim(self) -> RestorationClaim:
		if self._restoration_claim_consumed:
			raise HarnessCorruption("mutation restoration claim was consumed more than once")
		self._restoration_claim_consumed = True
		if self.dispatch is MutationSqlDispatch.NOT_DISPATCHED:
			return RestorationClaim.BLOCKED_BEFORE_DISPATCH
		return RestorationClaim.ELIGIBLE_AFTER_DISPATCH


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


def private_command_output(
	command: list[str],
	env: dict[str, str],
	*,
	byte_limit: int,
	failure_message: str,
) -> bytes:
	"""Read bounded private output and never copy command or output into an error."""
	process: subprocess.Popen[bytes] | None = None
	stdout = None
	try:
		if byte_limit <= 0:
			raise TestFailure(failure_message)
		process = subprocess.Popen(
			command,
			stdin=subprocess.DEVNULL,
			stdout=subprocess.PIPE,
			stderr=subprocess.DEVNULL,
			env=env,
			cwd=REPO_ROOT,
		)
		stdout = process.stdout
		if stdout is None:
			raise TestFailure(failure_message)
		descriptor = stdout.fileno()
		os.set_blocking(descriptor, False)
		deadline = time.monotonic() + POSTGRES_PRIVATE_COMMAND_TIMEOUT_SECONDS
		payload = bytearray()
		while True:
			remaining = deadline - time.monotonic()
			if remaining <= 0:
				raise TestFailure(failure_message)
			readable, _, _ = select.select([descriptor], [], [], remaining)
			if not readable:
				raise TestFailure(failure_message)
			chunk = os.read(descriptor, min(64 * 1024, byte_limit + 1 - len(payload)))
			if not chunk:
				break
			payload.extend(chunk)
			if len(payload) > byte_limit:
				raise TestFailure(failure_message)
		remaining = deadline - time.monotonic()
		if remaining <= 0 or process.wait(timeout=remaining) != 0:
			raise TestFailure(failure_message)
		return bytes(payload)
	except TestFailure:
		raise
	except (OSError, ValueError, subprocess.SubprocessError):
		raise TestFailure(failure_message) from None
	finally:
		if process is not None:
			if process.poll() is None:
				process.kill()
			try:
				process.wait()
			except BaseException:
				pass
		if stdout is not None:
			stdout.close()


def postgres_tool_version(
	path: Path, name: str, env: dict[str, str]
) -> bytes:
	failure_message = "PostgreSQL toolchain authority is unavailable"
	version = private_command_output(
		[str(path), "--version"],
		env,
		byte_limit=POSTGRES_TOOL_VERSION_MAX_BYTES,
		failure_message=failure_message,
	)
	pattern = re.compile(
		(
			rf"{re.escape(name)} \(PostgreSQL\) "
			r"18(?:\.[0-9]+)*(?: [ -~]+)?\n?"
		).encode("ascii")
	)
	if pattern.fullmatch(version) is None:
		raise TestFailure(failure_message)
	return version


def postgres_toolchain_fingerprint(
	tools: dict[str, Path], env: dict[str, str]
) -> str:
	failure_message = "PostgreSQL toolchain authority is unavailable"
	try:
		if set(tools) != set(POSTGRES_TOOL_NAMES):
			raise TestFailure(failure_message)
		paths = [tools[name] for name in POSTGRES_TOOL_NAMES]
		if (
			len(paths) != len(POSTGRES_TOOL_NAMES)
			or any(not path.is_absolute() or path.resolve(strict=True) != path for path in paths)
			or len({path.parent for path in paths}) != 1
		):
			raise TestFailure(failure_message)
		fingerprint = hashlib.sha256(
			b"decodex/postgres-toolchain-authority/1\0"
		)
		for name, path in zip(POSTGRES_TOOL_NAMES, paths):
			before = path.stat()
			if not stat.S_ISREG(before.st_mode) or before.st_mode & 0o111 == 0:
				raise TestFailure(failure_message)
			binary_digest = hashlib.sha256()
			with path.open("rb") as binary:
				while chunk := binary.read(64 * 1024):
					binary_digest.update(chunk)
			after = path.stat()
			stable_fields = (
				"st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_size",
				"st_mtime_ns", "st_ctime_ns",
			)
			if any(
				getattr(before, field_name) != getattr(after, field_name)
				for field_name in stable_fields
			):
				raise TestFailure(failure_message)
			version = postgres_tool_version(path, name, env)
			for value in (
				name.encode("ascii"),
				binary_digest.digest(),
				hashlib.sha256(version).digest(),
			):
				fingerprint.update(len(value).to_bytes(4, "big"))
				fingerprint.update(value)
		return fingerprint.hexdigest()
	except TestFailure:
		raise
	except (OSError, ValueError):
		raise TestFailure(failure_message) from None


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


def staged_source_binding() -> dict[str, str]:
	"""Bind a complete candidate to its base commit and exact staged tree."""
	status = git_read_text(
		"status", "--porcelain=v1", "--untracked-files=all",
		byte_limit=GIT_STATUS_MAX_BYTES,
	).splitlines()
	if not status:
		raise TestFailure("retained-title boundary requires a non-empty staged candidate")
	if any(len(entry) < 3 or entry[0] in {" ", "?"} or entry[1] != " " for entry in status):
		raise TestFailure(
			"retained-title boundary requires all candidate changes in the index"
		)
	binding = {
		"head": git_read_text(
			"rev-parse", "--verify", "HEAD", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
		"tree": git_read_text(
			"write-tree", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
	}
	if any(re.fullmatch(r"[0-9a-f]{40}", value) is None for value in binding.values()):
		raise TestFailure("retained-title staged source binding is invalid")
	if git_read_text(
		"cat-file", "-t", binding["tree"], byte_limit=GIT_METADATA_MAX_BYTES
	).strip() != "tree":
		raise TestFailure("retained-title staged source tree is invalid")
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


def _reap_live_doctor_child(
	process: subprocess.Popen[str] | subprocess.Popen[bytes], child: str,
) -> HarnessCorruption | None:
	"""Bound termination and reap attempts without abandoning fallback cleanup."""
	diagnostics: list[str] = []
	reaped = False

	def record(operation: str, error: Exception) -> None:
		diagnostics.append(f"{operation}: {type(error).__name__}")

	try:
		returncode = process.poll()
	except Exception as error:
		record("poll", error)
		returncode = None
	if returncode is None:
		try:
			process.terminate()
		except Exception as error:
			record("terminate", error)
		try:
			process.communicate(timeout=10)
			reaped = process.returncode is not None
		except Exception as error:
			record("communicate after terminate", error)
	else:
		try:
			process.wait(timeout=10)
			reaped = process.returncode is not None
		except Exception as error:
			record("wait after exit", error)
	if not reaped:
		try:
			process.kill()
		except Exception as error:
			record("kill", error)
		try:
			process.communicate(timeout=10)
			reaped = process.returncode is not None
		except Exception as error:
			record("communicate after kill", error)
	if not reaped:
		try:
			process.wait(timeout=10)
			reaped = process.returncode is not None
		except Exception as error:
			record("final wait", error)
	if reaped:
		return None
	detail = ", ".join(diagnostics) or "process state remained indeterminate"
	return HarnessCorruption(f"{child} child was not reaped ({detail})")


def _raise_child_cleanup_corruption(cleanup: HarnessCorruption | None) -> None:
	if cleanup is None:
		return
	active_error = sys.exc_info()[1]
	if isinstance(active_error, StageActionFailure):
		raise StageActionFailure(
			active_error.primary,
			active_error.secondary + (cleanup,),
		) from active_error.primary
	if isinstance(active_error, Exception):
		raise StageActionFailure(active_error, (cleanup,)) from active_error
	raise cleanup


def _write_pipe_bytes(descriptor: int, payload: bytes) -> None:
	view = memoryview(payload)
	while view:
		written = os.write(descriptor, view)
		if written <= 0:
			raise OSError("pipe write made no progress")
		view = view[written:]


def _read_bounded_secret_logging_frames(
	descriptor: int, *, deadline: float
) -> tuple[str, ...]:
	"""Read the exact newline-framed logging handshake without text buffering."""
	buffer = bytearray()
	frames: list[str] = []
	total_bytes = 0
	while len(frames) < len(SECRET_LOGGING_EXPECTED_FRAMES):
		terminator = buffer.find(b"\n")
		if terminator >= 0:
			if terminator == 0 or terminator > SECRET_LOGGING_FRAME_MAX_BYTES:
				raise TestFailure(
					"secret-bearing PostgreSQL logging readiness framing is invalid"
				)
			frame = bytes(buffer[:terminator])
			del buffer[:terminator + 1]
			if any(byte < 0x20 or byte > 0x7e for byte in frame):
				raise TestFailure(
					"secret-bearing PostgreSQL logging readiness framing is invalid"
				)
			frames.append(frame.decode("ascii"))
			if len(frames) == len(SECRET_LOGGING_EXPECTED_FRAMES):
				if buffer:
					raise TestFailure(
						"secret-bearing PostgreSQL logging readiness framing is invalid"
					)
				return tuple(frames)
			continue
		if len(buffer) > SECRET_LOGGING_FRAME_MAX_BYTES:
			raise TestFailure(
				"secret-bearing PostgreSQL logging readiness exceeded framing bounds"
			)
		remaining = deadline - time.monotonic()
		if remaining <= 0:
			raise TestFailure("secret-bearing PostgreSQL logging readiness timed out")
		try:
			readable, _, _ = select.select([descriptor], [], [], remaining)
		except (OSError, ValueError):
			raise TestFailure(
				"secret-bearing PostgreSQL logging readiness could not be read"
			) from None
		if not readable:
			raise TestFailure("secret-bearing PostgreSQL logging readiness timed out")
		try:
			chunk = os.read(
				descriptor,
				min(4096, SECRET_LOGGING_HANDSHAKE_MAX_BYTES + 1 - total_bytes),
			)
		except OSError:
			raise TestFailure(
				"secret-bearing PostgreSQL logging readiness could not be read"
			) from None
		if not chunk:
			raise TestFailure(
				"secret-bearing PostgreSQL logging readiness ended before completion"
			)
		total_bytes += len(chunk)
		if total_bytes > SECRET_LOGGING_HANDSHAKE_MAX_BYTES:
			raise TestFailure(
				"secret-bearing PostgreSQL logging readiness exceeded framing bounds"
			)
		buffer.extend(chunk)
	raise HarnessCorruption("secret logging frame reader exited without a result")


def _establish_secret_logging_guard(
	stdin_descriptor: int, stdout_descriptor: int
) -> None:
	try:
		_write_pipe_bytes(stdin_descriptor, SECRET_LOGGING_PRELUDE)
	except OSError:
		raise TestFailure(
			"secret-bearing PostgreSQL logging prelude could not be written"
		) from None
	frames = _read_bounded_secret_logging_frames(
		stdout_descriptor,
		deadline=time.monotonic() + SECRET_LOGGING_READY_TIMEOUT_SECONDS,
	)
	if frames != SECRET_LOGGING_EXPECTED_FRAMES:
		raise TestFailure("secret-bearing PostgreSQL logging is not fail-closed")


class _LiveDoctorMutationSqlExecutor:
	"""Own every mutation child and the conservative SQL-delivery facts it exposes."""

	def __init__(self, attempt: LiveDoctorMutationAttempt) -> None:
		self._attempt = attempt

	def execute(
		self,
		database: str,
		sql: str,
		env: dict[str, str],
		*,
		role: str | None,
		secret_sql: bool,
	) -> None:
		if self._attempt.probe is not LiveDoctorProbeProgress.READINESS_REACHED:
			raise HarnessCorruption("mutation SQL executor ran before live-doctor readiness")
		if secret_sql:
			if role is not None:
				raise HarnessCorruption("secret live-doctor mutation cannot select a role")
			self._execute_secret(database, sql, env)
		else:
			self._execute_ordinary(database, sql, env, role=role)

	def _execute_ordinary(
		self,
		database: str,
		sql: str,
		env: dict[str, str],
		*,
		role: str | None,
	) -> None:
		mutation_env = env if role is None else env.copy()
		if role is not None:
			mutation_env["PGUSER"] = role
		try:
			process = subprocess.Popen(
				[
					"psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database,
					"-c", sql,
				],
				text=True,
				stdout=subprocess.PIPE,
				stderr=subprocess.PIPE,
				env=mutation_env,
				cwd=REPO_ROOT,
			)
		except Exception as error:
			raise TestFailure("live-doctor mutation SQL process did not start") from error
		try:
			# A returned Popen owns the payload in argv; delivery is now possible.
			self._attempt._mark_delivery_possible()
			try:
				stdout, stderr = process.communicate(timeout=10)
			except subprocess.TimeoutExpired as error:
				raise TestFailure("live-doctor mutation SQL command timed out") from error
			if process.returncode != 0:
				raise TestFailure(
					f"live-doctor mutation SQL command failed ({process.returncode})\n"
					f"stdout:\n{stdout}\nstderr:\n{stderr}"
				)
			self._attempt._mark_command_acknowledged()
		finally:
			_raise_child_cleanup_corruption(
				_reap_live_doctor_child(process, "live-doctor mutation SQL")
			)

	def _execute_secret(
		self, database: str, sql: str, env: dict[str, str]
	) -> None:
		try:
			process = subprocess.Popen(
				["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database],
				bufsize=0,
				stdin=subprocess.PIPE,
				stdout=subprocess.PIPE,
				stderr=subprocess.PIPE,
				env=env,
				cwd=REPO_ROOT,
			)
		except Exception as error:
			raise TestFailure(
				"secret live-doctor mutation process failed before payload dispatch"
			) from error
		try:
			if process.stdin is None or process.stdout is None or process.stderr is None:
				raise TestFailure("secret live-doctor mutation pipes are unavailable")
			try:
				stdin_descriptor = process.stdin.fileno()
				_establish_secret_logging_guard(
					stdin_descriptor, process.stdout.fileno()
				)
				_write_pipe_bytes(stdin_descriptor, b"\\set VERBOSITY terse\n")
				payload = sql
				if not sql.rstrip().endswith(";"):
					payload += ";"
				payload += f"\n\\echo {SECRET_SQL_DONE_MARKER}\n\\quit\n"
				payload_bytes = payload.encode("utf-8")
				# A write can fail after accepting a payload prefix.
				self._attempt._mark_may_have_dispatched()
				_write_pipe_bytes(stdin_descriptor, payload_bytes)
				stdout, stderr = process.communicate(timeout=10)
				if (
					process.returncode != 0
					or SECRET_SQL_DONE_MARKER.encode("ascii") not in stdout.splitlines()
					or stderr
				):
					raise TestFailure("secret live-doctor mutation command failed")
				self._attempt._mark_command_acknowledged()
			except subprocess.TimeoutExpired as error:
				raise TestFailure("secret live-doctor mutation command timed out") from error
			except HarnessCorruption:
				raise
			except TestFailure:
				raise
			except Exception as error:
				if self._attempt.dispatch is MutationSqlDispatch.NOT_DISPATCHED:
					raise TestFailure(
						"secret live-doctor mutation failed before payload dispatch"
					) from error
				raise TestFailure(
					"secret live-doctor mutation failed after payload dispatch"
				) from error
		finally:
			_raise_child_cleanup_corruption(
				_reap_live_doctor_child(process, "secret live-doctor mutation SQL")
			)


def run_live_doctor_mutation(
	root: Path,
	database: str,
	sql: str,
	case: str,
	work: Path,
	env: dict[str, str],
	attempt: LiveDoctorMutationAttempt,
	*,
	unsafe_authority: bool = False,
	cluster_authority: bool = False,
	secret_sql: bool = False,
	mutation_probe: str | None = None,
) -> str:
	"""Coordinate a real daemon query around an adapter-owned database mutation."""
	attempt.mark_probe_started()
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
	try:
		deadline = time.monotonic() + 20
		while not (sync / "ready").exists():
			if process.poll() is not None:
				stdout, stderr = process.communicate(timeout=10)
				if secret_sql:
					raise TestFailure("live doctor exited before secret-bearing mutation")
				raise TestFailure(
					f"live doctor exited before {case} mutation\nstdout:\n{stdout}\nstderr:\n{stderr}"
				)
			if time.monotonic() >= deadline:
				if secret_sql:
					raise TestFailure(
						"live doctor did not reach the secret-bearing mutation barrier"
					)
				raise TestFailure(f"live doctor did not reach {case} barrier")
			time.sleep(0.01)

		attempt.mark_readiness_reached()
		_LiveDoctorMutationSqlExecutor(attempt).execute(
			database,
			sql,
			env,
			role=None if cluster_authority else MIGRATION_ROLE,
			secret_sql=secret_sql,
		)
		if mutation_probe is not None and psql(database, mutation_probe, env) != "t":
			raise TestFailure(f"{case} authority mutation probe is vacuous")
		(sync / "mutated").write_text("mutated", encoding="utf-8")
		try:
			stdout, stderr = process.communicate(timeout=30)
		except subprocess.TimeoutExpired as error:
			if secret_sql:
				raise TestFailure(
					"live doctor did not finish the secret-bearing drift check"
				) from error
			raise TestFailure(f"live doctor did not finish {case}") from error
		if process.returncode != 0:
			if secret_sql:
				raise TestFailure("live doctor failed after the secret-bearing mutation")
			raise TestFailure(
				f"live doctor failed after {case} mutation\nstdout:\n{stdout}\nstderr:\n{stderr}"
			)
		return stdout.strip() or stderr.strip()
	finally:
		_raise_child_cleanup_corruption(
			_reap_live_doctor_child(process, "live doctor")
		)


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


def cleanup_restore_prerequisite_gate(
	state: RestorePrerequisiteGateState,
	work: Path | None,
	data_dir: Path | None,
	env: dict[str, str] | None,
	cluster_start_attempted: bool,
	*,
	fault_injector: Callable[[str], None] | None = None,
) -> None:
	"""Run the v2 gate cleanup owners without exposing operational details."""
	try:
		state.begin_cleanup(cluster_start_attempted, work is not None)
		def inject(point: str) -> None:
			if fault_injector is not None:
				if point not in RESTORE_PREREQUISITE_CLEANUP_FAULT_POINTS:
					raise HarnessCorruption(
						"restore prerequisite cleanup fault point is invalid"
					)
				fault_injector(point)
		def stop_cluster() -> None:
			if data_dir is None or env is None:
				raise HarnessCorruption("restore prerequisite teardown state is invalid")
			status = (
				postgres_status(data_dir, env)
				if data_dir.exists() else ClusterStatus.STOPPED
			)
			if status is ClusterStatus.RUNNING:
				try:
					run(
						["pg_ctl", "-D", str(data_dir), "-m", "fast", "-w", "stop"],
						env,
					)
				except Exception:
					pass
				status = postgres_status(data_dir, env)
			if status is ClusterStatus.RUNNING:
				try:
					run(
						[
							"pg_ctl", "-D", str(data_dir), "-m", "immediate",
							"-w", "stop",
						],
						env,
					)
				except Exception:
					pass
				status = postgres_status(data_dir, env)
			if status is not ClusterStatus.STOPPED:
				raise TestFailure("restore prerequisite cluster cleanup failed")
		def remove_private_work() -> None:
			if work is None:
				raise HarnessCorruption(
					"restore prerequisite private work is absent"
				)
			shutil.rmtree(work)
		actions: dict[str, Callable[[], object]] = {
			"cluster_stop": stop_cluster,
			"private_work_cleanup": remove_private_work,
		}
		if state.required_cleanup_owners:
			inject("before_first_cleanup_owner")
		for owner in state.required_cleanup_owners:
			state.run_cleanup(owner, actions[owner])
			inject(f"after_{owner}_action_before_transition")
			state.complete_cleanup_owner(owner)
			if owner == "cluster_stop":
				inject("between_cluster_stop_and_private_work_cleanup")
		state.begin_cleanup_finalization()
		inject("during_cleanup_finalization")
		state.finish_cleanup()
	except BaseException as error:
		try:
			state.capture_cleanup_failure(error)
		except BaseException:
			state._repair_cleanup_for_failure_document()


def database_url(socket_dir: Path, port: int, database: str, role: str) -> str:
	return f"postgresql://{role}@/{database}?host={socket_dir.as_posix()}&port={port}"


def psql(database: str, sql: str, env: dict[str, str]) -> str:
	return run(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database, "-c", sql],
		env,
	)


def capture_postgres_version(env: dict[str, str]) -> dict[str, object]:
	try:
		version = json.loads(psql(
			"postgres",
			"SELECT pg_catalog.json_build_object("
			"'version',pg_catalog.current_setting('server_version'),"
			"'version_num',pg_catalog.current_setting('server_version_num')::integer,"
			"'major',pg_catalog.current_setting('server_version_num')::integer/10000)::text",
			env,
		))
	except (TypeError, json.JSONDecodeError):
		raise TestFailure("PostgreSQL major-version authority is unavailable") from None
	if not isinstance(version, dict) or version.get("major") != 18:
		raise TestFailure("authority candidate capture requires PostgreSQL major 18")
	return version


def psql_secret(
	database: str, sql: str, env: dict[str, str], *, expect_failure: bool = False
) -> str:
	"""Execute secret-bearing SQL only after one live session disables statement logging."""
	process = subprocess.Popen(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database],
		bufsize=0,
		stdin=subprocess.PIPE,
		stdout=subprocess.PIPE,
		stderr=subprocess.PIPE,
		env=env,
		cwd=REPO_ROOT,
	)
	try:
		if process.stdin is None or process.stdout is None or process.stderr is None:
			raise TestFailure("secret-bearing PostgreSQL fixture pipes are unavailable")
		stdin_descriptor = process.stdin.fileno()
		_establish_secret_logging_guard(
			stdin_descriptor, process.stdout.fileno()
		)
		control = b"\\set VERBOSITY terse\n"
		if expect_failure:
			control += b"\\set ON_ERROR_STOP off\n"
		_write_pipe_bytes(stdin_descriptor, control)
		payload = sql
		if not sql.rstrip().endswith(";"):
			payload += ";"
		payload += f"\n\\echo {SECRET_SQL_DONE_MARKER}\n\\quit\n"
		_write_pipe_bytes(stdin_descriptor, payload.encode("utf-8"))
		stdout_bytes, stderr_bytes = process.communicate(timeout=10)
		try:
			stdout = stdout_bytes.decode("utf-8")
			stderr = stderr_bytes.decode("utf-8")
		except UnicodeDecodeError:
			raise TestFailure(
				"secret-bearing PostgreSQL fixture emitted invalid output"
			) from None
		lines = stdout.splitlines()
		if process.returncode != 0 or SECRET_SQL_DONE_MARKER not in lines:
			raise TestFailure("secret-bearing PostgreSQL fixture command failed")
		if expect_failure:
			if "ERROR:" not in stderr:
				raise TestFailure("secret-bearing PostgreSQL failure probe unexpectedly succeeded")
			return ""
		if stderr:
			raise TestFailure("secret-bearing PostgreSQL fixture emitted diagnostics")
		return "\n".join(
			line for line in lines if line != SECRET_SQL_DONE_MARKER
		).strip()
	except subprocess.TimeoutExpired as error:
		raise TestFailure("secret-bearing PostgreSQL fixture command timed out") from error
	finally:
		_raise_child_cleanup_corruption(
			_reap_live_doctor_child(process, "secret-bearing PostgreSQL fixture")
		)


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


def pgcrypto_archive_declaration_is_exact(payload: bytes) -> bool:
	"""Accept only the closed PostgreSQL 18 list grammar needed by this guard."""
	if (
		not payload
		or len(payload) > POSTGRES_ARCHIVE_TOC_MAX_BYTES
		or not payload.endswith(b"\n")
		or b"\r" in payload
		or b"\0" in payload
	):
		return False
	try:
		lines = payload[:-1].decode("ascii").split("\n")
	except UnicodeDecodeError:
		return False
	if (
		lines.count(";     Format: CUSTOM") != 1
		or sum(
			PG18_TOC_DUMP_VERSION_RE.fullmatch(line) is not None for line in lines
		) != 1
		or lines.count("; Selected TOC Entries:") != 1
	):
		return False
	selected_index = lines.index("; Selected TOC Entries:")
	if any(
		not line.startswith(";") or re.fullmatch(r";[ -~]*", line) is None
		for line in lines[:selected_index]
	):
		return False

	active_entries = 0
	active_dump_ids: set[str] = set()
	pgcrypto_entries = 0
	for line in lines[selected_index + 1:]:
		if line == ";":
			continue
		if line.startswith(";"):
			disabled = line[1:]
			if disabled.startswith(" "):
				disabled = disabled[1:]
			if PG18_TOC_ENTRY_RE.fullmatch(disabled) is not None:
				return False
			return False
		entry = PG18_TOC_ENTRY_RE.fullmatch(line)
		if entry is None:
			return False
		active_entries += 1
		dump_id = entry.group("dump_id")
		if dump_id in active_dump_ids:
			return False
		active_dump_ids.add(dump_id)
		body = entry.group("body")
		if not body.startswith("EXTENSION"):
			continue
		extension = PG18_TOC_EXTENSION_RE.fullmatch(body)
		if extension is None:
			return False
		if extension.group("tag") != "pgcrypto":
			continue
		if (
			entry.group("table_oid") != "3079"
			or entry.group("object_oid") == "0"
			or extension.group("namespace") != "-"
			or extension.group("owner") != ""
		):
			return False
		pgcrypto_entries += 1
	return active_entries > 0 and pgcrypto_entries == 1


def guard_pgcrypto_archive_declaration(
	dump_path: Path, pg_restore_tool: Path, env: dict[str, str]
) -> bool:
	try:
		postgres_tool_version(pg_restore_tool, "pg_restore", env)
		payload = private_command_output(
			[str(pg_restore_tool), "--list", str(dump_path)],
			env,
			byte_limit=POSTGRES_ARCHIVE_TOC_MAX_BYTES,
			failure_message="PostgreSQL archive declaration is unavailable",
		)
	except TestFailure:
		raise AuthorityRestoreTargetFailure(
			"archive_declaration_guarded", "archive_declaration_invalid"
		) from None
	if not pgcrypto_archive_declaration_is_exact(payload):
		raise AuthorityRestoreTargetFailure(
			"archive_declaration_guarded", "archive_declaration_invalid"
		)
	return True


def _run_authority_restore_stage(
	checkpoint: str, action: Callable[[], object]
) -> object:
	try:
		return action()
	except AuthorityRestoreTargetFailure:
		raise
	except Exception:
		raise AuthorityRestoreTargetFailure(checkpoint, "stage_failed") from None


def restore_authority_capture_target(
	dump_path: Path,
	target_database: str,
	env: dict[str, str],
	pg_restore_tool: Path,
	invocations: AuthorityRestoreInvocations,
	*,
	stage_runner: Callable[[str, Callable[[], object]], object] | None = None,
) -> dict[str, bool]:
	"""Guard, create, precreate, and restore one fresh authority target."""
	def run_stage_owner(
		checkpoint: str,
		action: Callable[[], object],
		*,
		before: Callable[[], None] | None = None,
		validate: Callable[[object], None] | None = None,
	) -> object:
		if stage_runner is not None:
			def gate_action() -> object:
				try:
					if before is not None:
						before()
					result = action()
					if validate is not None:
						validate(result)
					return result
				except AuthorityRestoreTargetFailure as error:
					if error.checkpoint == checkpoint:
						raise
					if (
						checkpoint == "archive_declaration_guarded"
						and error.checkpoint == "restore_database_fresh_template0"
						and error.reason == "duplicate_invocation"
					):
						raise RestorePrerequisiteExpectedFailure(
							"duplicate_invocation"
						) from None
					raise HarnessCorruption(
						"restore prerequisite helper classification is invalid"
					) from None
			return stage_runner(checkpoint, gate_action)
		if before is not None:
			before()
		result = _run_authority_restore_stage(checkpoint, action)
		if validate is not None:
			validate(result)
		return result

	def record_guard_target() -> None:
		invocations.begin_target(target_database)
		invocations.archive_guard += 1

	run_stage_owner(
		"archive_declaration_guarded",
		lambda: guard_pgcrypto_archive_declaration(dump_path, pg_restore_tool, env),
		before=record_guard_target,
	)

	def record_database_create() -> None:
		invocations.database_create += 1

	run_stage_owner(
		"restore_database_fresh_template0",
		lambda: create_database(target_database, env),
		before=record_database_create,
	)

	def record_pgcrypto_absence_check() -> None:
		invocations.pgcrypto_absence_check += 1

	def require_pgcrypto_absent(absence: object) -> None:
		if absence != "0":
			raise AuthorityRestoreTargetFailure(
				"restore_pgcrypto_absent", "target_not_fresh"
			)

	run_stage_owner(
		"restore_pgcrypto_absent",
		lambda: psql_as(
			MIGRATION_ROLE,
			target_database,
			"SELECT pg_catalog.count(*) FROM pg_catalog.pg_extension "
			"WHERE extname='pgcrypto'",
			env,
		),
		before=record_pgcrypto_absence_check,
		validate=require_pgcrypto_absent,
	)

	def record_prerequisite_create() -> None:
		invocations.prerequisite_create += 1

	run_stage_owner(
		"restore_prerequisite_created",
		lambda: psql_as(
			MIGRATION_ROLE, target_database, RESTORE_PREREQUISITE_SQL, env
		),
		before=record_prerequisite_create,
	)

	def record_restore() -> None:
		invocations.restore += 1

	run_stage_owner(
		"restored_once",
		lambda: run(
			[
				str(pg_restore_tool),
				"--exit-on-error",
				"-d",
				target_database,
				str(dump_path),
			],
			env,
		),
		before=record_restore,
	)
	return {
		"archive_declaration_guarded": True,
		"restore_database_fresh_template0": True,
		"restore_pgcrypto_absent": True,
		"restore_prerequisite_created": True,
		"restored_once": True,
	}


def require_authority_restore_invocation_policy(
	invocations: AuthorityRestoreInvocations,
	expected_targets: tuple[str, ...],
) -> dict[str, bool]:
	results = invocations.policy_results(expected_targets)
	if set(results) != {
		"archive_guard_once",
		"restore_database_once",
		"pgcrypto_absence_check_once",
		"restore_prerequisite_once",
		"restore_once",
	} or not all(results.values()):
		raise AuthorityRestoreTargetFailure(
			"gate", "invocation_policy_failed"
		)
	return results


def clone_authority_database(source: str, target: str, env: dict[str, str]) -> None:
	"""Clone the migrated role-neutral template without adding a runtime identity."""
	psql(
		"postgres",
		f"CREATE DATABASE {target} WITH TEMPLATE {source} OWNER {MIGRATION_ROLE}",
		env,
	)
	psql(
		"postgres",
		f"REVOKE CREATE ON DATABASE {target} FROM PUBLIC; "
		f"GRANT CONNECT, CREATE ON DATABASE {target} TO {MIGRATION_ROLE}",
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


def run_migration_through_v24(env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			"postgres_migration_through_v24_fixture", "--exact",
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
			"test-support", "--test", "postgres_store", "--run-ignored", "all",
			"--no-tests=fail", "--",
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


def run_postgres_store_test(test: str, env: dict[str, str]) -> str:
	return run(
		[
			"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
			"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
			test, "--exact",
		],
		env,
	)


def run_postgres_store_contracts(
	socket_dir: Path, port: int, env: dict[str, str]
) -> str:
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
	primary_output = run_postgres_store_test("postgres_store_contract", env)
	isolated_env = env.copy()
	create_database(DIRECT_MISSING_EXTENSION_DATABASE, isolated_env)
	set_contract_urls(
		isolated_env,
		socket_dir,
		port,
		DIRECT_MISSING_EXTENSION_DATABASE,
		RUNTIME_ROLE,
	)
	migration_output = run_migration(isolated_env)
	provision_runtime(DIRECT_MISSING_EXTENSION_DATABASE, RUNTIME_ROLE, isolated_env)
	missing_extension_output = run_postgres_store_test(
		"postgres_store_missing_pgcrypto_is_incompatible", isolated_env
	)
	return "\n".join((primary_output, migration_output, missing_extension_output))


def run_continuation_focused_contracts(
	socket_dir: Path, port: int, env: dict[str, str]
) -> str:
	create_database(DATABASE, env)
	set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
	migration_output = run_migration(env)
	provision_runtime(DATABASE, RUNTIME_ROLE, env)
	return "\n".join((
		migration_output,
		run_postgres_store_contracts(socket_dir, port, env),
	))


def run_reset_card_focused_contracts(
	socket_dir: Path, port: int, env: dict[str, str]
) -> str:
	create_database(DATABASE, env)
	set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
	migration_output = run_migration(env)
	provision_runtime(DATABASE, RUNTIME_ROLE, env)
	atomic_account_output = run_postgres_store_test(
		"reset_cards::account_terminal_mutation_and_receipt_are_atomic_and_replay_exactly",
		env,
	)
	reset_card_output = run_postgres_store_test(
		"reset_cards::reset_card_private_claim_and_reclaim_contract",
		env,
	)
	return "\n".join((migration_output, atomic_account_output, reset_card_output))


def retained_title_authority_inventory(
	work: Path,
	database: str,
	env: dict[str, str],
	source_binding: dict[str, str],
) -> dict[str, object]:
	manifest_path = work / f"{database}-retained-title-authority.json"
	dump_schema_manifest(manifest_path, database, env, structured_errors=True)
	document = load_capture_manifest(
		manifest_path,
		"retained_title",
		database,
		source_binding=source_binding,
		secret_markers=(),
	)
	manifests = require_capture_components(
		document,
		"retained_title",
		database,
		source_binding=source_binding,
		secret_markers=(),
	)
	actual_digests = {
		component: hashlib.sha256(manifest.encode("utf-8")).hexdigest()
		for component, manifest in manifests.items()
	}
	expected_digests = {
		"schema": rust_digest_constant("SCHEMA_CONTRACT_SHA256"),
		"authority": rust_digest_constant("CONFIGURED_AUTHORITY_SHA256"),
	}
	return {
		"actual_digests": actual_digests,
		"configured_authority_inventory": authority_manifest_evidence(
			manifests["authority"]
		),
		"expected_digests": expected_digests,
		"schema_inventory": authority_manifest_evidence(manifests["schema"]),
	}


def prepare_postgres_authority_inventory(
	socket_dir: Path, port: int, work: Path, env: dict[str, str]
) -> str:
	working_binding = {
		"head": git_read_text(
			"rev-parse", "--verify", "HEAD", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
		"tree": git_read_text(
			"rev-parse", "--verify", "HEAD^{tree}", byte_limit=GIT_METADATA_MAX_BYTES
		).strip(),
	}
	fresh_inventory = retained_title_authority_inventory(
		work, POSTGRES_PREPARATION_DATABASE, env, working_binding
	)
	if fresh_inventory["actual_digests"] != fresh_inventory["expected_digests"]:
		raise TestFailure("V27 fresh closed-authority digests do not match authority.rs")
	fresh_runtime_authority = capture_runtime_authority(
		POSTGRES_PREPARATION_DATABASE, env
	)
	if (
		fresh_runtime_authority["direct_non_grantable_execute_count"]
		!= len(RUNTIME_EXECUTE_SIGNATURES)
		or fresh_runtime_authority["direct_non_grantable_type_usage_count"]
		!= len(RUNTIME_TYPE_NAMES)
	):
		raise TestFailure("V27 fresh runtime authority counts are not exact")

	create_database(AUTHORITY_CAPTURE_UPGRADE_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, AUTHORITY_CAPTURE_UPGRADE_DATABASE, RUNTIME_ROLE
	)
	run_migration_through_v24(env)
	upgrade_v24_ledger = capture_migration_ledger(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env, through_version=24
	)
	psql_as(
		MIGRATION_ROLE,
		AUTHORITY_CAPTURE_UPGRADE_DATABASE,
		f"GRANT USAGE ON TYPE {', '.join(PRE_V27_RUNTIME_TYPE_NAMES)} "
		f"TO {RUNTIME_ROLE}; "
		f"GRANT EXECUTE ON FUNCTION {AUTHORITY_ANCHOR_SIGNATURE} TO {RUNTIME_ROLE}",
		env,
	)
	upgrade_anchor_binding = capture_upgrade_anchor_binding(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	upgrade_type_bindings = capture_upgrade_type_bindings(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	run_migration(env)
	upgrade_v27_ledger = capture_migration_ledger(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	if (
		len(upgrade_v24_ledger) != 24
		or upgrade_v24_ledger[-1].get("version") != 24
		or len(upgrade_v27_ledger) != 27
		or upgrade_v27_ledger[-1].get("version") != 27
		or upgrade_v27_ledger[-1].get("name") != "mac_account_lifecycle"
	):
		raise TestFailure("V27 upgrade migration ledger is not exact")
	upgrade_runtime_authority = capture_upgrade_runtime_authority(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)

	return json.dumps({
		"schema": "decodex/postgres-preparation-stage/1",
		"stage": "v27_closed_authority",
		"fresh_inventory": fresh_inventory,
		"fresh_runtime_authority": fresh_runtime_authority,
		"upgrade": {
			"v24_ledger": upgrade_v24_ledger,
			"pre_v27_anchor_binding": upgrade_anchor_binding,
			"pre_v27_type_bindings": upgrade_type_bindings,
			"v27_ledger": upgrade_v27_ledger,
			"runtime_authority": upgrade_runtime_authority,
		},
	}, sort_keys=True)


def run_retained_title_core_boundary(
	socket_dir: Path,
	port: int,
	work: Path,
	env: dict[str, str],
	source_binding: dict[str, str],
) -> dict[str, object]:
	run(
		[
			"python3", "-m", "unittest",
			"tests.scripts.test_vnext_architecture.VnextArchitectureTests."
			"test_v22_retained_title_bridge_is_two_effect_and_production_inert",
		],
		env,
	)
	create_database(DATABASE, env)
	set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
	run_migration(env)
	ledger = capture_migration_ledger(DATABASE, env)
	if (
		len(ledger) != 22
		or ledger[-1].get("version") != 22
		or ledger[-1].get("name") != "retained_title_experiment_bridge"
	):
		raise TestFailure("retained-title boundary migration ledger is not exact V14-V22")
	provision_runtime(DATABASE, RUNTIME_ROLE, env)
	runtime_authority = capture_runtime_authority(DATABASE, env)
	authority_inventory = retained_title_authority_inventory(
		work, DATABASE, env, source_binding
	)
	if authority_inventory["actual_digests"] != authority_inventory["expected_digests"]:
		raise TestFailure("retained-title boundary authority digests do not match source")
	run_postgres_store_test("postgres_store_contract", env)
	end_binding = staged_source_binding()
	if end_binding != source_binding:
		raise TestFailure("retained-title staged source binding changed during execution")
	return {
		"acceptance": "V14-V22 retained-title core",
		"architecture_contract": {"status": "passed"},
		"commands": {
			"architecture": (
				"python3 -m unittest tests.scripts.test_vnext_architecture."
				"VnextArchitectureTests."
				"test_v22_retained_title_bridge_is_two_effect_and_production_inert"
			),
			"postgres": (
				"cargo nextest run -p decodex-postgres --features test-support "
				"--test postgres_store --run-ignored all -- "
				"postgres_store_contract --exact"
			),
		},
		"deferred": {
			"XY-1363": "live creation and Desktop discovery",
			"XY-1304": (
				"aggregate validation, production enablement, trusted full-check "
				"publication, and landing"
			),
		},
		"ledger": ledger,
		"pinned_app_server": {
			"primary_source": (
				"https://github.com/openai/codex/tree/rust-v0.145.0-alpha.18/"
				"codex-rs/app-server-protocol/src/protocol/v2"
			),
			"thread_name_on_start_result": "nullable",
			"title_mutation": "thread/name/set",
			"version": "codex-cli 0.145.0-alpha.18",
		},
		"postgres_contract": {"status": "passed"},
		"proofs": [
			"one-shot creation fence",
			"one-shot title fence",
			"exact start response binding",
			"exact-ID thread/read binding",
			"retained-title attestation",
			"V17 eligibility transition",
			"structural production non-reachability",
		],
		"runtime_authority": runtime_authority,
		"schema": "decodex/postgres-retained-title-acceptance/1",
		"source_binding": {"start": source_binding, "end": end_binding},
		**authority_inventory,
	}


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


def _validate_semantic_manifest_shape(
	document: object, location: str, *, require_semantic_authority: bool = False
) -> None:
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


def validate_semantic_manifest(
	document: object, location: str, *, require_semantic_authority: bool = False
) -> dict[str, object]:
	_validate_semantic_manifest_shape(
		document, location, require_semantic_authority=require_semantic_authority
	)
	assert isinstance(document, dict)
	if require_semantic_authority:
		validate_semantic_authority_evidence(document["semantic_authority"])
	return document


def _append_semantic_authority_field(canonical: bytearray, value: str) -> None:
	encoded = value.encode("utf-8")
	if len(encoded) > 0xffffffff:
		raise TestFailure("semantic authority definition field is too large")
	canonical.extend(len(encoded).to_bytes(4, "big"))
	canonical.extend(encoded)


def semantic_authority_definition_fingerprint(
	definition: dict[str, object],
) -> str:
	predicates = definition["predicates"]
	assert isinstance(predicates, list)
	canonical = bytearray(SEMANTIC_AUTHORITY_FINGERPRINT_DOMAIN)
	_append_semantic_authority_field(canonical, SEMANTIC_AUTHORITY_SCHEMA)
	_append_semantic_authority_field(
		canonical, SEMANTIC_AUTHORITY_DEFINITION_SCHEMA
	)
	canonical.extend(len(predicates).to_bytes(4, "big"))
	for predicate in predicates:
		assert isinstance(predicate, dict)
		name = predicate["name"]
		classification = predicate["classification"]
		assert isinstance(name, str) and isinstance(classification, str)
		_append_semantic_authority_field(canonical, name)
		_append_semantic_authority_field(canonical, classification)
	return hashlib.sha256(canonical).hexdigest()


def _parse_semantic_authority_evidence(
	evidence: object,
) -> tuple[dict[str, object], list[dict[str, object]]]:
	if (
		not isinstance(evidence, dict)
		or set(evidence) != {
			"definition", "fingerprint", "observations", "schema"
		}
		or evidence["schema"] != SEMANTIC_AUTHORITY_SCHEMA
		or not isinstance(evidence["definition"], dict)
		or not isinstance(evidence["fingerprint"], str)
		or re.fullmatch(r"[0-9a-f]{64}", evidence["fingerprint"]) is None
		or not isinstance(evidence["observations"], list)
	):
		raise TestFailure("invalid semantic authority evidence")
	definition = evidence["definition"]
	assert isinstance(definition, dict)
	if (
		set(definition) != {"predicates", "schema"}
		or definition["schema"] != SEMANTIC_AUTHORITY_DEFINITION_SCHEMA
		or not isinstance(definition["predicates"], list)
		or not 0 < len(definition["predicates"]) <= SEMANTIC_AUTHORITY_MAX_PREDICATES
	):
		raise TestFailure("invalid semantic authority definition")
	predicates = definition["predicates"]
	assert isinstance(predicates, list)
	names: set[str] = set()
	for predicate in predicates:
		if (
			not isinstance(predicate, dict)
			or set(predicate) != {"classification", "name"}
			or not isinstance(predicate["name"], str)
			or re.fullmatch(r"[a-z][a-z0-9_]{0,63}", predicate["name"]) is None
			or not isinstance(predicate["classification"], str)
			or predicate["classification"] not in SEMANTIC_AUTHORITY_FAILURE_POLICIES
			or predicate["name"] in names
		):
			raise TestFailure("invalid semantic authority definition predicate")
		names.add(predicate["name"])
	recomputed_fingerprint = semantic_authority_definition_fingerprint(definition)
	if evidence["fingerprint"] != recomputed_fingerprint:
		raise TestFailure("semantic authority emitted fingerprint differs")
	if recomputed_fingerprint != SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT:
		raise TestFailure("semantic authority definition is not supported")

	observations = evidence["observations"]
	assert isinstance(observations, list)
	if len(observations) != len(predicates):
		raise TestFailure("semantic authority observation set differs")
	observation_names: set[str] = set()
	for index, observation in enumerate(observations):
		if (
			not isinstance(observation, dict)
			or set(observation) != {"name", "passed"}
			or not isinstance(observation["name"], str)
			or re.fullmatch(r"[a-z][a-z0-9_]{0,63}", observation["name"]) is None
			or type(observation["passed"]) is not bool
			or observation["name"] in observation_names
		):
			raise TestFailure("invalid semantic authority observation")
		observation_names.add(observation["name"])
		if observation["name"] != predicates[index]["name"]:
			raise TestFailure("semantic authority observation order differs")
	return definition, observations


def validate_semantic_authority_evidence(evidence: object) -> list[dict[str, object]]:
	_, observations = _parse_semantic_authority_evidence(evidence)
	if any(observation["passed"] is False for observation in observations):
		raise TestFailure("semantic authority contains a failed observation")
	return observations


def _semantic_authority_success_summary(
	evidence: dict[str, object],
	definition: dict[str, object],
	observations: list[dict[str, object]],
) -> dict[str, object]:
	assert isinstance(evidence, dict)
	evidence_sha256 = hashlib.sha256(json.dumps(
		evidence, sort_keys=True, separators=(",", ":"), ensure_ascii=True
	).encode("utf-8")).hexdigest()
	return {
		"all_passed": True,
		"definition_schema": definition["schema"],
		"evidence_sha256": evidence_sha256,
		"fingerprint": evidence["fingerprint"],
		"observation_count": len(observations),
		"schema": SEMANTIC_AUTHORITY_SCHEMA,
	}


def require_capture_semantic_authority(
	document: dict[str, object],
) -> dict[str, object]:
	evidence = document.get("semantic_authority")
	definition, observations = _parse_semantic_authority_evidence(evidence)
	if any(observation["passed"] is False for observation in observations):
		raise TestFailure("semantic authority contains a failed observation")
	assert isinstance(evidence, dict)
	return _semantic_authority_success_summary(evidence, definition, observations)


def _serialize_semantic_authority_diagnostic(
	diagnostic: dict[str, object],
) -> str:
	return json.dumps(
		diagnostic, sort_keys=True, separators=(",", ":"), ensure_ascii=True
	)


def _candidate_artifact_malformed_failure() -> TestFailure:
	serialized = json.dumps({
		"artifact": {"classification": "artifact_malformed"},
		"capture_only": True,
		"schema": MANIFEST_DIAGNOSTIC_SCHEMA,
	}, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
	return TestFailure("authority candidate capture diagnostic: " + serialized)


def require_semantic_authority_checkpoint_parity(
	checkpoints: dict[str, dict[str, object]],
) -> None:
	if set(checkpoints) != {"source", "restored_once", "restored_twice"}:
		raise TestFailure("semantic authority checkpoint set differs")
	summaries = [
		checkpoints[checkpoint]
		for checkpoint in ("source", "restored_once", "restored_twice")
	]
	if summaries[0] != summaries[1] or summaries[0] != summaries[2]:
		raise TestFailure("semantic authority evidence differs across checkpoints")


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


def postgres_start_failure(
	error: TestFailure, log_path: Path, secret_markers: tuple[str, ...]
) -> TestFailure:
	"""Retain one bounded, redacted startup diagnostic in the primary failure."""
	try:
		with log_path.open("rb") as log_file:
			size = os.fstat(log_file.fileno()).st_size
			offset = max(0, size - POSTGRES_START_LOG_EXCERPT_MAX_BYTES)
			log_file.seek(offset)
			payload = log_file.read(POSTGRES_START_LOG_EXCERPT_MAX_BYTES)
	except OSError as diagnostic_error:
		excerpt = f"<unavailable: {type(diagnostic_error).__name__}>"
		truncated = False
	else:
		excerpt, text_truncated = bounded_redacted_text(
			payload.decode("utf-8", errors="replace"),
			secret_markers,
			POSTGRES_START_LOG_EXCERPT_MAX_BYTES,
		)
		truncated = offset > 0 or text_truncated
		if not excerpt:
			excerpt = "<empty>"
	return TestFailure(
		f"{error}\nPostgreSQL startup log excerpt"
		f"{' (tail truncated)' if truncated else ''}:\n{excerpt}"
	)


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


def parse_candidate_capture_manifest(
	artifact_bytes: bytes,
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> tuple[dict[str, object], dict[str, object]]:
	semantic_diagnostic: str | None = None
	semantic_summary: dict[str, object] | None = None
	try:
		document = json.loads(artifact_bytes.decode("utf-8"))
		_validate_semantic_manifest_shape(
			document, checkpoint, require_semantic_authority=True
		)
		assert isinstance(document, dict)
		validate_capture_manifest_rows(document)
		require_manifest_binding(document, expected_database)
		validate_capture_component_errors(document)

		evidence = document["semantic_authority"]
		definition, observations = _parse_semantic_authority_evidence(evidence)
		validated_source_binding = require_source_binding(
			source_binding, "semantic authority source binding is invalid"
		)
		if checkpoint not in SEMANTIC_AUTHORITY_DIAGNOSTIC_CHECKPOINTS:
			raise TestFailure("semantic authority checkpoint is invalid")

		predicates = definition["predicates"]
		assert isinstance(predicates, list)
		failures = [
			{
				"failure_policy": predicate["classification"],
				"predicate": predicate["name"],
			}
			for predicate, observation in zip(predicates, observations)
			if observation["passed"] is False
		]
		if failures:
			assert isinstance(evidence, dict)
			semantic_diagnostic = _serialize_semantic_authority_diagnostic({
				"checkpoint": checkpoint,
				"definition_fingerprint": evidence["fingerprint"],
				"failures": failures,
				"schema": SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
				"source_binding": validated_source_binding,
			})
			if not isinstance(semantic_diagnostic, str) or any(
				marker and marker in semantic_diagnostic for marker in secret_markers
			):
				raise TestFailure("semantic authority diagnostic redaction failed")
		else:
			assert isinstance(evidence, dict)
			semantic_summary = _semantic_authority_success_summary(
				evidence, definition, observations
			)
	except Exception:
		raise _candidate_artifact_malformed_failure() from None

	if semantic_diagnostic is not None:
		raise SemanticAuthorityDiagnostic(semantic_diagnostic)
	assert semantic_summary is not None
	return document, semantic_summary


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


def load_candidate_capture_manifest(
	path: Path,
	checkpoint: str,
	expected_database: str,
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> tuple[dict[str, object], dict[str, object]]:
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
	return parse_candidate_capture_manifest(
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
			CONSTRAINT_CONTRACT_FIELDS, before, after
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


def require_restore_work_passed(
	report: dict[str, object], owner: str, required: tuple[str, ...]
) -> None:
	"""Promote required nested restore results to their top-level owner stage."""
	stages = report.get("stages")
	if not isinstance(stages, dict):
		raise HarnessCorruption(f"{owner} restore report state is invalid")
	failures: list[str] = []
	for name in required:
		result = stages.get(name)
		if not isinstance(result, dict):
			raise HarnessCorruption(f"{owner} restore result {name} is missing or invalid")
		status = result.get("status")
		if status not in {"passed", "failed", "unavailable"}:
			raise HarnessCorruption(f"{owner} restore result {name} has invalid status")
		if status != "passed":
			failures.append(f"{name} is {status}")
	if failures:
		raise TestFailure(f"{owner} required restore work failed: {', '.join(failures)}")


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
	except Exception as error:
		stage_failures.append(f"baseline manifest capture failed:\n{error}")

	if checkpoints["baseline"] is not None:
		try:
			command_output = run_work_item_test("postgres_exact_work_item_commands", env)
		except Exception as error:
			source_behavior_error = error
	else:
		source_behavior_error = TestFailure(
			"source behavior was not run because the baseline manifest is unavailable"
		)

	try:
		dump_schema_manifest(post_attempt_path, WORK_ITEM_DATABASE, env)
		checkpoints["post_attempt"] = load_semantic_manifest(post_attempt_path)
	except Exception as error:
		stage_failures.append(f"post-attempt manifest capture failed:\n{error}")

	dump_path = work / "xy1343-work-items.dump"
	dump_succeeded = False
	restore_database_created = False
	try:
		run(["pg_dump", "-Fc", "-f", str(dump_path), WORK_ITEM_DATABASE], env)
		dump_succeeded = True
	except Exception as error:
		stage_failures.append(f"post-attempt pg_dump failed:\n{error}")
	if dump_succeeded:
		try:
			create_database(WORK_ITEM_RESTORE_DATABASE, env)
			restore_database_created = True
		except Exception as error:
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
		except Exception as error:
			stage_failures.append(f"post-attempt pg_restore failed:\n{error}")
		set_contract_urls(env, socket_dir, port, WORK_ITEM_RESTORE_DATABASE, RUNTIME_ROLE)
		try:
			dump_schema_manifest(restored_path, WORK_ITEM_RESTORE_DATABASE, env)
			checkpoints["restored"] = load_semantic_manifest(restored_path)
		except Exception as error:
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
		except Exception as error:
			failures.append(f"restored verifier/behavior failed:\n{error}")
	if failures:
		raise TestFailure(
			"XY-1343 focused evidence finalized with failures:\n\n"
			+ "\n\n".join(failures)
		)
	return "\n".join((migration_output, command_output, restore_output))


def run_managed_run_v26_suite(
	socket_dir: Path, port: int, work: Path, env: dict[str, str]
) -> str:
	stage_env = env.copy()
	create_database(MANAGED_RUN_DATABASE, stage_env)
	set_contract_urls(
		stage_env, socket_dir, port, MANAGED_RUN_DATABASE, RUNTIME_ROLE
	)
	migration_output = run_migration(stage_env)
	provision_runtime(MANAGED_RUN_DATABASE, RUNTIME_ROLE, stage_env)
	paths = {
		"baseline": work / "xy1417-managed-run-v26-baseline-manifest.json",
		"post_attempt": work / "xy1417-managed-run-v26-post-attempt-manifest.json",
		"restored": work / "xy1417-managed-run-v26-restored-manifest.json",
	}
	checkpoints: dict[str, dict[str, object] | None] = dict.fromkeys(paths)
	stage_failures: list[str] = []
	command_output = ""
	restore_output = ""
	source_behavior_error: Exception | None = None

	try:
		dump_schema_manifest(paths["baseline"], MANAGED_RUN_DATABASE, stage_env)
		checkpoints["baseline"] = load_semantic_manifest(paths["baseline"])
	except Exception as error:
		stage_failures.append(f"baseline manifest capture failed:\n{error}")
	if checkpoints["baseline"] is not None:
		try:
			command_output = run_managed_run_test(
				"postgres_managed_run_v26_contract", stage_env
			)
		except Exception as error:
			source_behavior_error = error
	else:
		source_behavior_error = TestFailure(
			"source behavior was not run because the baseline manifest is unavailable"
		)
	try:
		dump_schema_manifest(paths["post_attempt"], MANAGED_RUN_DATABASE, stage_env)
		checkpoints["post_attempt"] = load_semantic_manifest(paths["post_attempt"])
	except Exception as error:
		stage_failures.append(f"post-attempt manifest capture failed:\n{error}")

	dump_path = work / "xy1417-managed-run-v26.dump"
	dump_succeeded = False
	restore_database_created = False
	try:
		run(
			["pg_dump", "-Fc", "-f", str(dump_path), MANAGED_RUN_DATABASE],
			stage_env,
		)
		dump_succeeded = True
	except Exception as error:
		stage_failures.append(f"post-attempt pg_dump failed:\n{error}")
	if dump_succeeded:
		try:
			create_database(MANAGED_RUN_RESTORE_DATABASE, stage_env)
			restore_database_created = True
		except Exception as error:
			stage_failures.append(f"restore database creation failed:\n{error}")
	if restore_database_created:
		try:
			run(
				["pg_restore", "--exit-on-error", "-d", MANAGED_RUN_RESTORE_DATABASE,
				 str(dump_path)],
				stage_env,
			)
		except Exception as error:
			stage_failures.append(f"post-attempt pg_restore failed:\n{error}")
		set_contract_urls(
			stage_env, socket_dir, port, MANAGED_RUN_RESTORE_DATABASE, RUNTIME_ROLE
		)
		try:
			dump_schema_manifest(
				paths["restored"], MANAGED_RUN_RESTORE_DATABASE, stage_env
			)
			checkpoints["restored"] = load_semantic_manifest(paths["restored"])
		except Exception as error:
			stage_failures.append(f"restored manifest capture failed:\n{error}")

	diagnostics, manifest_failures = manifest_diagnostics(checkpoints)
	print(
		"XY-1417 ManagedRun V26 semantic manifest diagnostics:\n"
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
				"postgres_managed_run_v26_restore", stage_env
			)
		except Exception as error:
			failures.append(f"restored verifier/behavior failed:\n{error}")
	if failures:
		raise TestFailure(
			"XY-1417 ManagedRun V26 stage finalized with failures:\n\n"
			+ "\n\n".join(failures)
		)
	return "\n".join((migration_output, command_output, restore_output))


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
		f"GRANT SELECT ON TABLE decodex.accounts TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE ON TABLE "
		f"decodex.quota_windows, decodex.command_receipts, "
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
		f"GRANT SELECT ON TABLE decodex.managed_runs, "
		f"decodex.managed_run_assignments TO {role}; "
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
	database: str, env: dict[str, str], *, through_version: int | None = None
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
	if through_version is not None:
		expected_identity = [
			identity for identity in expected_identity if identity[0] <= through_version
		]
	if actual_identity != expected_identity:
		raise TestFailure("authority candidate migration ledger differs from embedded source")
	return ledger


def prepare_changed_postgres_migrations(
	socket_dir: Path, port: int, env: dict[str, str]
) -> str:
	create_database(POSTGRES_PREPARATION_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, POSTGRES_PREPARATION_DATABASE, RUNTIME_ROLE
	)
	run([
		"cargo", "nextest", "run", "-p", "decodex-postgres", "--lib", "--",
		"migrations::tests::embedded_migrations_do_not_schema_qualify_postgresql_syntax_constructs",
		"--exact",
	], env)
	run([
		"cargo", "nextest", "run", "-p", "decodex-postgres", "--lib", "--",
		"authority::tests::canonical_inventory_covers_every_shipped_decodex_function_once",
		"--exact",
	], env)
	run_migration(env)
	ledger = capture_migration_ledger(POSTGRES_PREPARATION_DATABASE, env)
	if (
		len(ledger) != 27
		or ledger[-1].get("version") != 27
		or ledger[-1].get("name") != "mac_account_lifecycle"
		or not isinstance(ledger[-1].get("checksum"), str)
	):
		raise TestFailure("PostgreSQL preparation did not reach the exact V1-V27 ledger")
	return json.dumps({
		"schema": "decodex/postgres-preparation-stage/1",
		"stage": "migration_syntax",
		"migration_count": len(ledger),
		"terminal_migration": ledger[-1],
	}, sort_keys=True)


def prepare_changed_embedded_sql(env: dict[str, str]) -> str:
	provision_runtime(POSTGRES_PREPARATION_DATABASE, RUNTIME_ROLE, env)
	output = run([
		"cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
		"test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
		"postgres_changed_sql_preparation_contract", "--exact", "--nocapture",
	], env)
	match = re.search(r"decodex_changed_sql_prepared=([1-9][0-9]*)", output)
	if match is None or int(match.group(1)) != 27:
		raise TestFailure("changed embedded SQL preparation source count is not exact")
	return json.dumps({
		"schema": "decodex/postgres-preparation-stage/1",
		"stage": "changed_embedded_sql_prepare",
		"source_count": int(match.group(1)),
	}, sort_keys=True)


def capture_runtime_authority(database: str, env: dict[str, str]) -> dict[str, object]:
	probe = json.loads(psql(
		database,
		"SELECT pg_catalog.json_build_object("
		f"'database',pg_catalog.current_database(),'migration_role','{MIGRATION_ROLE}',"
		f"'runtime_role','{RUNTIME_ROLE}',"
		f"'non_default_runtime_role','{RUNTIME_ROLE}'<>'decodex_runtime',"
		f"'runtime_login',(SELECT rolcanlogin FROM pg_catalog.pg_roles WHERE rolname='{RUNTIME_ROLE}'),"
		f"'anchor_execute',pg_catalog.has_function_privilege('{RUNTIME_ROLE}',"
		f"'{AUTHORITY_ANCHOR_SIGNATURE}','EXECUTE'),"
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
		raise TestFailure("V24 upgrade anchor binding is not direct and non-grantable")
	return rows[0]


def capture_upgrade_type_bindings(database: str, env: dict[str, str]) -> list[object]:
	type_values = ",".join(
		f"('{identity}',pg_catalog.to_regtype('{identity}'))"
		for identity in sorted(PRE_V27_RUNTIME_TYPE_NAMES)
	)
	rows = json.loads(psql(
		database,
		"WITH allowed(identity,oid) AS (VALUES " + type_values + ") "
		"SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
		"'identity',allowed.identity,'catalog_identity',type.oid::pg_catalog.regtype::text,"
		"'grantor',pg_catalog.pg_get_userbyid(privilege.grantor),"
		"'grantee',pg_catalog.pg_get_userbyid(privilege.grantee),"
		"'is_grantable',privilege.is_grantable) ORDER BY allowed.identity),"
		"'[]'::pg_catalog.json)::text FROM allowed "
		"JOIN pg_catalog.pg_type AS type ON type.oid=allowed.oid "
		"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(type.typacl,"
		"pg_catalog.acldefault('T',type.typowner))) AS privilege WHERE "
		"privilege.privilege_type='USAGE' "
		f"AND privilege.grantee='{RUNTIME_ROLE}'::pg_catalog.regrole",
		env,
	))
	if (
		not isinstance(rows, list)
		or [row.get("identity") for row in rows if isinstance(row, dict)]
		!= list(sorted(PRE_V27_RUNTIME_TYPE_NAMES))
		or any(
			not isinstance(row, dict)
			or row.get("grantor") != MIGRATION_ROLE
			or row.get("grantee") != RUNTIME_ROLE
			or row.get("is_grantable") is not False
			for row in rows
		)
	):
		raise TestFailure("V24 upgrade type binding is not direct and non-grantable")
	return rows


def capture_upgrade_runtime_authority(database: str, env: dict[str, str]) -> dict[str, object]:
	allowed_function_identities = tuple(sorted((
		AUTHORITY_ANCHOR_SIGNATURE,
		*UPGRADE_RUNTIME_EXECUTE_SIGNATURES,
	)))
	allowed_type_identities = tuple(sorted((
		*PRE_V27_RUNTIME_TYPE_NAMES,
		*UPGRADE_RUNTIME_TYPE_NAMES,
	)))
	function_values = ",".join(
		f"('{identity}',pg_catalog.to_regprocedure('{identity}'))"
		for identity in allowed_function_identities
	)
	type_values = ",".join(
		f"('{identity}',pg_catalog.to_regtype('{identity}'))"
		for identity in allowed_type_identities
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
		raise TestFailure("V27 upgrade runtime function authority is not exact")
	if (
		not isinstance(type_grants, list)
		or [row.get("identity") for row in type_grants if isinstance(row, dict)]
		!= list(allowed_type_identities)
		or any(
			not isinstance(row, dict)
			or row.get("grantor") != MIGRATION_ROLE
			or row.get("grantee") != RUNTIME_ROLE
			or row.get("is_grantable") is not False
			for row in type_grants
		)
	):
		raise TestFailure("V27 upgrade runtime type authority is not exact")
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
		raise TestFailure("V27 upgrade added unrelated runtime authority")

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
			"type_usage_grants": [
				row for row in type_grants
				if row["identity"] in UPGRADE_RUNTIME_TYPE_NAMES
			],
		},
		"all_direct_runtime_function_grants": function_grants,
		"all_direct_runtime_type_grants": type_grants,
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
		require_receipt_ledger(ledger, through_version=27)
	if ledgers["source"] != ledgers["restored_once"] or ledgers["source"] != ledgers["restored_twice"]:
		raise TestFailure("Phase A V27 ledgers differ across restore checkpoints")
	if ledgers["source"][-1]["name"] != "mac_account_lifecycle":
		raise TestFailure("Phase A ledger does not end at V27")
	upgrade = require_exact_keys(
		receipt["one_grantee_upgrade"],
		{
			"database", "pre_v27_anchor_binding", "pre_v27_type_bindings",
			"runtime_authority", "v24_ledger", "v27_ledger",
		},
		"Phase A one-grantee upgrade evidence is malformed",
	)
	require_receipt_ledger(upgrade["v24_ledger"], through_version=24)
	upgrade_v27 = require_receipt_ledger(upgrade["v27_ledger"], through_version=27)
	if (
		upgrade["database"] != AUTHORITY_CAPTURE_UPGRADE_DATABASE
		or upgrade_v27 != ledgers["source"]
	):
		raise TestFailure("Phase A one-grantee upgrade does not reach the exact V27 ledger")
	upgrade_authority = require_exact_keys(
		upgrade["runtime_authority"],
		{
			"all_direct_runtime_function_grants", "all_direct_runtime_type_grants",
			"anchor_binding", "database", "migration_delta", "migration_role",
			"runtime_role", "unrelated_authority", "v19_internal_sealing",
		},
		"Phase A one-grantee authority evidence is malformed",
	)
	delta = require_exact_keys(
		upgrade_authority["migration_delta"],
		{"execute_count", "execute_grants", "type_usage_count", "type_usage_grants"},
		"Phase A one-grantee authority delta is malformed",
	)
	pre_v27_type_bindings = upgrade["pre_v27_type_bindings"]
	if (
		upgrade_authority["database"] != AUTHORITY_CAPTURE_UPGRADE_DATABASE
		or upgrade_authority["migration_role"] != MIGRATION_ROLE
		or upgrade_authority["runtime_role"] != RUNTIME_ROLE
		or delta["execute_count"] != len(UPGRADE_RUNTIME_EXECUTE_SIGNATURES)
		or not isinstance(delta["execute_grants"], list)
		or len(delta["execute_grants"]) != len(UPGRADE_RUNTIME_EXECUTE_SIGNATURES)
		or delta["type_usage_count"] != len(UPGRADE_RUNTIME_TYPE_NAMES)
		or not isinstance(delta["type_usage_grants"], list)
		or len(delta["type_usage_grants"]) != len(UPGRADE_RUNTIME_TYPE_NAMES)
		or not isinstance(upgrade_authority["all_direct_runtime_function_grants"], list)
		or len(upgrade_authority["all_direct_runtime_function_grants"])
		!= 1 + len(UPGRADE_RUNTIME_EXECUTE_SIGNATURES)
		or not isinstance(upgrade_authority["all_direct_runtime_type_grants"], list)
		or len(upgrade_authority["all_direct_runtime_type_grants"])
		!= len(PRE_V27_RUNTIME_TYPE_NAMES) + len(UPGRADE_RUNTIME_TYPE_NAMES)
		or not isinstance(pre_v27_type_bindings, list)
		or len(pre_v27_type_bindings) != len(PRE_V27_RUNTIME_TYPE_NAMES)
		or not isinstance(upgrade_authority["v19_internal_sealing"], list)
		or len(upgrade_authority["v19_internal_sealing"]) != len(V19_INTERNAL_SIGNATURES)
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
		or [
			row.get("identity")
			for row in upgrade_authority["all_direct_runtime_type_grants"]
		]
		!= list(sorted((*PRE_V27_RUNTIME_TYPE_NAMES, *UPGRADE_RUNTIME_TYPE_NAMES)))
		or [row.get("identity") for row in upgrade_authority["v19_internal_sealing"]]
		!= list(sorted(V19_INTERNAL_SIGNATURES))
	):
		raise TestFailure("Phase A one-grantee authority identities differ")
	for grant in (
		*delta["execute_grants"], *delta["type_usage_grants"],
		*upgrade_authority["all_direct_runtime_function_grants"],
		*upgrade_authority["all_direct_runtime_type_grants"],
		*pre_v27_type_bindings,
		upgrade["pre_v27_anchor_binding"], upgrade_authority["anchor_binding"],
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
		upgrade["pre_v27_anchor_binding"] != upgrade_authority["anchor_binding"]
		or upgrade_authority["anchor_binding"]["identity"] != AUTHORITY_ANCHOR_SIGNATURE
		or pre_v27_type_bindings != [
			grant for grant in upgrade_authority["all_direct_runtime_type_grants"]
			if grant["identity"] in PRE_V27_RUNTIME_TYPE_NAMES
		]
	):
		raise TestFailure("Phase A one-grantee V24 authority lineage differs")
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
	for value in semantic.values():
		summary = require_exact_keys(
			value,
			{
				"all_passed",
				"definition_schema",
				"evidence_sha256",
				"fingerprint",
				"observation_count",
				"schema",
			},
			"Phase A semantic authority summary is malformed",
		)
		if (
			summary["schema"] != SEMANTIC_AUTHORITY_SCHEMA
			or summary["definition_schema"] != SEMANTIC_AUTHORITY_DEFINITION_SCHEMA
			or summary["all_passed"] is not True
			or summary["fingerprint"] != SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT
			or not isinstance(summary["observation_count"], int)
			or isinstance(summary["observation_count"], bool)
			or not 0 < summary["observation_count"] <= SEMANTIC_AUTHORITY_MAX_PREDICATES
			or not isinstance(summary["evidence_sha256"], str)
			or re.fullmatch(r"[0-9a-f]{64}", summary["evidence_sha256"]) is None
		):
			raise TestFailure("Phase A semantic authority did not pass exactly")
	require_semantic_authority_checkpoint_parity(semantic)

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


def validate_private_receipt_output_path(path: Path, subject: str) -> None:
	if not path.is_absolute():
		raise TestFailure(f"{subject} output path must be absolute")
	try:
		path.relative_to(REPO_ROOT)
	except ValueError:
		pass
	else:
		raise TestFailure(f"{subject} output path must be outside the source tree")
	parent = path.parent
	for component in (parent, *parent.parents):
		if component.is_symlink():
			raise TestFailure(f"{subject} output path must not contain a symlink")
	if not parent.is_dir() or parent.resolve(strict=True) != parent:
		raise TestFailure(f"{subject} output parent must be an exact existing directory")
	parent_metadata = parent.stat()
	if parent_metadata.st_uid != os.geteuid() or parent_metadata.st_mode & 0o077:
		raise TestFailure(f"{subject} output parent must be operator-owned and private")
	if path.exists() or path.is_symlink():
		raise TestFailure(f"{subject} output already exists")


def validate_authority_candidate_output_path(path: Path) -> None:
	validate_private_receipt_output_path(path, "authority candidate")


def validate_restore_prerequisite_output_path(path: Path) -> None:
	validate_private_receipt_output_path(path, "restore prerequisite receipt")


def publish_private_receipt(
	path: Path,
	receipt: dict[str, object],
	validator: Callable[[Path], None],
) -> None:
	validator(path)
	parent = path.parent
	payload = (json.dumps(receipt, sort_keys=True, separators=(",", ":")) + "\n").encode()
	if len(payload) > AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES:
		raise TestFailure("private receipt exceeds its byte limit")
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


def publish_authority_candidate(path: Path, receipt: dict[str, object]) -> None:
	publish_private_receipt(path, receipt, validate_authority_candidate_output_path)


def publish_restore_prerequisite_receipt(
	path: Path, receipt: dict[str, object]
) -> None:
	if receipt.get("passed") is True:
		validate_restore_prerequisite_gate_receipt(receipt)
	elif receipt.get("passed") is False:
		validate_restore_prerequisite_gate_diagnostic(receipt)
	else:
		raise TestFailure("restore prerequisite receipt is invalid")
	publish_private_receipt(path, receipt, validate_restore_prerequisite_output_path)


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


def restore_prerequisite_definition() -> dict[str, object]:
	return {
		"archive_grammar": RESTORE_PREREQUISITE_ARCHIVE_GRAMMAR,
		"cli": RESTORE_PREREQUISITE_CLI,
		"diagnostic": {
			"checkpoint_reason_matrix": [
				{"checkpoint": checkpoint, "reasons": list(reasons)}
				for checkpoint, reasons in RESTORE_PREREQUISITE_REASON_MATRIX
			],
			"fields": [
				"acceptance", "cleanup_finalized", "cleanup_status",
				"completed_cleanup_owners", "completed_checkpoints",
				"definition_fingerprint", "failure_document_repaired", "passed",
				"primary_checkpoint", "primary_reason", "required_cleanup_owners",
				"schema", "secondary_cleanup_reason",
				"semantic_authority_diagnostic", "source_binding",
			],
			"failure_document": {
				"fallback": "fixed_receipt_validation_harness_corruption",
				"normal_and_repair_owner": "receipt_validation",
				"preserve_first_primary": True,
			},
			"reason_set": list(RESTORE_PREREQUISITE_DIAGNOSTIC_REASONS),
			"schema": RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA,
			"semantic_diagnostic": {
				"allowed_primary_checkpoints": [
					"source_semantic_authority",
					"restored_once_semantic_authority",
				],
				"schema": SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
			},
			"source_binding": (
				"validated_or_null_before_source_binding_preflight"
			),
		},
		"execution_progress": {
			"checkpoints": list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS),
			"completed_checkpoints": "validated_successful_prefix_only",
			"failure_owner": "first_uncompleted_execution_checkpoint",
		},
		"invocation_policy": list(RESTORE_PREREQUISITE_INVOCATION_POLICIES),
		"lifecycle": {
			"cleanup": {
				"completed_owners": "successful_prefix_only",
				"fault_injection_points": list(
					RESTORE_PREREQUISITE_CLEANUP_FAULT_POINTS
				),
				"finalization": {
					"fail_closed": True,
					"owner": RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
					"required_for_every_status": True,
				},
				"owner_states": list(RESTORE_PREREQUISITE_CLEANUP_OWNER_STATES),
				"required_owner_derivation": {
					"cluster_stop": "cluster_start_attempted",
					"private_work_cleanup": "private_work_exists",
				},
				"required_owner_sequences": [
					list(sequence)
					for sequence in RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES
				],
				"secondary_reason": "cleanup_failed",
				"status_proof": (
					"required_sequence_and_completed_sequence_and_finalization"
				),
				"statuses": list(RESTORE_PREREQUISITE_CLEANUP_STATUSES),
			},
			"owners": list(RESTORE_PREREQUISITE_LIFECYCLE_CHECKPOINTS),
			"precedence": {
				"cleanup": "secondary_unless_no_primary",
				"failure_document_repair": "never_relabels_valid_first_primary",
				"first_primary": "immutable",
				"publication": "never_relabels_primary",
			},
		},
		"pass": {
			"acceptance": False,
			"fields": [
				"acceptance", "cleanup_finalized", "cleanup_status",
				"completed_cleanup_owners", "completed_checkpoints",
				"definition_fingerprint", "gate_only", "invocation_policy",
				"later_phase_a_decision_only", "passed", "postgres_toolchain",
				"required_cleanup_owners", "schema", "source_binding",
			],
			"meaning": "later_revised_phase_a_decision_only",
			"schema": RESTORE_PREREQUISITE_GATE_SCHEMA,
		},
		"postgres": {
			"major": 18,
			"tools": list(POSTGRES_TOOL_NAMES),
		},
		"prerequisite": {
			"role": MIGRATION_ROLE,
			"sql": RESTORE_PREREQUISITE_SQL,
		},
		"publication": {
			"create_only": True,
			"directory_fsync": True,
			"failure_stderr": "same_canonical_diagnostic",
			"fixed_fallback": "receipt_validation_harness_corruption",
			"file_fsync": True,
			"file_mode": "0600",
		},
		"restore": {
			"identity": "bootstrap",
			"options": ["--exit-on-error"],
		},
		"schema": RESTORE_PREREQUISITE_DEFINITION_SCHEMA,
		"semantic_authority_definition_fingerprint": (
			SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT
		),
	}


def restore_prerequisite_definition_fingerprint() -> str:
	fingerprint = hashlib.sha256(json.dumps(
		restore_prerequisite_definition(),
		sort_keys=True,
		separators=(",", ":"),
		ensure_ascii=True,
	).encode("utf-8")).hexdigest()
	if fingerprint != RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT:
		raise HarnessCorruption("restore prerequisite definition fingerprint differs")
	return fingerprint


def parse_restore_prerequisite_semantic_diagnostic(
	serialized: str,
	gate_checkpoint: str,
	source_binding: dict[str, str] | None,
) -> dict[str, object]:
	checkpoint_map = {
		"source_semantic_authority": "source",
		"restored_once_semantic_authority": "restored_once",
	}
	if (
		gate_checkpoint not in checkpoint_map
		or source_binding is None
		or not isinstance(serialized, str)
		or not serialized
		or len(serialized.encode("utf-8")) > AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES
	):
		raise TestFailure("restore prerequisite semantic diagnostic is invalid")
	def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
		result: dict[str, object] = {}
		for key, value in pairs:
			if key in result:
				raise ValueError("duplicate JSON key")
			result[key] = value
		return result
	try:
		document = json.loads(serialized, object_pairs_hook=unique_object)
	except (json.JSONDecodeError, ValueError):
		raise TestFailure("restore prerequisite semantic diagnostic is invalid") from None
	if (
		json.dumps(
			document, sort_keys=True, separators=(",", ":"), ensure_ascii=True
		) != serialized
		or not isinstance(document, dict)
		or set(document) != {
			"checkpoint", "definition_fingerprint", "failures", "schema",
			"source_binding",
		}
		or document["schema"] != SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA
		or document["checkpoint"] != checkpoint_map[gate_checkpoint]
		or document["definition_fingerprint"]
		!= SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT
		or require_source_binding(
			document["source_binding"],
			"restore prerequisite semantic diagnostic is invalid",
		) != source_binding
		or not isinstance(document["failures"], list)
		or not 0 < len(document["failures"]) <= SEMANTIC_AUTHORITY_MAX_PREDICATES
	):
		raise TestFailure("restore prerequisite semantic diagnostic is invalid")
	seen: set[tuple[str, str]] = set()
	for failure in document["failures"]:
		if (
			not isinstance(failure, dict)
			or set(failure) != {"failure_policy", "predicate"}
			or not isinstance(failure["failure_policy"], str)
			or failure["failure_policy"] not in SEMANTIC_AUTHORITY_FAILURE_POLICIES
			or not isinstance(failure["predicate"], str)
			or re.fullmatch(r"[a-z][a-z0-9_]{0,63}", failure["predicate"]) is None
			or (failure["failure_policy"], failure["predicate"]) in seen
		):
			raise TestFailure("restore prerequisite semantic diagnostic is invalid")
		seen.add((failure["failure_policy"], failure["predicate"]))
	return document


def validate_restore_prerequisite_gate_diagnostic(
	diagnostic: object,
) -> dict[str, object]:
	fields = {
		"acceptance", "cleanup_finalized", "cleanup_status",
		"completed_cleanup_owners", "completed_checkpoints",
		"definition_fingerprint", "failure_document_repaired", "passed",
		"primary_checkpoint", "primary_reason", "required_cleanup_owners",
		"schema", "secondary_cleanup_reason", "semantic_authority_diagnostic",
		"source_binding",
	}
	if not isinstance(diagnostic, dict) or set(diagnostic) != fields:
		raise TestFailure("restore prerequisite diagnostic is invalid")
	completed = diagnostic["completed_checkpoints"]
	primary_checkpoint = diagnostic["primary_checkpoint"]
	primary_reason = diagnostic["primary_reason"]
	cleanup_status = diagnostic["cleanup_status"]
	secondary_cleanup_reason = diagnostic["secondary_cleanup_reason"]
	required_cleanup_owners = diagnostic["required_cleanup_owners"]
	completed_cleanup_owners = diagnostic["completed_cleanup_owners"]
	cleanup_primary = primary_checkpoint in {
		*RESTORE_PREREQUISITE_CLEANUP_OWNERS,
		RESTORE_PREREQUISITE_CLEANUP_FINALIZATION_OWNER,
	}
	if (
		diagnostic["schema"] != RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA
		or diagnostic["acceptance"] is not False
		or diagnostic["passed"] is not False
		or diagnostic["definition_fingerprint"]
		!= RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT
		or not isinstance(completed, list)
		or completed
		!= list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS[:len(completed)])
		or not isinstance(primary_checkpoint, str)
		or not isinstance(primary_reason, str)
		or primary_reason
		not in RESTORE_PREREQUISITE_ALLOWED_REASONS.get(primary_checkpoint, ())
		or diagnostic["cleanup_finalized"] is not True
		or type(diagnostic["failure_document_repaired"]) is not bool
		or cleanup_status not in RESTORE_PREREQUISITE_CLEANUP_STATUSES
		or not isinstance(required_cleanup_owners, list)
		or tuple(required_cleanup_owners)
		not in RESTORE_PREREQUISITE_CLEANUP_OWNER_SEQUENCES
		or not isinstance(completed_cleanup_owners, list)
		or completed_cleanup_owners
		!= required_cleanup_owners[:len(completed_cleanup_owners)]
		or (
			cleanup_status == "not_required"
			and (required_cleanup_owners or completed_cleanup_owners)
		)
		or (
			cleanup_status == "passed"
			and (
				not required_cleanup_owners
				or completed_cleanup_owners != required_cleanup_owners
			)
		)
		or (
			cleanup_status == "failed" and (
				secondary_cleanup_reason is not None
				if cleanup_primary else
				secondary_cleanup_reason != "cleanup_failed"
			)
		)
		or (cleanup_status != "failed" and secondary_cleanup_reason is not None)
	):
		raise TestFailure("restore prerequisite diagnostic is invalid")
	if primary_checkpoint in RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS:
		if RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS.index(
			primary_checkpoint
		) != len(completed):
			raise TestFailure("restore prerequisite diagnostic is invalid")
	elif primary_checkpoint in RESTORE_PREREQUISITE_RECEIPT_LIFECYCLE_CHECKPOINTS:
		fixed_fallback = (
			diagnostic["failure_document_repaired"] is True
			and primary_checkpoint == "receipt_validation"
			and primary_reason == "harness_corruption"
		)
		if (
			not fixed_fallback
			and completed != list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS)
		):
			raise TestFailure("restore prerequisite diagnostic is invalid")
	elif not cleanup_primary:
		raise TestFailure("restore prerequisite diagnostic is invalid")
	if (
		cleanup_primary
		and cleanup_status != "failed"
	):
		raise TestFailure("restore prerequisite diagnostic is invalid")
	source_binding = diagnostic["source_binding"]
	source_checkpoint_index = RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS.index(
		"source_binding_preflight"
	)
	if len(completed) > source_checkpoint_index:
		validated_source_binding = require_source_binding(
			source_binding, "restore prerequisite diagnostic is invalid"
		)
	else:
		if source_binding is not None:
			raise TestFailure("restore prerequisite diagnostic is invalid")
		validated_source_binding = None
	semantic = diagnostic["semantic_authority_diagnostic"]
	if semantic is not None:
		if (
			primary_checkpoint not in {
				"source_semantic_authority", "restored_once_semantic_authority",
			}
			or primary_reason != "operation_failed"
			or validated_source_binding is None
		):
			raise TestFailure("restore prerequisite diagnostic is invalid")
		serialized = json.dumps(
			semantic, sort_keys=True, separators=(",", ":"), ensure_ascii=True
		)
		parse_restore_prerequisite_semantic_diagnostic(
			serialized, primary_checkpoint, validated_source_binding
		)
	return diagnostic


def canonical_restore_prerequisite_gate_diagnostic(diagnostic: object) -> str:
	validated = validate_restore_prerequisite_gate_diagnostic(diagnostic)
	return json.dumps(
		validated, sort_keys=True, separators=(",", ":"), ensure_ascii=True
	)


def fixed_restore_prerequisite_failure_diagnostic() -> dict[str, object]:
	"""Return the closed last-resort receipt-validation diagnostic."""
	return validate_restore_prerequisite_gate_diagnostic({
		"acceptance": False,
		"cleanup_finalized": True,
		"cleanup_status": "failed",
		"completed_cleanup_owners": [],
		"completed_checkpoints": [],
		"definition_fingerprint": RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT,
		"failure_document_repaired": True,
		"passed": False,
		"primary_checkpoint": "receipt_validation",
		"primary_reason": "harness_corruption",
		"required_cleanup_owners": [],
		"schema": RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA,
		"secondary_cleanup_reason": "cleanup_failed",
		"semantic_authority_diagnostic": None,
		"source_binding": None,
	})


def populate_authority_capture_database(
	database: str, env: dict[str, str]
) -> None:
	psql_as(
		RUNTIME_ROLE,
		database,
		"INSERT INTO decodex.accounts(account_id,display_label) VALUES "
		"('10000000-0000-4000-8000-000000001300','XY-1300 capture fixture')",
		env,
	)


def capture_candidate_semantic_checkpoint(
	path: Path,
	checkpoint: str,
	database: str,
	env: dict[str, str],
	*,
	source_binding: dict[str, str],
	secret_markers: tuple[str, ...],
) -> tuple[dict[str, object], dict[str, object], dict[str, str]]:
	dump_schema_manifest(path, database, env, structured_errors=True)
	document, semantic_authority = load_candidate_capture_manifest(
		path,
		checkpoint,
		database,
		source_binding=source_binding,
		secret_markers=secret_markers,
	)
	manifests = require_capture_components(
		document,
		checkpoint,
		database,
		source_binding=source_binding,
		secret_markers=secret_markers,
	)
	return document, semantic_authority, manifests


def restore_prerequisite_gate_receipt(
	state: RestorePrerequisiteGateState,
) -> dict[str, object]:
	source_binding = state.source_binding
	toolchain_fingerprint = state.toolchain_fingerprint
	invocation_policy = state.invocation_policy
	if (
		state.primary_checkpoint is not None
		or state.completed_checkpoints
		!= RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS
		or state.cleanup_status != "passed"
		or source_binding is None
		or toolchain_fingerprint is None
		or invocation_policy is None
	):
		raise TestFailure("restore prerequisite receipt is invalid")
	return {
		"acceptance": False,
		"cleanup_finalized": state.cleanup_finalization_completed,
		"cleanup_status": "passed",
		"completed_cleanup_owners": list(state.completed_cleanup_owners),
		"completed_checkpoints": list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS),
		"definition_fingerprint": restore_prerequisite_definition_fingerprint(),
		"gate_only": True,
		"invocation_policy": invocation_policy,
		"later_phase_a_decision_only": True,
		"passed": True,
		"postgres_toolchain": {
			"authority_fingerprint": toolchain_fingerprint,
			"major_18": True,
			"stable": True,
		},
		"required_cleanup_owners": list(state.required_cleanup_owners),
		"schema": RESTORE_PREREQUISITE_GATE_SCHEMA,
		"source_binding": {
			"start": source_binding,
			"end": source_binding,
		},
	}


def validate_restore_prerequisite_gate_receipt(
	receipt: object,
) -> dict[str, object]:
	if not isinstance(receipt, dict):
		raise TestFailure("restore prerequisite receipt is invalid")
	try:
		if set(receipt) != {
			"acceptance", "cleanup_finalized", "cleanup_status",
			"completed_cleanup_owners", "completed_checkpoints",
			"definition_fingerprint", "gate_only", "invocation_policy",
			"later_phase_a_decision_only", "passed", "postgres_toolchain",
			"required_cleanup_owners", "schema", "source_binding",
		}:
			raise TestFailure("restore prerequisite receipt is invalid")
		source_binding = receipt["source_binding"]
		assert isinstance(source_binding, dict)
		start = require_source_binding(
			source_binding["start"], "restore prerequisite source binding is invalid"
		)
		end = require_source_binding(
			source_binding["end"], "restore prerequisite source binding is invalid"
		)
		toolchain = receipt["postgres_toolchain"]
		assert isinstance(toolchain, dict)
		toolchain_fingerprint = toolchain["authority_fingerprint"]
		assert isinstance(toolchain_fingerprint, str)
		invocation_policy = receipt["invocation_policy"]
		assert isinstance(invocation_policy, dict)
	except (AssertionError, KeyError, TestFailure):
		raise TestFailure("restore prerequisite receipt is invalid") from None
	if (
		start != end
		or receipt["schema"] != RESTORE_PREREQUISITE_GATE_SCHEMA
		or receipt["acceptance"] is not False
		or receipt["passed"] is not True
		or receipt["cleanup_finalized"] is not True
		or receipt["cleanup_status"] != "passed"
		or receipt["required_cleanup_owners"]
		!= list(RESTORE_PREREQUISITE_CLEANUP_OWNERS)
		or receipt["completed_cleanup_owners"]
		!= list(RESTORE_PREREQUISITE_CLEANUP_OWNERS)
		or receipt["completed_checkpoints"]
		!= list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS)
		or receipt["definition_fingerprint"]
		!= RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT
		or receipt["gate_only"] is not True
		or receipt["later_phase_a_decision_only"] is not True
		or re.fullmatch(r"[0-9a-f]{64}", toolchain_fingerprint) is None
		or set(invocation_policy) != set(RESTORE_PREREQUISITE_INVOCATION_POLICIES)
		or any(value is not True for value in invocation_policy.values())
	):
		raise TestFailure("restore prerequisite receipt is invalid")
	expected = {
		"acceptance": False,
		"cleanup_finalized": True,
		"cleanup_status": "passed",
		"completed_cleanup_owners": list(RESTORE_PREREQUISITE_CLEANUP_OWNERS),
		"completed_checkpoints": list(RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS),
		"definition_fingerprint": RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT,
		"gate_only": True,
		"invocation_policy": {
			name: True for name in RESTORE_PREREQUISITE_INVOCATION_POLICIES
		},
		"later_phase_a_decision_only": True,
		"passed": True,
		"postgres_toolchain": {
			"authority_fingerprint": toolchain_fingerprint,
			"major_18": True,
			"stable": True,
		},
		"required_cleanup_owners": list(RESTORE_PREREQUISITE_CLEANUP_OWNERS),
		"schema": RESTORE_PREREQUISITE_GATE_SCHEMA,
		"source_binding": {"start": start, "end": start},
	}
	if receipt != expected:
		raise TestFailure("restore prerequisite receipt is invalid")
	return receipt


def run_restore_prerequisite_gate(
	state: RestorePrerequisiteGateState,
	socket_dir: Path,
	port: int,
	work: Path,
	log_path: Path,
	env: dict[str, str],
	secret_markers: tuple[str, ...],
	tools: dict[str, Path],
) -> None:
	expected_source_binding = state.source_binding
	expected_toolchain_fingerprint = state.toolchain_fingerprint
	if expected_source_binding is None or expected_toolchain_fingerprint is None:
		raise HarnessCorruption("restore prerequisite preflight state is invalid")
	def require_source_binding_unchanged() -> dict[str, str]:
		binding = frozen_source_binding()
		if binding != expected_source_binding:
			raise RestorePrerequisiteExpectedFailure("changed")
		return binding
	def require_toolchain_unchanged() -> str:
		fingerprint = postgres_toolchain_fingerprint(tools, env)
		if fingerprint != expected_toolchain_fingerprint:
			raise RestorePrerequisiteExpectedFailure("changed")
		return fingerprint
	state.run("source_binding_gate_start", require_source_binding_unchanged)
	state.run("toolchain_gate_start", require_toolchain_unchanged)
	state.run("server_version", lambda: capture_postgres_version(env))
	state.run("definition_binding", restore_prerequisite_definition_fingerprint)

	gate_invocations = RestorePrerequisiteGateInvocations()
	restore_invocations = AuthorityRestoreInvocations()

	def create_source_database() -> None:
		gate_invocations.record("source_database")
		create_database(RESTORE_PREREQUISITE_SOURCE_DATABASE, env)
		set_contract_urls(
			env, socket_dir, port, RESTORE_PREREQUISITE_SOURCE_DATABASE, RUNTIME_ROLE
		)
	state.run("source_database_created", create_source_database)

	def migrate_source() -> object:
		gate_invocations.record("source_migration")
		return run_migration(env)
	state.run("source_migrated", migrate_source)

	def provision_source() -> None:
		gate_invocations.record("source_provisioning")
		provision_runtime(RESTORE_PREREQUISITE_SOURCE_DATABASE, RUNTIME_ROLE, env)
	state.run(
		"source_provisioned",
		provision_source,
	)

	def populate_source() -> None:
		gate_invocations.record("source_population")
		populate_authority_capture_database(RESTORE_PREREQUISITE_SOURCE_DATABASE, env)
	state.run(
		"source_populated",
		populate_source,
	)

	def capture_source_semantic() -> dict[str, object]:
		gate_invocations.record("source_semantic")
		result = capture_candidate_semantic_checkpoint(
			work / "restore-prerequisite-source.json",
			"source",
			RESTORE_PREREQUISITE_SOURCE_DATABASE,
			env,
			source_binding=expected_source_binding,
			secret_markers=secret_markers,
		)
		if (
			not isinstance(result, tuple)
			or len(result) != 3
			or not isinstance(result[1], dict)
		):
			raise HarnessCorruption("restore prerequisite source semantic state is invalid")
		return result[1]
	source_semantic_authority = state.run(
		"source_semantic_authority",
		capture_source_semantic,
	)

	dump_path = work / "restore-prerequisite-source.dump"
	def create_source_archive() -> object:
		gate_invocations.record("source_dump")
		return run(
			[
				str(tools["pg_dump"]),
				"-Fc",
				"-f",
				str(dump_path),
				RESTORE_PREREQUISITE_SOURCE_DATABASE,
			],
			env,
		)
	state.run("source_archive_created", create_source_archive)

	restore_checkpoints = restore_authority_capture_target(
		dump_path,
		RESTORE_PREREQUISITE_R1_DATABASE,
		env,
		tools["pg_restore"],
		restore_invocations,
		stage_runner=state.run,
	)

	def capture_restored_semantic() -> dict[str, object]:
		set_contract_urls(
			env, socket_dir, port, RESTORE_PREREQUISITE_R1_DATABASE, RUNTIME_ROLE
		)
		gate_invocations.record("restored_semantic")
		result = capture_candidate_semantic_checkpoint(
			work / "restore-prerequisite-restored-once.json",
			"restored_once",
			RESTORE_PREREQUISITE_R1_DATABASE,
			env,
			source_binding=expected_source_binding,
			secret_markers=secret_markers,
		)
		if (
			not isinstance(result, tuple)
			or len(result) != 3
			or not isinstance(result[1], dict)
		):
			raise HarnessCorruption("restore prerequisite restored semantic state is invalid")
		return result[1]
	restored_semantic_authority = state.run(
		"restored_once_semantic_authority",
		capture_restored_semantic,
	)
	def require_semantic_authority_equal() -> None:
		if source_semantic_authority != restored_semantic_authority:
			raise RestorePrerequisiteExpectedFailure("semantic_authority_changed")
	state.run("semantic_authority_equal", require_semantic_authority_equal)

	def require_invocation_policy() -> dict[str, bool]:
		try:
			restore_policy = require_authority_restore_invocation_policy(
				restore_invocations, (RESTORE_PREREQUISITE_R1_DATABASE,)
			)
		except AuthorityRestoreTargetFailure as error:
			if (
				error.checkpoint == "gate"
				and error.reason == "invocation_policy_failed"
			):
				raise RestorePrerequisiteExpectedFailure(
					"invocation_policy_failed"
				) from None
			raise HarnessCorruption(
				"restore prerequisite invocation classification is invalid"
			) from None
		if not isinstance(restore_policy, dict):
			raise HarnessCorruption("restore prerequisite invocation state is invalid")
		invocation_policy = {
			**gate_invocations.policy_results(),
			**restore_policy,
		}
		if (
			set(restore_checkpoints) != {
				"archive_declaration_guarded",
				"restore_database_fresh_template0",
				"restore_pgcrypto_absent",
				"restore_prerequisite_created",
				"restored_once",
			}
			or not all(restore_checkpoints.values())
		):
			raise RestorePrerequisiteExpectedFailure("invocation_policy_failed")
		return state.bind_invocation_policy(invocation_policy)
	state.run("invocation_policy", require_invocation_policy)
	state.run("source_binding_gate_end", require_source_binding_unchanged)
	state.run("toolchain_gate_end", require_toolchain_unchanged)
	state.run(
		"privacy_validation",
		lambda: assert_postgres_logs_redact((log_path,), secret_markers),
	)
	state.run("stopped_after_restored_once", lambda: None)


def run_authority_candidate_capture(
	socket_dir: Path,
	port: int,
	work: Path,
	log_path: Path,
	env: dict[str, str],
	secret_markers: tuple[str, ...],
	phase_a: PhaseAAuthorityReceipt | None = None,
	*,
	pg_dump_tool: Path = Path("pg_dump"),
	pg_restore_tool: Path = Path("pg_restore"),
) -> dict[str, object]:
	start_binding = frozen_source_binding()
	if phase_a is not None:
		validate_phase_b_source_delta(phase_a, start_binding)
	pg_version = capture_postgres_version(env)

	create_database(AUTHORITY_CAPTURE_UPGRADE_DATABASE, env)
	set_contract_urls(
		env, socket_dir, port, AUTHORITY_CAPTURE_UPGRADE_DATABASE, RUNTIME_ROLE
	)
	run_migration_through_v24(env)
	upgrade_v24_ledger = capture_migration_ledger(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env, through_version=24
	)
	psql_as(
		MIGRATION_ROLE,
		AUTHORITY_CAPTURE_UPGRADE_DATABASE,
		f"GRANT USAGE ON TYPE {', '.join(PRE_V27_RUNTIME_TYPE_NAMES)} "
		f"TO {RUNTIME_ROLE}; "
		f"GRANT EXECUTE ON FUNCTION {AUTHORITY_ANCHOR_SIGNATURE} TO {RUNTIME_ROLE}",
		env,
	)
	upgrade_anchor_binding = capture_upgrade_anchor_binding(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	upgrade_type_bindings = capture_upgrade_type_bindings(
		AUTHORITY_CAPTURE_UPGRADE_DATABASE, env
	)
	run_migration(env)
	upgrade_v27_ledger = capture_migration_ledger(AUTHORITY_CAPTURE_UPGRADE_DATABASE, env)
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
	populate_authority_capture_database(AUTHORITY_CAPTURE_DATABASE, env)

	source_path = work / "authority-candidate-source.json"
	if phase_a is None:
		source, source_semantic_authority, source_manifests = (
			capture_candidate_semantic_checkpoint(
				source_path,
				"source",
				AUTHORITY_CAPTURE_DATABASE,
				env,
				source_binding=start_binding,
				secret_markers=secret_markers,
			)
		)
	else:
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
		source_semantic_authority = require_capture_semantic_authority(source)
		source_manifests = require_capture_components(
			source,
			"source",
			AUTHORITY_CAPTURE_DATABASE,
			source_binding=start_binding,
			secret_markers=secret_markers,
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
	run(
		[
			str(pg_dump_tool),
			"-Fc",
			"-f",
			str(dump_path),
			AUTHORITY_CAPTURE_DATABASE,
		],
		env,
	)
	restore_invocations = AuthorityRestoreInvocations()
	restore_authority_capture_target(
		dump_path,
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
		env,
		pg_restore_tool,
		restore_invocations,
	)
	set_contract_urls(env, socket_dir, port, AUTHORITY_CAPTURE_RESTORE_DATABASE, RUNTIME_ROLE)
	restored_path = work / "authority-candidate-restored.json"
	if phase_a is None:
		restored, restored_semantic_authority, restored_manifests = (
			capture_candidate_semantic_checkpoint(
				restored_path,
				"restored_once",
				AUTHORITY_CAPTURE_RESTORE_DATABASE,
				env,
				source_binding=start_binding,
				secret_markers=secret_markers,
			)
		)
		restored_capture_checkpoint = "restored_once"
	else:
		dump_schema_manifest(
			restored_path,
			AUTHORITY_CAPTURE_RESTORE_DATABASE,
			env,
			structured_errors=True,
		)
		restored = load_capture_manifest(
			restored_path,
			"restored",
			AUTHORITY_CAPTURE_RESTORE_DATABASE,
			source_binding=start_binding,
			secret_markers=secret_markers,
		)
		restored_semantic_authority = require_capture_semantic_authority(restored)
		restored_capture_checkpoint = "restored"
		restored_manifests = require_capture_components(
			restored,
			restored_capture_checkpoint,
			AUTHORITY_CAPTURE_RESTORE_DATABASE,
			source_binding=start_binding,
			secret_markers=secret_markers,
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
		str(pg_dump_tool), "-Fc", "-f", str(second_dump_path),
		AUTHORITY_CAPTURE_RESTORE_DATABASE,
	], env)
	restore_authority_capture_target(
		second_dump_path,
		AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		env,
		pg_restore_tool,
		restore_invocations,
	)
	set_contract_urls(
		env, socket_dir, port, AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE, RUNTIME_ROLE
	)
	second_restored_path = work / "authority-candidate-restored-twice.json"
	if phase_a is None:
		second_restored, second_restored_semantic_authority, (
			second_restored_manifests
		) = capture_candidate_semantic_checkpoint(
			second_restored_path,
			"restored_twice",
			AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
			env,
			source_binding=start_binding,
			secret_markers=secret_markers,
		)
	else:
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
		second_restored_semantic_authority = require_capture_semantic_authority(
			second_restored
		)
		second_restored_manifests = require_capture_components(
			second_restored,
			"restored_twice",
			AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
			source_binding=start_binding,
			secret_markers=secret_markers,
		)
	require_authority_restore_invocation_policy(
		restore_invocations,
		(
			AUTHORITY_CAPTURE_RESTORE_DATABASE,
			AUTHORITY_CAPTURE_SECOND_RESTORE_DATABASE,
		),
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
	require_semantic_authority_checkpoint_parity(checkpoint_semantic_authority)
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
			"v24_ledger": upgrade_v24_ledger,
			"pre_v27_anchor_binding": upgrade_anchor_binding,
			"pre_v27_type_bindings": upgrade_type_bindings,
			"v27_ledger": upgrade_v27_ledger,
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
			"v24_ledger": upgrade_v24_ledger,
			"pre_v27_anchor_binding": upgrade_anchor_binding,
			"pre_v27_type_bindings": upgrade_type_bindings,
			"v27_ledger": upgrade_v27_ledger,
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
	local_transport_stage = root / "server" / "decodex.sock.stage"
	local_transport_path_bytes = len(os.fsencode(local_transport_stage)) + 1
	if local_transport_path_bytes > PORTABLE_UNIX_SOCKET_PATH_MAX_BYTES:
		raise TestFailure(
			"Decodex local transport Unix socket path is too long: "
			f"{local_transport_path_bytes} bytes including the terminating NUL exceeds "
			f"the portable {PORTABLE_UNIX_SOCKET_PATH_MAX_BYTES}-byte limit"
		)
	root.mkdir(mode=0o700)
	config_path = root / "config.toml"
	config_path.write_text(
		f'''version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {os.geteuid()}

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


def main(
	restore_prerequisite_state: RestorePrerequisiteGateState | None = None,
) -> (
	int | AuthorityCandidatePublication | RestorePrerequisiteGatePublication
):
	focused_work_items = sys.argv[1:] == ["--focus-work-items"]
	focused_managed_runs = sys.argv[1:] == ["--focus-managed-runs"]
	focused_managed_repositories = sys.argv[1:] == ["--focus-managed-repositories"]
	focused_continuation = sys.argv[1:] == ["--focus-continuation"]
	focused_reset_cards = sys.argv[1:] == ["--focus-reset-cards"]
	focused_authority = sys.argv[1:] == ["--focus-authority-classification"]
	focused_retained_title = sys.argv[1:] == ["--focus-retained-title-core"]
	preparation_mode = sys.argv[1:] == ["--prepare-retained-title-core"]
	capture_only = len(sys.argv) == 3 and sys.argv[1] == "--capture-authority-candidate"
	acceptance_mode = len(sys.argv) == 4 and sys.argv[1] == "--accept-authority-candidate"
	restore_prerequisite_mode = restore_prerequisite_state is not None
	def parse_restore_prerequisite_cli() -> Path:
		if len(sys.argv) != 3 or sys.argv[1] != RESTORE_PREREQUISITE_CLI:
			raise TestFailure("restore prerequisite invocation is invalid")
		return Path(sys.argv[2])
	restore_prerequisite_output = (
		restore_prerequisite_state.run("cli", parse_restore_prerequisite_cli)
		if restore_prerequisite_state is not None else None
	)
	capture_output = (
		Path(sys.argv[3]) if acceptance_mode
		else Path(sys.argv[2]) if capture_only
		else None
	)
	authority_mode = capture_only or acceptance_mode or restore_prerequisite_mode
	normal_aggregate = not (
		focused_work_items or focused_managed_runs or focused_managed_repositories
		or focused_continuation or focused_reset_cards or focused_authority
		or focused_retained_title
		or preparation_mode or authority_mode
	)
	reported_run = (
		normal_aggregate
		or preparation_mode
		or focused_retained_title
		or focused_managed_runs
	)
	orchestrator = StageOrchestrator({}, []) if reported_run or focused_authority else None
	def temporary_root_preflight() -> Path | None:
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
		elif sys.platform == "darwin":
			default_root = Path("/private/tmp")
			if not default_root.is_dir():
				raise TestFailure(
					"default macOS PostgreSQL temporary root is unavailable; set "
					"DECODEX_TEST_TEMP_ROOT to a short existing absolute directory"
				)
			temp_root = default_root.resolve(strict=True)
		return temp_root
	def postgres_tool_discovery() -> dict[str, Path]:
		tools: dict[str, Path] = {}
		for name in POSTGRES_TOOL_NAMES:
			location = shutil.which(name)
			if location is None:
				raise TestFailure(f"required PostgreSQL tool is unavailable: {name}")
			tools[name] = Path(location).resolve(strict=True)
		return tools
	def configuration_preflight() -> dict[str, object]:
		if sys.argv[1:] and not (
			focused_work_items or focused_managed_runs or focused_managed_repositories
			or focused_continuation or focused_reset_cards or focused_authority
			or focused_retained_title
			or preparation_mode or authority_mode
		):
			raise TestFailure(
				"usage: postgres_store_test.py [--focus-work-items|--focus-managed-runs|"
				"--focus-managed-repositories|--focus-continuation|"
				"--focus-reset-cards|"
				"--focus-authority-classification|--focus-retained-title-core|"
				"--prepare-retained-title-core|"
				"--capture-authority-restore-prerequisite-v2 "
				"ABSOLUTE_PRIVATE_RECEIPT_PATH|"
				"--capture-authority-candidate ABSOLUTE_OUTPUT_PATH|"
				"--accept-authority-candidate PHASE_A_RECEIPT ABSOLUTE_OUTPUT_PATH]"
			)
		source_binding = (
			frozen_source_binding() if normal_aggregate
			else staged_source_binding() if focused_retained_title else None
		)
		phase_a = (
			load_phase_a_authority_receipt(Path(sys.argv[2]))
			if acceptance_mode else None
		)
		if phase_a is not None:
			validate_phase_b_source_delta(phase_a, frozen_source_binding())
		if capture_output is not None:
			validate_authority_candidate_output_path(capture_output)
		temp_root = temporary_root_preflight()
		tools = postgres_tool_discovery()
		return {
			"phase_a": phase_a,
			"source_binding": source_binding,
			"temp_root": temp_root,
			"toolchain_fingerprint": None,
			"tools": tools,
		}
	def restore_output_contract() -> Path:
		if (
			restore_prerequisite_state is None
			or not isinstance(restore_prerequisite_output, Path)
		):
			raise HarnessCorruption("restore prerequisite CLI state is invalid")
		validate_restore_prerequisite_output_path(restore_prerequisite_output)
		restore_prerequisite_state.bind_output_path(restore_prerequisite_output)
		return restore_prerequisite_output
	def restore_source_binding_preflight() -> dict[str, str]:
		if restore_prerequisite_state is None:
			raise HarnessCorruption("restore prerequisite state is absent")
		return restore_prerequisite_state.bind_source(frozen_source_binding())
	def restore_temporary_root_preflight() -> Path | None:
		result = temporary_root_preflight()
		if result is not None and not isinstance(result, Path):
			raise HarnessCorruption("restore prerequisite temporary root is invalid")
		return result
	def restore_postgres_tool_discovery() -> dict[str, Path]:
		result = postgres_tool_discovery()
		if (
			set(result) != set(POSTGRES_TOOL_NAMES)
			or any(not isinstance(path, Path) for path in result.values())
		):
			raise HarnessCorruption("restore prerequisite tool discovery is invalid")
		return result
	def restore_toolchain_preflight(tools: dict[str, Path]) -> str:
		if restore_prerequisite_state is None:
			raise HarnessCorruption("restore prerequisite state is absent")
		return restore_prerequisite_state.bind_toolchain(
			postgres_toolchain_fingerprint(tools, os.environ.copy())
		)
	if restore_prerequisite_state is not None:
		restore_prerequisite_state.run("output_contract", restore_output_contract)
		source_binding = restore_prerequisite_state.run(
			"source_binding_preflight", restore_source_binding_preflight
		)
		temp_root = restore_prerequisite_state.run(
			"temporary_root", restore_temporary_root_preflight
		)
		tools = restore_prerequisite_state.run(
			"tool_discovery", restore_postgres_tool_discovery
		)
		toolchain_fingerprint = restore_prerequisite_state.run(
			"toolchain_preflight", lambda: restore_toolchain_preflight(tools)
		)
		preflight: object = {
			"phase_a": None,
			"source_binding": source_binding,
			"temp_root": temp_root,
			"toolchain_fingerprint": toolchain_fingerprint,
			"tools": tools,
		}
	else:
		preflight = (
			run_stage(
				orchestrator,
				"configuration_preflight",
				configuration_preflight,
				fatal=True,
			)
			if orchestrator is not None
			else configuration_preflight()
		)
	if not isinstance(preflight, dict):
		if orchestrator is None or orchestrator.primary_failure is None:
			raise HarnessCorruption("configuration preflight lost its primary failure")
		raise orchestrator.primary_failure
	phase_a = preflight["phase_a"]
	if phase_a is not None and not isinstance(phase_a, PhaseAAuthorityReceipt):
		raise HarnessCorruption("configuration preflight Phase A state is invalid")
	source_binding = preflight["source_binding"]
	if (
		normal_aggregate or focused_retained_title or restore_prerequisite_mode
	) and not isinstance(
		source_binding, dict
	):
		raise HarnessCorruption("configuration preflight source binding is invalid")
	temp_root = preflight["temp_root"]
	toolchain_fingerprint = preflight["toolchain_fingerprint"]
	tools = preflight["tools"]
	if temp_root is not None and not isinstance(temp_root, Path):
		raise HarnessCorruption("configuration preflight temporary root is invalid")
	if not isinstance(tools, dict):
		raise HarnessCorruption("configuration preflight result is invalid")
	if restore_prerequisite_mode and (
		not isinstance(toolchain_fingerprint, str)
		or re.fullmatch(r"[0-9a-f]{64}", toolchain_fingerprint) is None
	):
		raise HarnessCorruption("configuration preflight toolchain binding is invalid")
	work: Path | None = None
	data_dir: Path | None = None
	socket_dir: Path | None = None
	log_path: Path | None = None
	env: dict[str, str] | None = None
	postgres_share: Path | None = None
	# TCP is disabled; the port only distinguishes the socket filename inside this unique directory.
	port = 55_432
	role_setting_canary_guc = ""
	role_setting_secret_canary = ""
	cluster_start_attempted = False
	cluster_started = False
	try:
		def private_environment_setup() -> Path:
			nonlocal work, data_dir, socket_dir, log_path, env, postgres_share
			nonlocal role_setting_canary_guc, role_setting_secret_canary
			work = Path(tempfile.mkdtemp(
				prefix=("decodex-xy1343-" if focused_work_items else
					"decodex-xy1417-" if focused_managed_runs else
					"decodex-xy1364-" if focused_managed_repositories else
					"decodex-xy1364-continuation-" if focused_continuation else
					"decodex-reset-card-" if focused_reset_cards else
					"decodex-xy1364-authority-" if focused_authority else
					"decodex-xy1368-boundary-" if focused_retained_title else
					"decodex-xy1422-preparation-" if preparation_mode else
					"decodex-xy1421-restore-prerequisite-"
					if restore_prerequisite_mode else
					"decodex-xy1300-capture-" if authority_mode else "decodex-xy1267-"),
				dir=temp_root,
			))
			work = work.resolve()
			data_dir = work / "postgres"
			socket_dir = work / "socket"
			log_path = work / "postgres.log"
			socket_path = socket_dir / f".s.PGSQL.{port}"
			socket_path_bytes = len(os.fsencode(socket_path)) + 1
			if socket_path_bytes > PORTABLE_UNIX_SOCKET_PATH_MAX_BYTES:
				raise TestFailure(
					"PostgreSQL Unix socket path is too long: "
					f"{socket_path_bytes} bytes including the terminating NUL exceeds "
					f"the portable {PORTABLE_UNIX_SOCKET_PATH_MAX_BYTES}-byte limit; "
					"set DECODEX_TEST_TEMP_ROOT to a shorter absolute directory"
				)
			role_setting_canary_guc = f"xy1272.canary_{secrets.token_hex(16)}"
			role_setting_secret_canary = secrets.token_hex(32)
			if restore_prerequisite_state is not None:
				restore_prerequisite_state.bind_secret_markers(
					(role_setting_canary_guc, role_setting_secret_canary)
				)
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
			return work
		created_work = (
			restore_prerequisite_state.run("private_work", private_environment_setup)
			if restore_prerequisite_state is not None else
			run_stage(
				orchestrator,
				"private_environment_setup",
				private_environment_setup,
				depends_on=("configuration_preflight",),
				fatal=True,
			)
			if orchestrator is not None else
			private_environment_setup()
		)
		if not isinstance(created_work, Path):
			if orchestrator is None or orchestrator.primary_failure is None:
				raise HarnessCorruption("private environment setup lost its primary failure")
			raise orchestrator.primary_failure
		def initialize_postgres_cluster() -> None:
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
		def start_postgres_cluster() -> None:
			nonlocal cluster_started
			try:
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
			except TestFailure as error:
				raise postgres_start_failure(
					error,
					log_path,
					(role_setting_canary_guc, role_setting_secret_canary),
				) from error
			cluster_started = True
		def create_base_roles() -> None:
			roles = [MIGRATION_ROLE, RUNTIME_ROLE]
			if not authority_mode and not preparation_mode:
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
				attributes = (
					"" if role in group_roles else " LOGIN NOINHERIT VALID UNTIL 'infinity'"
				)
				psql("postgres", f"CREATE ROLE {role}{attributes}", env)

			return None
		def fatal_postgres_preflight() -> None:
			nonlocal cluster_start_attempted
			initialize_postgres_cluster()
			cluster_start_attempted = True
			start_postgres_cluster()
			create_base_roles()
		if restore_prerequisite_state is not None:
			restore_prerequisite_state.run(
				"cluster_init", initialize_postgres_cluster
			)
			cluster_start_attempted = True
			restore_prerequisite_state.run(
				"cluster_start", start_postgres_cluster
			)
			restore_prerequisite_state.run("role_setup", create_base_roles)
		elif orchestrator is None:
			fatal_postgres_preflight()
		else:
			run_stage(
				orchestrator,
				"cluster_preflight",
				fatal_postgres_preflight,
				depends_on=("private_environment_setup",),
				fatal=True,
			)
			if orchestrator.stages["cluster_preflight"]["status"] != "passed":
				if orchestrator.primary_failure is None:
					raise HarnessCorruption("fatal preflight lost its primary failure")
				raise orchestrator.primary_failure

		if focused_work_items:
			print(run_work_item_focused_contracts(socket_dir, port, work, env))
			return 0
		if focused_managed_runs:
			run_stage(
				orchestrator,
				"managed_run_v26_suite",
				lambda: run_managed_run_v26_suite(socket_dir, port, work, env),
				depends_on=("cluster_preflight",),
			)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			return 0
		if focused_managed_repositories:
			print(run_managed_repository_focused_contracts(socket_dir, port, env))
			return 0
		if focused_continuation:
			print(run_continuation_focused_contracts(socket_dir, port, env))
			return 0
		if focused_reset_cards:
			print(run_reset_card_focused_contracts(socket_dir, port, env))
			return 0
		if focused_retained_title:
			if not isinstance(source_binding, dict):
				raise HarnessCorruption("retained-title source binding is invalid")
			receipt = run_stage(
				orchestrator,
				"retained_title_postgres_boundary",
				lambda: run_retained_title_core_boundary(
					socket_dir, port, work, env, source_binding
				),
				depends_on=("cluster_preflight",),
			)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			print(json.dumps(receipt, sort_keys=True, separators=(",", ":")))
			return 0
		if preparation_mode:
			run_stage(
				orchestrator,
				"migration_syntax",
				lambda: prepare_changed_postgres_migrations(socket_dir, port, env),
				depends_on=("cluster_preflight",),
			)
			run_stage(
				orchestrator,
				"changed_embedded_sql_prepare",
				lambda: prepare_changed_embedded_sql(env),
				depends_on=("migration_syntax",),
			)
			run_stage(
				orchestrator,
				"generated_authority_inventory",
				lambda: prepare_postgres_authority_inventory(
					socket_dir, port, work, env
				),
				depends_on=("changed_embedded_sql_prepare",),
			)
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			return 0
		if restore_prerequisite_output is not None:
			if (
				restore_prerequisite_state is None
				or not isinstance(source_binding, dict)
				or not isinstance(toolchain_fingerprint, str)
			):
				raise HarnessCorruption(
					"restore prerequisite preflight binding is invalid"
				)
			run_restore_prerequisite_gate(
				restore_prerequisite_state,
				socket_dir,
				port,
				work,
				log_path,
				env,
				(role_setting_canary_guc, role_setting_secret_canary),
				tools,
			)
			return RestorePrerequisiteGatePublication(
				restore_prerequisite_output, restore_prerequisite_state
			)
		if capture_output is not None:
			capture_receipt = run_authority_candidate_capture(
				socket_dir,
				port,
				work,
				log_path,
				env,
				(role_setting_canary_guc, role_setting_secret_canary),
				phase_a,
				pg_dump_tool=tools["pg_dump"],
				pg_restore_tool=tools["pg_restore"],
			)
			return AuthorityCandidatePublication(capture_output, capture_receipt)
		if orchestrator is None or (
			normal_aggregate and not isinstance(source_binding, dict)
		):
			raise HarnessCorruption("normal aggregate orchestration state is invalid")
		if focused_authority:
			# Schedule definitions stay canonical, but only the authority boundary executes.
			orchestrator.scheduling_stopped = True
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
			output = run_role_profile_final_gate_contracts(
				data_dir, log_path, socket_dir, port, work, env, restore_report
			)
			require_restore_work_passed(
				restore_report,
				"RoleProfile suite",
				(
					"role_profile_post_command_capture",
					"role_profile_restore",
					"role_profile_restored_capture",
					"role_profile_restored_check",
				),
			)
			return output
		run_stage(
			orchestrator,
			"role_profile_suite",
			role_profile_suite,
			depends_on=("cluster_preflight",),
		)

		def runtime_session_suite() -> str:
			output = run_runtime_session_final_gate_contracts(
				data_dir, log_path, socket_dir, port, work, env, restore_report
			)
			require_restore_work_passed(
				restore_report,
				"RuntimeSession suite",
				("runtime_session_restore", "runtime_session_restored_check"),
			)
			return output
		run_stage(
			orchestrator,
			"runtime_session_suite",
			runtime_session_suite,
			depends_on=("cluster_preflight",),
		)
		run_stage(
			orchestrator,
			"managed_run_v26_suite",
			lambda: run_managed_run_v26_suite(socket_dir, port, work, env),
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
			return run_postgres_store_contracts(socket_dir, port, env)
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
		bootstrap_root = work / "dx-main"
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
		auth_bootstrap_root = work / "dx-auth"
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
			psql(
				"postgres",
				f"REVOKE ALL ON DATABASE {AUTHORITY_DATABASE} FROM {RUNTIME_ROLE}",
				env,
			)
			set_contract_urls(env, socket_dir, port, AUTHORITY_DATABASE, RUNTIME_ROLE)
			run_migration(env)
			scenarios: list[AuthorityScenario] = []
			def add_authority_scenario(
				case_id: str,
				expected_store_error: AuthorityStoreError,
				expected: AuthorityClassification,
				role: str,
				*,
				mutation_sql: str,
				precondition_sql: str,
				postcondition_sql: str,
				pre_runtime_rejected_sql: str | None = None,
				pre_runtime_rejected_sqlstate: str | None = None,
				post_runtime_rejected_sql: str | None = None,
				post_runtime_rejected_sqlstate: str | None = None,
				runtime_effect_sql: str | None = None,
				restore_sql: str | None = None,
				restore_postcondition_sql: str | None = None,
				invariant_sql: str | None = None,
			) -> None:
				index = len(scenarios)
				case_database = f"decodex_xy1364_case_{index:02d}"
				clone_authority_database(AUTHORITY_DATABASE, case_database, env)
				provision_runtime(case_database, role, env)
				def expand(sql: str | None) -> str | None:
					if sql is None:
						return None
					return sql.replace("$CASE_DATABASE", case_database).replace(
						"$RUNTIME_ROLE", role
					)
				scenarios.append(AuthorityScenario(
					case_id=case_id,
					expected_store_error=expected_store_error,
					expected=expected,
					baseline_migration_url=database_url(
						socket_dir, port, case_database, MIGRATION_ROLE
					),
					baseline_runtime_url=database_url(
						socket_dir, port, case_database, role
					),
					case_migration_url=database_url(
						socket_dir, port, case_database, MIGRATION_ROLE
					),
					case_runtime_url=database_url(socket_dir, port, case_database, role),
					admin_url=database_url(socket_dir, port, case_database, env["PGUSER"]),
					mutation_sql=expand(mutation_sql),
					precondition_sql=expand(precondition_sql),
					postcondition_sql=expand(postcondition_sql),
					pre_runtime_rejected_sql=expand(pre_runtime_rejected_sql),
					pre_runtime_rejected_sqlstate=pre_runtime_rejected_sqlstate,
					post_runtime_rejected_sql=expand(post_runtime_rejected_sql),
					post_runtime_rejected_sqlstate=post_runtime_rejected_sqlstate,
					runtime_effect_sql=expand(runtime_effect_sql),
					restore_sql=expand(restore_sql),
					restore_postcondition_sql=expand(restore_postcondition_sql),
					invariant_sql=expand(invariant_sql),
				))
				if case_id == "truncate":
					root = work / "dx-unsafe"
					write_bootstrap_config(
						root, socket_dir, port, case_database, MIGRATION_ROLE, role
					)
					env["DECODEX_TEST_UNSAFE_AUTHORITY_ROOT"] = str(root)
				elif case_id == "missing-ledger-select":
					root = work / "dx-incompat"
					write_bootstrap_config(
						root, socket_dir, port, case_database, MIGRATION_ROLE, role
					)
					env["DECODEX_TEST_INCOMPATIBLE_AUTHORITY_ROOT"] = str(root)
			register_authority_scenarios(add_authority_scenario)
			unsafe_count = sum(
				scenario.expected is AuthorityClassification.UNSAFE_DATABASE_AUTHORITY
				for scenario in scenarios
			)
			incompatible_count = sum(
				scenario.expected is AuthorityClassification.DATABASE_INCOMPATIBLE
				for scenario in scenarios
			)
			unsafe_store_count = sum(
				scenario.expected_store_error is AuthorityStoreError.UNSAFE_AUTHORITY
				for scenario in scenarios
			)
			incompatible_store_count = sum(
				scenario.expected_store_error is AuthorityStoreError.INCOMPATIBLE
				for scenario in scenarios
			)
			migration_store_count = sum(
				scenario.expected_store_error is AuthorityStoreError.MIGRATION
				for scenario in scenarios
			)
			if unsafe_count != 28 or incompatible_count != 6:
				raise HarnessCorruption(
					"authority scenario inventory must contain exactly 28 unsafe and 6 incompatible cases"
				)
			if (
				unsafe_store_count != 28
				or incompatible_store_count != 5
				or migration_store_count != 1
			):
				raise HarnessCorruption(
					"authority scenario inventory must contain exact 28 unsafe, "
					"5 incompatible, and 1 migration StoreError expectations"
				)
			missing_select = [
				scenario for scenario in scenarios if scenario.case_id == "missing-ledger-select"
			]
			if (
				len(missing_select) != 1
				or missing_select[0].expected_store_error is not AuthorityStoreError.INCOMPATIBLE
				or missing_select[0].expected is not AuthorityClassification.DATABASE_INCOMPATIBLE
			):
				raise HarnessCorruption("missing-ledger-select must be exclusively incompatible")
			ledger_tamper = [
				scenario for scenario in scenarios if scenario.case_id == "ledger-tamper"
			]
			if (
				len(ledger_tamper) != 1
				or ledger_tamper[0].expected_store_error is not AuthorityStoreError.MIGRATION
				or ledger_tamper[0].expected is not AuthorityClassification.DATABASE_INCOMPATIBLE
			):
				raise HarnessCorruption(
					"ledger-tamper must be migration failure projected as incompatible"
				)
			env["DECODEX_TEST_POSTGRES_AUTHORITY_SCENARIOS"] = authority_scenario_payload(
				scenarios
			)
			authority_matrix_features = (
				["--features", "test-support"] if focused_authority else []
			)
			outputs = [run(
				[
					"cargo", "nextest", "run", "-p", "decodex-postgres",
					*authority_matrix_features, "--test",
					"postgres_store", "--run-ignored", "all", "--",
					"postgres_authority_classification_matrix", "--exact",
				],
				env,
			)]
			outputs.append(run(
				[
					"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
					"bootstrap_doctor", "--run-ignored", "all", "--",
					"isolated_postgres_overprivileged_runtime_is_unavailable", "--exact",
				],
				env,
			))
			outputs.append(run(
				[
					"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
					"bootstrap_doctor", "--run-ignored", "all", "--",
					"isolated_postgres_incompatible_runtime_is_unavailable", "--exact",
				],
				env,
			))

			if normal_aggregate:
				clone_authority_database(AUTHORITY_DATABASE, IDENTITY_CAST_DATABASE, env)
				provision_runtime(IDENTITY_CAST_DATABASE, RUNTIME_ROLE, env)
				set_contract_urls(env, socket_dir, port, IDENTITY_CAST_DATABASE, RUNTIME_ROLE)
				psql(
					IDENTITY_CAST_DATABASE,
					"CREATE FUNCTION public.xy1315_uuid_to_text(pg_catalog.uuid) "
					"RETURNS pg_catalog.text LANGUAGE sql IMMUTABLE STRICT "
					"AS 'SELECT $1::pg_catalog.text'; CREATE CAST "
					"(pg_catalog.uuid AS pg_catalog.text) WITH FUNCTION "
					"public.xy1315_uuid_to_text(pg_catalog.uuid) AS IMPLICIT",
					env,
				)
				outputs.append(run(
					[
						"cargo", "nextest", "run", "-p", "decodex-postgres", "--test",
						"postgres_store", "--run-ignored", "all", "--",
						"postgres_store_rejects_implicit_uuid_to_text_cast", "--exact",
					],
					env,
				))
				for live_index, (database, case_id, mutation) in enumerate((
					(
						LEDGER_TAMPER_DATABASE,
						"ledger-tamper",
						"UPDATE public.refinery_schema_history SET name=name||'_tampered' WHERE version=1",
					),
					(
						MISSING_EXTENSION_DATABASE,
						"missing-pgcrypto",
						"DROP EXTENSION pgcrypto CASCADE",
					),
				)):
					clone_authority_database(AUTHORITY_DATABASE, database, env)
					provision_runtime(database, RUNTIME_ROLE, env)
					root = work / f"dx-live-{live_index}"
					write_bootstrap_config(
						root, socket_dir, port, database, MIGRATION_ROLE, RUNTIME_ROLE
					)
					outputs.append(run_live_doctor_mutation(
						root,
						database,
						mutation,
						case_id,
						work,
						env,
						LiveDoctorMutationAttempt(),
					))

			return "\n".join(outputs)
		if focused_authority:
			orchestrator.scheduling_stopped = False
		authority_output = run_stage(
			orchestrator,
			"authority_safety_suite",
			authority_safety_suite,
			depends_on=("cluster_preflight",),
		)
		if focused_authority:
			if orchestrator.primary_failure is not None:
				raise orchestrator.primary_failure
			if not isinstance(authority_output, str):
				raise HarnessCorruption("focused authority suite produced no evidence")
			print(authority_output)
			orchestrator.outputs.clear()
			return 0

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
			hostile_search_root = work / "dx-hostile"
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
			failure_offset = len(acceptance_failures)
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
			stage_failures = acceptance_failures[failure_offset:]
			try:
				require_restore_work_passed(
					restore_report,
					"primary restore suite",
					(
						"primary_post_command_capture",
						"primary_restore",
						"primary_restored_capture",
						"primary_sequence_state",
						"primary_store_restored_check",
						"managed_repository_restored_check",
					),
				)
			except TestFailure as error:
				stage_failures.append(str(error))
			if stage_failures:
				raise TestFailure(
					"primary restore suite failed: " + "; ".join(stage_failures)
				)
			aggregate_context["dump_path"] = dump_path
			aggregate_context["live_authority"] = live_authority
			return "\n".join((restore_output, managed_repository_restore_output))
		run_stage(
			orchestrator,
			"primary_restore_suite",
			primary_restore_suite,
			depends_on=(
				"primary_foundation",
				"role_profile_suite",
				"runtime_session_suite",
				"blob_session_restart",
				"postgres_store_contract",
				"managed_repository_contracts",
				"account_composition",
				"bootstrap_doctor_history_daemon",
				"live_endpoint_doctor",
				"authentication_rejection",
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
			require_restore_work_passed(
				restore_report,
				"redaction canary suite",
				("authority_canary_manifest",),
			)
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
			default_acl_tamper_root = work / "dx-acl"
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
				LiveDoctorMutationAttempt(),
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
		authority_drift_stages: list[str] = []
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
			mutation_attempt = LiveDoctorMutationAttempt()
			def authority_mutation_probe() -> str:
				output = run_live_doctor_mutation(
					bootstrap_root,
					DATABASE,
					mutation,
					case,
					work,
					env,
					mutation_attempt,
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
			probe_dependencies = (
				"primary_foundation",
				"bootstrap_configuration",
				"redaction_canary_suite",
			)
			if previous_restoration is not None:
				probe_dependencies += (previous_restoration,)
			run_stage(
				orchestrator,
				probe_stage,
				authority_mutation_probe,
				depends_on=probe_dependencies,
			)
			authority_drift_stages.append(probe_stage)
			restoration_stage = f"authority_drift::{case}::restoration"
			restoration_claim = mutation_attempt.consume_restoration_claim()
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
				depends_on=(probe_stage,),
				always_run=(
					restoration_claim is RestorationClaim.ELIGIBLE_AFTER_DISPATCH
				),
			)
			authority_drift_stages.append(restoration_stage)
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
					"aggregate V14-V27 PostgreSQL acceptance failure:\n"
					+ json.dumps(diagnostics, sort_keys=True)
				)
			return json.dumps(artifact_evidence, sort_keys=True)
		final_dependencies = (
			"primary_foundation",
			"role_profile_suite",
			"runtime_session_suite",
			"managed_run_v26_suite",
			"v8_migration_boundary",
			"blob_session_restart",
			"postgres_store_contract",
			"managed_repository_contracts",
			"account_composition",
			"bootstrap_configuration",
			"bootstrap_doctor_history_daemon",
			"live_endpoint_doctor",
			"authentication_rejection",
			"turkish_collation_contract",
			"authority_safety_suite",
			"hostile_search_path_suite",
			"primary_restore_suite",
			"redaction_canary_suite",
			"default_acl_restore_suite",
			*authority_drift_stages,
			"authority_drift_redaction",
		)
		run_stage(
			orchestrator,
			"final_acceptance_evidence",
			final_acceptance_evidence,
			depends_on=final_dependencies,
		)
		if orchestrator.primary_failure is not None:
			raise orchestrator.primary_failure
		return 0
	finally:
		active_error = sys.exc_info()[1]
		if restore_prerequisite_state is not None:
			if active_error is not None:
				restore_prerequisite_state.capture_unhandled(active_error)
			cleanup_restore_prerequisite_gate(
				restore_prerequisite_state,
				work,
				data_dir,
				env,
				cluster_start_attempted,
			)
			work = None
			data_dir = None
			cluster_start_attempted = False
			cluster_started = False
			if (
				active_error is None
				and restore_prerequisite_state.primary_checkpoint is not None
			):
				raise RestorePrerequisiteGateAbort() from None
		selected_primary = (
			orchestrator.primary_failure
			if orchestrator is not None else
			active_error if isinstance(active_error, Exception) else None
		)
		if (
			orchestrator is not None
			and isinstance(active_error, Exception)
			and orchestrator.primary_failure is None
		):
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
			selected_primary = active_error
		stop_error: Exception | None = None
		teardown_error: Exception | None = None
		report_error: Exception | None = None
		diagnostic_error: Exception | None = None
		stop_failures: list[str] = []
		def teardown_status() -> ClusterStatus:
			try:
				if data_dir is None or env is None:
					raise HarnessCorruption("PostgreSQL teardown state is invalid")
				return (
					postgres_status(data_dir, env)
					if data_dir.exists() else ClusterStatus.STOPPED
				)
			except Exception as error:
				stop_failures.append(f"PostgreSQL status failed:\n{error}")
				return ClusterStatus.UNKNOWN
		status = ClusterStatus.STOPPED
		cluster_observed_running = False
		work_removed = False
		if work is not None and not cluster_start_attempted:
			try:
				shutil.rmtree(work)
				work_removed = True
			except Exception as error:
				teardown_error = TestFailure(
					f"failed to remove private preflight directory {work}: {error}"
				)
		elif cluster_start_attempted:
			status = teardown_status()
			cluster_observed_running = status is ClusterStatus.RUNNING
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
		if stop_diagnostics and not restore_prerequisite_mode:
			stop_diagnostics = f"\n\nShutdown diagnostics:\n{stop_diagnostics}"
		elif restore_prerequisite_mode:
			stop_diagnostics = ""
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
		if (
			cluster_start_attempted
			and status is ClusterStatus.STOPPED
			and work is not None
			and not work_removed
		):
			if stop_diagnostics:
				try:
					print(
						f"PostgreSQL shutdown recovered after an error:{stop_diagnostics}",
						file=sys.stderr,
					)
				except Exception as error:
					diagnostic_error = HarnessCorruption(
						f"PostgreSQL shutdown diagnostic emission failed: {error}"
					)
			try:
				shutil.rmtree(work)
			except Exception as error:
				teardown_error = TestFailure(
					f"failed to remove stopped task-owned PostgreSQL directory {work}: {error}"
				)
		elif stop_error is not None:
			teardown_error = stop_error
		if (
			not cluster_start_attempted
			and teardown_error is not None
			and selected_primary is not None
		):
			if isinstance(selected_primary, StageActionFailure):
				selected_primary = StageActionFailure(
					selected_primary.primary,
					selected_primary.secondary + (teardown_error,),
				)
			else:
				selected_primary = StageActionFailure(
					selected_primary,
					(teardown_error,),
				)
			selected_primary.add_note(
				f"Secondary pre-start private-work cleanup failure: {teardown_error}"
			)
		if selected_primary is None and teardown_error is not None:
			selected_primary = teardown_error
			if orchestrator is not None:
				orchestrator.primary_failure = teardown_error
		if selected_primary is None and diagnostic_error is not None:
			selected_primary = diagnostic_error
			if orchestrator is not None:
				orchestrator.primary_failure = diagnostic_error
				orchestrator.corruption = orchestrator.corruption or diagnostic_error
		if (
			reported_run
			and orchestrator is not None
			and (cluster_started or cluster_observed_running)
		):
			teardown_result: dict[str, object] = (
				{"status": "passed"} if teardown_error is None
				else {"status": "failed", "error": str(teardown_error)}
			)
			if stop_failures:
				teardown_result["diagnostics"] = stop_failures
			if diagnostic_error is not None:
				teardown_result["secondary_failures"] = [{
					"classification": "harness_corruption",
					"error": str(diagnostic_error),
				}]
			orchestrator.stages["teardown"] = teardown_result
			try:
				for output in orchestrator.outputs:
					print(output)
			except Exception as error:
				output_error = HarnessCorruption(
					f"aggregate output emission failed: {error}"
				)
				orchestrator.stages["aggregate_output"] = {
					"status": "failed",
					"classification": "harness_corruption",
					"error": str(output_error),
				}
				orchestrator.corruption = orchestrator.corruption or output_error
				orchestrator.scheduling_stopped = True
				if selected_primary is None:
					selected_primary = output_error
					orchestrator.primary_failure = output_error
			else:
				orchestrator.stages["aggregate_output"] = {"status": "passed"}
			orchestrator.stages["final_report"] = {"status": "passed"}
			report = {
				"schema": (
					"decodex/postgres-preparation-stage-report/1"
					if preparation_mode
					else "decodex/postgres-retained-title-stage-report/1"
					if focused_retained_title
					else "decodex/postgres-managed-run-v26-stage-report/1"
					if focused_managed_runs
					else "decodex/postgres-aggregate-stage-report/1"
				),
				"mode": (
					"vnext_postgres_preparation"
					if preparation_mode
					else "retained_title_boundary"
					if focused_retained_title
					else "managed_run_v26"
					if focused_managed_runs
					else "aggregate"
				),
				"primary_failure": (
					None if selected_primary is None else str(selected_primary)
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
				orchestrator.corruption = orchestrator.corruption or report_error
				if selected_primary is None:
					selected_primary = report_error
					orchestrator.primary_failure = report_error
		if selected_primary is not None and selected_primary is not active_error:
			raise selected_primary


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


def publish_completed_restore_prerequisite_gate(
	publication: RestorePrerequisiteGatePublication,
) -> None:
	state = publication.state
	receipt = state.run_receipt_lifecycle(
		"receipt_validation",
		lambda: validate_restore_prerequisite_gate_receipt(
			restore_prerequisite_gate_receipt(state)
		),
	)
	if not isinstance(receipt, dict):
		raise HarnessCorruption("restore prerequisite receipt state is invalid")
	def require_final_source_binding() -> None:
		expected = state.source_binding
		if expected is None:
			raise HarnessCorruption("restore prerequisite source binding is absent")
		if frozen_source_binding() != expected:
			raise RestorePrerequisiteExpectedFailure("changed")
	state.run_receipt_lifecycle(
		"receipt_source_binding", require_final_source_binding
	)
	state.run_receipt_lifecycle(
		"receipt_publication",
		lambda: publish_restore_prerequisite_receipt(publication.output_path, receipt),
	)


def publish_restore_prerequisite_failure(
	state: RestorePrerequisiteGateState,
) -> None:
	diagnostic = fixed_restore_prerequisite_failure_diagnostic()
	serialized = canonical_restore_prerequisite_gate_diagnostic(diagnostic)
	try:
		state.ensure_cleanup_finalized_without_work()
		def construct_failure_document() -> tuple[dict[str, object], str]:
			result = state.failure_document_with_fixed_fallback()
			return result, canonical_restore_prerequisite_gate_diagnostic(result)
		constructed = state.run_receipt_lifecycle(
			"receipt_validation", construct_failure_document, recovery=True
		)
		if (
			not isinstance(constructed, tuple)
			or len(constructed) != 2
			or not isinstance(constructed[0], dict)
			or not isinstance(constructed[1], str)
		):
			raise HarnessCorruption(
				"restore prerequisite failure publication state is invalid"
			)
		diagnostic, serialized = constructed
		publication_possible = (
			state.output_contract_validated and state.output_path is not None
		)
		if publication_possible:
			if not state.lifecycle_passed("receipt_source_binding"):
				def require_failure_source_binding() -> None:
					expected = state.source_binding
					if expected is not None and frozen_source_binding() != expected:
						raise RestorePrerequisiteExpectedFailure("changed")
				state.run_receipt_lifecycle(
					"receipt_source_binding",
					require_failure_source_binding,
					recovery=True,
				)
			if (
				state.lifecycle_passed("receipt_source_binding")
				and not state.lifecycle_passed("receipt_publication")
			):
				output_path = state.output_path
				if output_path is None:
					raise HarnessCorruption(
						"restore prerequisite output path is absent"
					)
				state.run_receipt_lifecycle(
					"receipt_publication",
					lambda: publish_restore_prerequisite_receipt(
						output_path, diagnostic
					),
					recovery=True,
				)
	except BaseException:
		pass
	finally:
		try:
			print(serialized, file=sys.stderr, flush=True)
		except BaseException:
			pass


def restore_prerequisite_gate_requested() -> bool:
	return len(sys.argv) > 1 and sys.argv[1] == RESTORE_PREREQUISITE_CLI


if __name__ == "__main__":
	restore_prerequisite_state = (
		RestorePrerequisiteGateState()
		if restore_prerequisite_gate_requested() else None
	)
	try:
		result = main(restore_prerequisite_state)
		if isinstance(result, AuthorityCandidatePublication):
			publish_completed_authority_candidate(result)
			exit_code = 0
		elif isinstance(result, RestorePrerequisiteGatePublication):
			publish_completed_restore_prerequisite_gate(result)
			exit_code = 0
		else:
			exit_code = result
	except BaseException as error:
		if restore_prerequisite_state is None:
			raise
		try:
			restore_prerequisite_state.capture_unhandled(error)
			restore_prerequisite_state.ensure_cleanup_finalized_without_work()
		except BaseException:
			pass
		try:
			publish_restore_prerequisite_failure(restore_prerequisite_state)
		except BaseException:
			try:
				print(canonical_restore_prerequisite_gate_diagnostic(
					fixed_restore_prerequisite_failure_diagnostic()
				), file=sys.stderr, flush=True)
			except BaseException:
				pass
		exit_code = 1
	raise SystemExit(exit_code)
