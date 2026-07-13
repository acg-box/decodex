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


REPO_ROOT = Path(__file__).resolve().parents[2]
DATABASE = "decodex_xy1267"
COLLATION_DATABASE = "decodex_xy1267_tr"
RESTORE_DATABASE = "decodex_xy1267_restore"


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


def main() -> int:
	work = Path(tempfile.mkdtemp(prefix="decodex-xy1267-"))
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
		run(
			[
				"psql",
				"-X",
				"-qAt",
				"-v",
				"ON_ERROR_STOP=1",
				"-d",
				"postgres",
				"-c",
				f"CREATE DATABASE {DATABASE} WITH TEMPLATE template0",
			],
			env,
		)
		env["DECODEX_TEST_DATABASE_URL"] = (
			f"postgresql://{env['PGUSER']}@/{DATABASE}"
			f"?host={socket_dir.as_posix()}&port={port}"
		)
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
		run(
			[
				"createdb",
				"--maintenance-db=postgres",
				"--template=template0",
				"--encoding=UTF8",
				"--locale-provider=icu",
				"--icu-locale=tr-TR",
				COLLATION_DATABASE,
			],
			env,
		)
		env["DECODEX_TEST_COLLATION_DATABASE_URL"] = (
			f"postgresql://{env['PGUSER']}@/{COLLATION_DATABASE}"
			f"?host={socket_dir.as_posix()}&port={port}"
		)
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
		dump_path = work / "decodex_xy1267.dump"
		run(["pg_dump", "-Fc", "-f", str(dump_path), DATABASE], env)
		run(
			[
				"psql",
				"-X",
				"-qAt",
				"-v",
				"ON_ERROR_STOP=1",
				"-d",
				"postgres",
				"-c",
				f"CREATE DATABASE {RESTORE_DATABASE} WITH TEMPLATE template0",
			],
			env,
		)
		run(["pg_restore", "--exit-on-error", "-d", RESTORE_DATABASE, str(dump_path)], env)
		env["DECODEX_TEST_DATABASE_URL"] = (
			f"postgresql://{env['PGUSER']}@/{RESTORE_DATABASE}"
			f"?host={socket_dir.as_posix()}&port={port}"
		)
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
		print(contract_output)
		print(collation_output)
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
