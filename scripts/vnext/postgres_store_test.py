#!/usr/bin/env python3
"""Run XY-1267 integration tests in a disposable PostgreSQL 18 cluster."""

from __future__ import annotations

from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import subprocess
import sys
import tempfile
import time


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1267"
COLLATION_DATABASE = "decodex_xy1267_tr"
RESTORE_DATABASE = "decodex_xy1267_restore"
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
MIGRATION_ROLE = "decodex_migration"
RUNTIME_ROLE = "decodex_runtime"
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
)


class TestFailure(RuntimeError):
	"""Raised when isolated PostgreSQL setup or the integration test fails."""


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


def run_authority_drift_case(
	root: Path,
	database: str,
	mutation: str,
	restore: str,
	case: str,
	work: Path,
	env: dict[str, str],
	*,
	cluster_authority: bool = False,
	secret_sql: bool = False,
	mutation_probe: str | None = None,
) -> str:
	"""Prove one live authority drift and restore the shared fixture exactly."""
	try:
		return run_live_doctor_mutation(
			root,
			database,
			mutation,
			case,
			work,
			env,
			unsafe_authority=True,
			cluster_authority=cluster_authority,
			secret_sql=secret_sql,
			mutation_probe=mutation_probe,
		)
	finally:
		if cluster_authority:
			if secret_sql:
				psql_secret(database, restore, env)
			else:
				psql(database, restore, env)
		else:
			psql_as(MIGRATION_ROLE, database, restore, env)


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


def dump_schema_manifest(path: Path, env: dict[str, str]) -> str:
	manifest_env = env.copy()
	manifest_env["DECODEX_SCHEMA_MANIFEST_PATH"] = str(path)
	return run(
		["cargo", "nextest", "run", "-p", "decodex-postgres", "--features",
		 "test-support", "--test", "postgres_store", "--run-ignored", "all", "--",
		 "postgres_schema_manifest_dump_fixture", "--exact"],
		manifest_env,
	)


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
		f"decodex.leases, decodex.conversations, decodex.runtime_sessions, "
		f"decodex.artifacts, decodex.turns, decodex.history_items TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.quota_exclusions TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.profile_snapshots, "
		f"decodex.account_snapshots, decodex.blob_objects, decodex.artifact_revisions, decodex.context_packs, "
		f"decodex.context_pack_sources, decodex.transition_proposals TO {role}; "
		f"GRANT SELECT ON TABLE decodex.history_cursors, decodex.history_item_versions TO {role}; "
		f"GRANT SELECT ON TABLE decodex.projects, decodex.agents, "
		f"decodex.policies, decodex.policy_revisions TO {role}; "
		f"GRANT SELECT ON TABLE decodex.programs, decodex.objectives, "
		f"decodex.objective_completion_evidence TO {role}; "
		f"GRANT DELETE ON TABLE decodex.blob_objects TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.activity TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decodex.outbox TO {role}; "
		f"GRANT USAGE ON SEQUENCE decodex.activity_sequence_seq, decodex.outbox_id_seq TO {role}; "
		f"GRANT USAGE ON TYPE {type_names} TO {role}; "
		f"GRANT EXECUTE ON FUNCTION {execute_signatures} TO {role}; "
		f"REVOKE ALL ON FUNCTION {trigger_signatures} FROM {role}, PUBLIC",
		env,
	)


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


def main() -> int:
	work = Path(tempfile.mkdtemp(prefix="decodex-xy1267-")).resolve()
	data_dir = work / "postgres"
	socket_dir = work / "socket"
	log_path = work / "postgres.log"
	socket_dir.mkdir()
	# TCP is disabled; the port only distinguishes the socket filename inside this unique directory.
	port = 55_432
	role_setting_canary_guc = f"xy1272.canary_{secrets.token_hex(16)}"
	role_setting_secret_canary = secrets.token_hex(32)
	env = os.environ.copy()
	initdb_path = Path(shutil.which("initdb") or "initdb").resolve()
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
		roles = [
			MIGRATION_ROLE,
			RUNTIME_ROLE,
			MISSING_SELECT_ROLE,
			HOSTILE_SEARCH_ROLE,
			FUNCTION_OWNER_ROLE,
			SET_BYPASS_ROLE,
			SET_LEDGER_WRITE_ROLE,
			SET_SEQUENCE_UPDATE_ROLE,
			MEMBERSHIP_ADMIN_ROLE,
			*UNSAFE_ROLES.values(),
		]
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

		create_database(DATABASE, env)
		set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
		migration_output = run_migration(env)
		provision_runtime(DATABASE, RUNTIME_ROLE, env)
		v8_boundary_output = run_v8_migration_boundary_contracts(env, socket_dir, port)
		set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
		restart_output = run_blob_session_restart_contract(
			data_dir, log_path, socket_dir, port, work, env
		)
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
			raise TestFailure("valid runtime role is not a non-vacuous least-privilege fixture")
		contract_output = run(
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
		env["DECODEX_TEST_SOCKET_DIRECTORY"] = str(socket_dir)
		account_composition_output = run(
			[
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
			],
			env,
		)
		bootstrap_root = work / "decodex-root"
		write_bootstrap_config(
			bootstrap_root, socket_dir, port, DATABASE, MIGRATION_ROLE, RUNTIME_ROLE
		)
		env["DECODEX_TEST_BOOTSTRAP_ROOT"] = str(bootstrap_root)
		env["DECODEX_TEST_SOCKET_PORT"] = str(port)
		bootstrap_output = run(
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
		)
		live_doctor_output = run(
			[
				"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
				"bootstrap_doctor", "--run-ignored", "all", "--",
				"isolated_postgres_live_doctor_rejects_replaced_endpoint", "--exact",
			],
			env,
		)
		auth_bootstrap_root = work / "decodex-auth-root"
		write_bootstrap_config(
			auth_bootstrap_root,
			socket_dir,
			port,
			DATABASE,
			"decodex_xy1307_role_that_does_not_exist",
			RUNTIME_ROLE,
		)
		env["DECODEX_TEST_AUTH_BOOTSTRAP_ROOT"] = str(auth_bootstrap_root)
		auth_bootstrap_output = run(
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
				"isolated_postgres_rejected_role_is_authentication",
				"--exact",
			],
			env,
		)
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
		collation_output = run(
			[
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
			],
			env,
		)

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
			f"WHERE inventory_namespace.nspname = 'decodex') = 59",
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
		) != "8|1":
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

		live_manifest_path = work / "schema-manifest-live.json"
		set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
		dump_schema_manifest(live_manifest_path, env)
		live_quota_snapshot = quota_authority_snapshot(DATABASE, env)
		live_quota = json.loads(live_quota_snapshot)
		if len(live_quota["windows"]) != 2 or len(live_quota["exclusions"]) != 2:
			raise TestFailure("live quota snapshot is not the populated two-window fixture")
		if any(row["dispatch_enabled"] for row in live_quota["exclusions"]):
			raise TestFailure("live quota exclusion unexpectedly enables dispatch")
		dump_path = work / "decodex_xy1267.dump"
		run(["pg_dump", "-Fc", "-f", str(dump_path), DATABASE], env)
		create_database(RESTORE_DATABASE, env)
		run(["pg_restore", "--exit-on-error", "-d", RESTORE_DATABASE, str(dump_path)], env)
		set_contract_urls(env, socket_dir, port, RESTORE_DATABASE, RUNTIME_ROLE)
		restored_manifest_path = work / "schema-manifest-restored.json"
		dump_schema_manifest(restored_manifest_path, env)
		restored_quota_snapshot = quota_authority_snapshot(RESTORE_DATABASE, env)
		if restored_quota_snapshot != live_quota_snapshot:
			raise TestFailure("dump/restore changed immutable quota authority evidence")
		live_manifest = live_manifest_path.read_text()
		restored_manifest = restored_manifest_path.read_text()
		if live_manifest != restored_manifest:
			live_document = json.loads(live_manifest)
			restored_document = json.loads(restored_manifest)
			changed = [
				component for component in ("authority", "schema")
				if live_document[component] != restored_document[component]
			]
			component = changed[0]
			live_rows = json.loads(live_document[component])
			restored_rows = json.loads(restored_document[component])
			only_live = [row for row in live_rows if row not in restored_rows]
			only_restored = [row for row in restored_rows if row not in live_rows]
			raise TestFailure(
				"dump/restore schema manifest changed\n"
				f"component: {component}\n"
				f"only live: {json.dumps(only_live[:8], indent=2)}\n"
				f"only restored: {json.dumps(only_restored[:8], indent=2)}"
			)
		restore_output = run(
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
				"postgres_store_restored_contract",
				"--exact",
			],
			env,
		)
		canary_manifest_path = work / "schema-manifest-role-setting-canary.json"
		canary_markers = (role_setting_canary_guc, role_setting_secret_canary)
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
			dump_schema_manifest(canary_manifest_path, env)
			canary_manifest = canary_manifest_path.read_text(encoding="utf-8")
			if json.loads(canary_manifest)["authority"] == json.loads(live_manifest)["authority"]:
				raise TestFailure("secret-bearing role setting did not change configured authority")
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
		authority_digest = hashlib.sha256(
			json.loads(live_manifest)["authority"].encode("utf-8")
		).hexdigest()
		print(
			f"configured PostgreSQL authority manifest SHA-256: {authority_digest}",
			flush=True,
		)

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
		authority_drift_outputs = []
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
			output = run_authority_drift_case(
				bootstrap_root,
				DATABASE,
				mutation,
				restore,
				case,
				work,
				env,
				cluster_authority=cluster_authority,
				secret_sql=secret_sql,
				mutation_probe=mutation_probe,
			)
			if secret_sql and (
				role_setting_secret_canary in output or role_setting_canary_guc in output
			):
				raise TestFailure("doctor output leaked a role-setting canary")
			authority_drift_outputs.append(output)
		assert_postgres_logs_redact((log_path,), canary_markers)
		print(migration_output)
		print(restart_output)
		print(v8_boundary_output)
		print(contract_output)
		print(account_composition_output)
		print(bootstrap_output)
		print(live_doctor_output)
		print(auth_bootstrap_output)
		print(collation_output)
		print(unsafe_authority_output)
		print(ledger_live_doctor_output)
		print(missing_extension_live_doctor_output)
		print(identity_cast_output)
		print(incompatible_authority_output)
		print(hostile_startup_path_output)
		print(hostile_search_output)
		print(restore_output)
		print(default_acl_live_doctor_output)
		print(default_acl_restore_output)
		print("\n".join(authority_drift_outputs))
		return 0
	finally:
		stop_error: Exception | None = None
		stop_failures: list[str] = []
		status = postgres_status(data_dir, env) if data_dir.exists() else ClusterStatus.STOPPED
		if status is ClusterStatus.RUNNING:
			try:
				run(["pg_ctl", "-D", str(data_dir), "-m", "fast", "-w", "stop"], env)
			except Exception as error:
				stop_failures.append(f"fast shutdown failed:\n{error}")
			status = postgres_status(data_dir, env)
		if status is ClusterStatus.RUNNING:
			try:
				run(["pg_ctl", "-D", str(data_dir), "-m", "immediate", "-w", "stop"], env)
			except Exception as error:
				stop_failures.append(f"immediate shutdown failed:\n{error}")
			status = postgres_status(data_dir, env)
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
			shutil.rmtree(work, ignore_errors=True)
		elif stop_error is not None:
			raise stop_error


if __name__ == "__main__":
	raise SystemExit(main())
