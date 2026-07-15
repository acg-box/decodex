#!/usr/bin/env python3
"""Run XY-1267 integration tests in a disposable PostgreSQL 18 cluster."""

from __future__ import annotations

from enum import Enum
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1267"
COLLATION_DATABASE = "decodex_xy1267_tr"
RESTORE_DATABASE = "decodex_xy1267_restore"
AUTHORITY_DATABASE = "decodex_xy1307_authority"
TRIGGER_DATABASE = "decodex_xy1307_trigger_contract"
FUNCTION_DATABASE = "decodex_xy1307_function_contract"
PRIVILEGED_FUNCTION_DATABASE = "decodex_xy1307_privileged_function"
TRIGGER_ESCAPE_DATABASE = "decodex_xy1307_trigger_escape"
EXTENSION_CONTROL_DATABASE = "decodex_xy1307_extension_control"
HOSTILE_SEARCH_DATABASE = "decodex_xy1307_hostile_search"
CONSTRAINT_DRIFT_DATABASE = "decodex_xy1307_constraint_drift"
EXTERNAL_CASCADE_DATABASE = "decodex_xy1307_external_cascade"
LEDGER_TAMPER_DATABASE = "decodex_xy1307_ledger_tamper"
MISSING_EXTENSION_DATABASE = "decodex_xy1307_missing_extension"
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
) -> str:
	"""Coordinate a real daemon query around an adapter-owned database mutation."""
	sync = work / f"live-doctor-{case}"
	sync.mkdir()
	test_env = env.copy()
	test_env["DECODEX_TEST_LIVE_INCOMPATIBLE_ROOT"] = str(root)
	test_env["DECODEX_TEST_LIVE_INCOMPATIBLE_SYNC"] = str(sync)
	command = [
		"cargo", "nextest", "run", "-p", "decodex-runtime", "--test",
		"bootstrap_doctor", "--run-ignored", "all", "--",
		"isolated_postgres_live_doctor_detects_database_incompatibility", "--exact",
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
			raise TestFailure(
				f"live doctor exited before {case} mutation\nstdout:\n{stdout}\nstderr:\n{stderr}"
			)
		if time.monotonic() >= deadline:
			process.kill()
			stdout, stderr = process.communicate()
			raise TestFailure(
				f"live doctor did not reach {case} barrier\nstdout:\n{stdout}\nstderr:\n{stderr}"
			)
		time.sleep(0.01)

	psql_as(MIGRATION_ROLE, database, sql, env)
	(sync / "mutated").write_text("mutated", encoding="utf-8")
	try:
		stdout, stderr = process.communicate(timeout=30)
	except subprocess.TimeoutExpired as error:
		process.kill()
		stdout, stderr = process.communicate()
		raise TestFailure(
			f"live doctor did not finish {case}\nstdout:\n{stdout}\nstderr:\n{stderr}"
		) from error
	if process.returncode != 0:
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


def psql_as(role: str, database: str, sql: str, env: dict[str, str]) -> str:
	role_env = env.copy()
	role_env["PGUSER"] = role
	return psql(database, sql, role_env)


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
		f"CREATE DATABASE {database} WITH TEMPLATE template0 ENCODING 'UTF8'{locale_clause}",
		env,
	)
	psql(database, f"GRANT USAGE, CREATE ON SCHEMA public TO {MIGRATION_ROLE}", env)
	psql(
		"postgres",
		f"REVOKE CREATE ON DATABASE {database} FROM PUBLIC; "
		f"GRANT CONNECT, CREATE ON DATABASE {database} TO {MIGRATION_ROLE}",
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


def provision_runtime(database: str, role: str, env: dict[str, str]) -> None:
	psql(
		database,
		f"GRANT CONNECT ON DATABASE {database} TO {role}; "
		f"GRANT USAGE ON SCHEMA public, decodex TO {role}; "
		f"GRANT SELECT ON TABLE public.refinery_schema_history TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE ON TABLE "
		f"decodex.accounts, decodex.quota_windows, decodex.command_receipts, "
		f"decodex.leases, decodex.conversations, decodex.runtime_sessions, "
		f"decodex.artifacts, decodex.turns, decodex.history_items TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.profile_snapshots, "
		f"decodex.account_snapshots, decodex.blob_objects, decodex.artifact_revisions, decodex.context_packs, "
		f"decodex.context_pack_sources, decodex.transition_proposals TO {role}; "
		f"GRANT SELECT ON TABLE decodex.history_cursors, decodex.history_item_versions TO {role}; "
		f"GRANT DELETE ON TABLE decodex.blob_objects TO {role}; "
		f"GRANT SELECT, INSERT ON TABLE decodex.activity TO {role}; "
		f"GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decodex.outbox TO {role}; "
		f"GRANT USAGE ON SEQUENCE decodex.activity_sequence_seq, decodex.outbox_id_seq TO {role}; "
		f"GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA decodex TO {role}; "
		f"REVOKE ALL ON FUNCTION decodex.capture_history_item_version() FROM {role}",
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
			if role == UNSAFE_ROLES["bypassrls"]:
				attributes += " BYPASSRLS"
			elif role == UNSAFE_ROLES["superuser"]:
				attributes += " SUPERUSER"
			psql("postgres", f"CREATE ROLE {role}{attributes}", env)

		create_database(DATABASE, env)
		set_contract_urls(env, socket_dir, port, DATABASE, RUNTIME_ROLE)
		migration_output = run_migration(env)
		provision_runtime(DATABASE, RUNTIME_ROLE, env)
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
			f"WHERE inventory_namespace.nspname = 'decodex') = 35",
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

		missing_select_root = work / "decodex-incompatible-missing-history-select"
		write_bootstrap_config(
			missing_select_root,
			socket_dir,
			port,
			AUTHORITY_DATABASE,
			MIGRATION_ROLE,
			MISSING_SELECT_ROLE,
		)
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
		) != "4|1":
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
				str(missing_select_root),
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
				"isolated_postgres_hostile_search_path_is_available", "--exact",
			],
			env,
		)

		dump_path = work / "decodex_xy1267.dump"
		run(["pg_dump", "-Fc", "-f", str(dump_path), DATABASE], env)
		create_database(RESTORE_DATABASE, env)
		run(["pg_restore", "--exit-on-error", "-d", RESTORE_DATABASE, str(dump_path)], env)
		set_contract_urls(env, socket_dir, port, RESTORE_DATABASE, RUNTIME_ROLE)
		restore_output = run(
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
				"postgres_store_restored_contract",
				"--exact",
			],
			env,
		)
		print(migration_output)
		print(restart_output)
		print(contract_output)
		print(account_composition_output)
		print(bootstrap_output)
		print(live_doctor_output)
		print(auth_bootstrap_output)
		print(collation_output)
		print(unsafe_authority_output)
		print(ledger_live_doctor_output)
		print(missing_extension_live_doctor_output)
		print(incompatible_authority_output)
		print(hostile_search_output)
		print(restore_output)
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
