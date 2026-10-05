#!/usr/bin/env python3
"""Verify the bundled SQLite product database and its active ownership graph."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess
import sys
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]
MIGRATIONS = (
    (48, "agent_baseline", ROOT / "database/migrations/0048_agent_baseline.sql"),
    (49, "native_settings_and_model_source", ROOT / "database/migrations/0049_native_settings_and_model_source.sql"),
    (50, "unique_account_names", ROOT / "database/migrations/0050_unique_account_names.sql"),
    (51, "nullable_reset_credit_expiry", ROOT / "database/migrations/0051_nullable_reset_credit_expiry.sql"),
    (52, "personal_access_token_credentials", ROOT / "database/migrations/0052_personal_access_token_credentials.sql"),
    (53, "independent_workspaces", ROOT / "database/migrations/0053_independent_workspaces.sql"),
)

DATABASE_RELATIVE_PATH = Path("server/decodex.sqlite3")
APPLICATION_ID = 0x4443_5831
MIGRATION_DIGEST_DOMAIN = b"decodex-sqlite-migration-v1\0"
COMMAND_TIMEOUT_SECONDS = 10 * 60
OUTPUT_LIMIT_BYTES = 64 * 1024
REQUIRED_TABLES = frozenset(
    {
        "account_usage_observations",
        "agent_async_skips",
        "agent_prompt_input_chunks",
        "agent_prompt_inputs",
        "agent_request_payloads",
        "schema_migrations",
        "reset_card_operations",
        "account_identities",
        "account_operations",
        "accounts",
        "account_credentials",
        "local_account_transfers",
        "account_routing_control",
        "account_routing_order",
        "account_quota_facts",
        "account_profile_snapshots",
        "account_profile_daily_usage",
        "codex_account_capability",
        "command_receipts",
        "legacy_account_route_interruptions",
        "role_profiles",
        "conversations",
        "quick_task_requests",
        "routing_decisions",
        "continuation_plans",
        "runtime_sessions",
        "turns",
        "history_items",
        "runtime_command_receipts",
        "conversation_routing_successors",
        "conversation_native_settings",
        "process_execution_epochs",
        "process_generations",
        "process_generation_death_evidence",
        "provider_attempts",
        "provider_attempt_positive_evidence",
        "context_packs",
        "programs",
        "program_entities",
        "program_signals",
        "program_claims",
        "program_proposals",
        "program_objectives",
        "program_work_items",
        "program_work_item_executions",
        "program_evidence",
        "program_reviews",
        "program_domain_pack_bindings",
        "desktop_settings",
        "account_quota_activation",
        "agent_work_items",
        "agent_dependencies",
        "agent_inbox_events",
        "agent_process_bindings",
        "agent_root_settings",
        "agent_capacity_retries",
        "agent_live_output",
        "agent_managers",
        "workspaces",
        "work_workspace",
        "agent_tool_versions",
        "agent_thread_revisions",
        "agent_usage",
        "agent_voice_calls",
        "agent_voice_observed_turns",
        "agent_async_questions",
        "agent_async_answers",
        "agent_async_recovery",
        "agent_misalignment",
        "agent_guardian_reviews",
        "agent_guardian_approvals",

    }
)


class GateFailure(RuntimeError):
    """One bounded, secret-negative local database gate failure."""


def migration_digest(source: bytes) -> str:
    """Return the digest format stored by the Rust migration owner."""
    digest = hashlib.sha256()
    digest.update(MIGRATION_DIGEST_DOMAIN)
    digest.update(source)
    return digest.hexdigest()


def read_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def validate_repository_contract() -> None:
    """Verify that only the one-shot transfer tool retains redb."""
    workspace = read_toml(ROOT / "Cargo.toml")
    members = set(workspace["workspace"]["members"])
    dependencies = workspace["workspace"]["dependencies"]
    if "database" not in members or "database/transfer" not in members:
        raise GateFailure("database workspace owners are missing")

    rusqlite = dependencies.get("rusqlite")
    if not isinstance(rusqlite, dict) or "bundled" not in rusqlite.get("features", []):
        raise GateFailure("SQLite is not bundled")

    runtime = read_toml(ROOT / "crates/decodex-runtime/Cargo.toml")["dependencies"]
    if "decodex-database" not in runtime:
        raise GateFailure("runtime does not own the SQLite adapter")
    if "redb" in runtime:
        raise GateFailure("runtime retains a retired storage dependency")

    transfer = read_toml(ROOT / "database/transfer/Cargo.toml")["dependencies"]
    if "redb" not in transfer or "decodex-database" not in transfer:
        raise GateFailure("one-shot transfer dependencies are incomplete")

    service = (ROOT / "apps/decodex-cli/src/lib.rs").read_text(encoding="utf-8")
    if "InitializeLocalDatabase" not in service or "ValidateLocalDatabase" not in service:
        raise GateFailure("service local database commands are missing")
    for retired in ("SuperviseLocal", "BootstrapLatestSchema", "ValidateCurrentAuthority"):
        if retired in service:
            raise GateFailure(f"retired service command remains active: {retired}")


def run_checked(command: list[str]) -> None:
    """Run one bounded command without forwarding unbounded diagnostics."""
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env={**os.environ, "RUST_BACKTRACE": "0"},
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=COMMAND_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise GateFailure("local database command did not complete") from error
    if len(completed.stdout) > OUTPUT_LIMIT_BYTES or len(completed.stderr) > OUTPUT_LIMIT_BYTES:
        raise GateFailure("local database command output exceeded its bound")
    if completed.returncode != 0:
        diagnostic = completed.stderr[-4_096:].decode("utf-8", errors="replace")
        raise GateFailure(f"local database command failed: {diagnostic}")


def inspect_database(path: Path) -> dict[str, object]:
    """Read only credential-negative schema and integrity facts."""
    if not path.is_file() or stat.S_IMODE(path.stat().st_mode) != 0o600:
        raise GateFailure("SQLite database is missing or not owner-private")
    expected_migrations = [
        (version, name, migration_digest(path.read_bytes()))
        for version, name, path in MIGRATIONS
    ]
    try:
        connection = sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
        application_id = connection.execute("PRAGMA application_id").fetchone()[0]
        user_version = connection.execute("PRAGMA user_version").fetchone()[0]
        journal_mode = connection.execute("PRAGMA journal_mode").fetchone()[0]
        quick_check = connection.execute("PRAGMA quick_check(1)").fetchone()[0]
        foreign_key_violations = connection.execute("PRAGMA foreign_key_check").fetchall()
        migrations = connection.execute(
            "SELECT version, name, sha256 FROM schema_migrations ORDER BY version"
        ).fetchall()
        task_profile = connection.execute(
            "SELECT revision, instructions FROM role_profiles WHERE role = 'task'"
        ).fetchone()
        conversation_request_columns = {
            row[1]: (row[2], row[3], row[4])
            for row in connection.execute("PRAGMA table_info(quick_task_requests)")
        }
        program_signal_columns = {
            row[1]: (row[2], row[3], row[4])
            for row in connection.execute("PRAGMA table_info(program_signals)")
        }
        program_pack_binding_columns = {
            row[1]: (row[2], row[3], row[4])
            for row in connection.execute(
                "PRAGMA table_info(program_domain_pack_bindings)"
            )
        }
        tables = frozenset(
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_schema "
                "WHERE type = 'table' AND name NOT LIKE 'sqlite_%'"
            )
        )
    except sqlite3.Error as error:
        raise GateFailure("SQLite read-only verification failed") from error
    finally:
        if "connection" in locals():
            connection.close()

    if application_id != APPLICATION_ID or user_version != MIGRATIONS[-1][0]:
        raise GateFailure("SQLite application or schema version differs")
    if str(journal_mode).lower() != "wal" or quick_check != "ok" or foreign_key_violations:
        raise GateFailure("SQLite integrity or journal configuration differs")
    if migrations != expected_migrations:
        raise GateFailure("SQLite migration ledger differs")
    if task_profile is None or task_profile[0] != 2 or not task_profile[1]:
        raise GateFailure("Task RoleProfile does not satisfy the executable contract")
    for column in ("model", "reasoning_effort", "fast"):
        if column not in conversation_request_columns:
            raise GateFailure("Conversation execution settings are missing")
    if "predecessor_review_id" not in program_signal_columns:
        raise GateFailure("Program continuation lineage is missing")
    for column in ("program_id", "pack_id", "pack_version", "pack_digest"):
        if column not in program_pack_binding_columns:
            raise GateFailure("Program Domain Pack binding is incomplete")
    if tables != REQUIRED_TABLES:
        raise GateFailure("SQLite table inventory differs")
    return {
        "application_id": application_id,
        "journal_mode": str(journal_mode).lower(),
        "migration_sha256": [migration[2] for migration in expected_migrations],
        "schema_version": user_version,
        "table_count": len(tables),
        "conversation_execution_columns": sorted(
            column
            for column in ("model", "reasoning_effort", "fast")
            if column in conversation_request_columns
        ),
        "program_continuation_lineage": "predecessor_review_id",
        "program_domain_pack_binding": "immutable",
    }


def run_gate() -> dict[str, object]:
    """Initialize twice, validate, and inspect one fresh fixed-path database."""
    validate_repository_contract()
    with tempfile.TemporaryDirectory(prefix="decodex-sqlite-gate-") as temporary:
        root = Path(temporary).resolve()
        initialize = [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "decodex-cli",
            "--bin",
            "decodex",
            "--",
            "initialize-local-database",
            "--root",
            str(root),
        ]
        run_checked(initialize)
        run_checked(initialize)
        run_checked(
            [
                "cargo",
                "run",
                "--quiet",
                "-p",
                "decodex-cli",
                "--bin",
                "decodex",
                "--",
                "validate-local-database",
                "--root",
                str(root),
            ]
        )
        database = inspect_database(root / DATABASE_RELATIVE_PATH)
    return {"gate": "local_database", "outcome": "passed", "database": database}


def main() -> int:
    try:
        result = run_gate()
    except GateFailure as error:
        print(json.dumps({"gate": "local_database", "outcome": "failed", "error": str(error)}))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
