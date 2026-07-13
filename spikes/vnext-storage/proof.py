#!/usr/bin/env python3
"""Orchestrate the isolated PostgreSQL backup/crash/restore proof for XY-1264."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import uuid


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1264"
RESTORE_DATABASE = "decodex_xy1264_restore"
ROLLBACK_DATABASE = "decodex_xy1264_rollback"


class ProofFailure(RuntimeError):
	"""Raised when a command or acceptance assertion fails."""


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
		raise ProofFailure(
			f"command failed ({completed.returncode}): {' '.join(command)}\n"
			f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
		)
	return completed.stdout.strip()


def psql(env: dict[str, str], database: str, sql: str) -> str:
	return run(
		["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-d", database, "-c", sql],
		env,
	)


def assert_equal(actual: object, expected: object, label: str) -> None:
	if actual != expected:
		raise ProofFailure(f"{label}: expected {expected!r}, found {actual!r}")


def free_port() -> int:
	with socket.socket() as listener:
		listener.bind(("127.0.0.1", 0))
		return int(listener.getsockname()[1])


def start_cluster(
	env: dict[str, str], data_dir: Path, log_path: Path, socket_dir: Path, port: int
) -> None:
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


def stop_cluster(env: dict[str, str], data_dir: Path, mode: str) -> None:
	run(["pg_ctl", "-D", str(data_dir), "-m", mode, "-w", "stop"], env)


def create_database(env: dict[str, str], name: str) -> None:
	psql(env, "postgres", f"CREATE DATABASE {name} WITH TEMPLATE template0")


def prove_crash_recovery(env: dict[str, str]) -> tuple[int, str]:
	psql(
		env,
		DATABASE,
		"INSERT INTO decodex.outbox (aggregate_key, payload) "
		"VALUES ('crash-recovery', '{\"kind\": \"crash\"}')",
	)
	worker = str(uuid.uuid4())
	claimed = psql(
		env,
		DATABASE,
		"UPDATE decodex.outbox SET state = 'in_flight', attempt_count = 1, "
		f"lease_holder = '{worker}', lease_expires_at = clock_timestamp() + interval '1 millisecond' "
		"WHERE aggregate_key = 'crash-recovery' RETURNING id",
	)
	return int(claimed), worker


def recover_after_restart(env: dict[str, str], crash_id: int) -> dict[str, object]:
	worker = str(uuid.uuid4())
	recovered = psql(
		env,
		DATABASE,
		"SELECT id || '|' || attempt_count || '|' || last_error "
		f"FROM decodex.claim_outbox('{worker}', 2000, interval '30 seconds') "
		f"WHERE id = {crash_id}",
	).split("|")
	assert_equal(recovered[0], str(crash_id), "crash-recovered outbox id")
	assert_equal(recovered[1], "2", "crash-recovered attempt count")
	assert_equal(recovered[2], "recovered_expired_claim", "crash-recovery reason")
	assert_equal(
		psql(env, DATABASE, f"SELECT decodex.complete_outbox({crash_id}, '{worker}')"),
		"t",
		"crash-recovered completion",
	)
	return {
		"immediate_shutdown": True,
		"restart": True,
		"recovered_id": crash_id,
		"attempt_count": 2,
		"side_effect_policy": (
			"at-least-once delivery with receipt/readback reconciliation; "
			"exactly-once is not claimed"
		),
	}


def backup_restore_rollback(
	env: dict[str, str], work: Path, pre_migration_dump: Path
) -> dict[str, object]:
	post_migration_dump = work / "post-migration.dump"
	run(["pg_dump", "-Fc", "-d", DATABASE, "-f", str(post_migration_dump)], env)
	create_database(env, RESTORE_DATABASE)
	run(
		[
			"pg_restore",
			"--exit-on-error",
			"--single-transaction",
			"--no-owner",
			"-d",
			RESTORE_DATABASE,
			str(post_migration_dump),
		],
		env,
	)
	assert_equal(
		psql(env, RESTORE_DATABASE, "SELECT count(*) FROM refinery_schema_history"),
		"1",
		"restored migration count",
	)
	for table in ("probe_entities", "outbox", "artifacts"):
		assert_equal(
			psql(env, RESTORE_DATABASE, f"SELECT count(*) FROM decodex.{table}"),
			psql(env, DATABASE, f"SELECT count(*) FROM decodex.{table}"),
			f"restored {table} count",
		)
	create_database(env, ROLLBACK_DATABASE)
	run(
		[
			"pg_restore",
			"--exit-on-error",
			"--single-transaction",
			"--no-owner",
			"-d",
			ROLLBACK_DATABASE,
			str(pre_migration_dump),
		],
		env,
	)
	assert_equal(
		psql(env, ROLLBACK_DATABASE, "SELECT to_regnamespace('decodex') IS NULL"),
		"t",
		"pre-migration rollback restore",
	)
	psql(env, DATABASE, "VACUUM (ANALYZE) decodex.outbox")
	psql(env, DATABASE, "REINDEX TABLE decodex.outbox")
	assert_equal(
		psql(
			env,
			DATABASE,
			"SELECT last_analyze IS NOT NULL FROM pg_stat_user_tables "
			"WHERE schemaname = 'decodex' AND relname = 'outbox'",
		),
		"t",
		"operator maintenance analyze readback",
	)
	return {
		"custom_dump_bytes": post_migration_dump.stat().st_size,
		"restore_database": RESTORE_DATABASE,
		"rollback_database": ROLLBACK_DATABASE,
		"restore_verified": True,
		"pre_migration_rollback_verified": True,
		"maintenance": [
			"VACUUM (ANALYZE) decodex.outbox",
			"REINDEX TABLE decodex.outbox",
		],
	}


def main() -> int:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--keep", action="store_true", help="keep the isolated cluster")
	parser.add_argument("--json-output", type=Path, help="write the measurement receipt")
	args = parser.parse_args()
	work = Path(tempfile.mkdtemp(prefix="decodex-xy1264-"))
	data_dir = work / "postgres"
	socket_dir = work / "socket"
	log_path = work / "postgres.log"
	socket_dir.mkdir()
	port = free_port()
	env = os.environ.copy()
	env.update(
		{
			"PGHOST": str(socket_dir),
			"PGPORT": str(port),
			"PGUSER": os.environ.get("USER", "postgres"),
		}
	)
	started = False
	try:
		initdb_path = Path(shutil.which("initdb") or "initdb").resolve()
		postgres_share = initdb_path.parent.parent / "share" / "postgresql"
		env["PATH"] = f"{initdb_path.parent}{os.pathsep}{env['PATH']}"
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
		start_cluster(env, data_dir, log_path, socket_dir, port)
		started = True
		create_database(env, DATABASE)
		pre_migration_dump = work / "pre-migration.dump"
		run(["pg_dump", "-Fc", "-d", DATABASE, "-f", str(pre_migration_dump)], env)
		env.update(
			{
				"DATABASE_URL": (
					f"postgresql://{env['PGUSER']}@/{DATABASE}"
					f"?host={socket_dir.as_posix()}&port={port}"
				),
				"DECODEX_PROOF_ROOT": str(work / "local-state"),
			}
		)
		rust_receipt = json.loads(
			run(
				[
					"cargo",
					"run",
					"--quiet",
					"-p",
					"decodex-vnext-storage-proof",
				],
				env,
			)
		)
		crash_id, _ = prove_crash_recovery(env)
		stop_cluster(env, data_dir, "immediate")
		started = False
		start_cluster(env, data_dir, log_path, socket_dir, port)
		started = True
		time.sleep(0.02)
		receipt = {
			"schema": "decodex/vnext-storage-feasibility/1",
			"source_revision": run(["git", "rev-parse", "HEAD"], env),
			"proof_source_sha256": hashlib.sha256(
				(REPO_ROOT / "spikes/vnext-storage/src/main.rs").read_bytes()
			).hexdigest(),
			"migration_sha256": hashlib.sha256(
				(REPO_ROOT / "spikes/vnext-storage/migrations/V1__bootstrap.sql").read_bytes()
			).hexdigest(),
			"host_macos": run(["sw_vers", "-productVersion"], env),
			"cluster": {
				"isolated": True,
				"tcp_disabled": True,
				"temporary_root": str(work),
				"postgres_share": str(postgres_share),
			},
			"runtime_proof": rust_receipt,
			"crash_recovery": recover_after_restart(env, crash_id),
			"backup_restore_rollback": backup_restore_rollback(
				env, work, pre_migration_dump
			),
			"cleanup": {
				"databases_are_inside_temporary_cluster": True,
				"automatic_cluster_removal": not args.keep,
			},
		}
		encoded = json.dumps(receipt, indent=2, sort_keys=True)
		print(encoded)
		if args.json_output:
			args.json_output.write_text(encoded + "\n", encoding="utf-8")
		return 0
	finally:
		if started:
			stop_cluster(env, data_dir, "fast")
		if args.keep:
			print(f"kept isolated proof root: {work}")
		else:
			shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
	raise SystemExit(main())
