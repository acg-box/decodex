#!/usr/bin/env python3
"""Maintain the local Decodex Radar SQLite ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sqlite3
import sys
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import load_json, utc_now_iso, validate_bundle, validate_signal  # noqa: E402

SCHEMA_VERSION = 2
DEFAULT_LEDGER_PATH = ".decodex/radar.sqlite3"
COMMIT_URL_RE = re.compile(r"/commit/([0-9a-f]{7,40})$")
PR_URL_RE = re.compile(r"/pull/(\d+)$")
SUBJECT_KINDS = {"commit", "pr"}
REVIEW_STATUSES = {
    "seen",
    "skipped",
    "watch",
    "signal",
    "control_plane",
    "social",
    "deprecated",
    "archived",
}
CONFIDENCE_VALUES = {"confirmed", "likely", "weak"}
ARTIFACT_KINDS = {
    "bundle",
    "analysis",
    "signal",
    "upstream_impact",
    "social_post",
    "release_delta",
    "archive_manifest",
    "ledger_export",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", default=DEFAULT_LEDGER_PATH, help="SQLite ledger path.")
    subcommands = parser.add_subparsers(dest="command", required=True)

    subcommands.add_parser("init", help="Initialize the ledger schema.")

    ingest = subcommands.add_parser("ingest", help="Ingest one bundle and optional derived artifacts.")
    ingest.add_argument("--bundle", required=True, help="Path to a github_change_bundle/v1 JSON file.")
    ingest.add_argument("--analysis", help="Optional analysis draft path.")
    ingest.add_argument("--signal", help="Optional rendered signal_entry/v1 path.")

    ingest_existing = subcommands.add_parser(
        "ingest-existing",
        help="Ingest existing checked-in bundles, analyses, and signals.",
    )
    ingest_existing.add_argument("--bundles-dir", default="artifacts/github/bundles")
    ingest_existing.add_argument("--analysis-dir", default="artifacts/github/analysis")
    ingest_existing.add_argument("--signals-dir", default="site/src/content/signals")

    summary = subcommands.add_parser("summary", help="Print ledger counts.")
    summary.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")

    return parser.parse_args()


def connect(path: str | Path) -> sqlite3.Connection:
    db_path = Path(path)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(db_path)
    connection.row_factory = sqlite3.Row
    initialize(connection)
    return connection


def initialize(connection: sqlite3.Connection) -> None:
    connection.executescript(
        """
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS metadata (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS upstream_commit (
          repo TEXT NOT NULL,
          sha TEXT NOT NULL,
          title TEXT NOT NULL,
          url TEXT NOT NULL,
          committed_at TEXT,
          pr_number INTEGER,
          first_seen_at TEXT NOT NULL,
          last_seen_at TEXT NOT NULL,
          PRIMARY KEY (repo, sha)
        );

        CREATE TABLE IF NOT EXISTS radar_review (
          repo TEXT NOT NULL,
          subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
          subject_id TEXT NOT NULL,
          status TEXT NOT NULL CHECK (
            status IN (
              'seen',
              'skipped',
              'watch',
              'signal',
              'control_plane',
              'social',
              'deprecated',
              'archived'
            )
          ),
          reason TEXT NOT NULL DEFAULT '',
          confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
          reviewed_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          PRIMARY KEY (repo, subject_kind, subject_id)
        );

        CREATE TABLE IF NOT EXISTS artifact_link (
          repo TEXT NOT NULL,
          subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
          subject_id TEXT NOT NULL,
          artifact_kind TEXT NOT NULL CHECK (
            artifact_kind IN (
              'bundle',
              'analysis',
              'signal',
              'upstream_impact',
              'social_post',
              'release_delta',
              'archive_manifest',
              'ledger_export'
            )
          ),
          path TEXT NOT NULL,
          sha256 TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
        );

        CREATE TABLE IF NOT EXISTS source_cache (
          url TEXT PRIMARY KEY,
          etag TEXT,
          body_sha256 TEXT NOT NULL,
          fetched_at TEXT NOT NULL,
          cache_path TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_upstream_commit_pr
          ON upstream_commit (repo, pr_number);

        CREATE INDEX IF NOT EXISTS idx_radar_review_status
          ON radar_review (status, reviewed_at);
        """
    )
    migrate_artifact_link_social_kind(connection)
    connection.execute(
        """
        INSERT INTO metadata (key, value)
        VALUES ('schema_version', ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        """,
        (str(SCHEMA_VERSION),),
    )
    connection.commit()


def migrate_artifact_link_social_kind(connection: sqlite3.Connection) -> None:
    row = connection.execute(
        """
        SELECT sql
        FROM sqlite_master
        WHERE type = 'table' AND name = 'artifact_link'
        """
    ).fetchone()
    if not row or "social_draft" not in row["sql"]:
        return

    connection.executescript(
        """
        ALTER TABLE artifact_link RENAME TO artifact_link_old;

        CREATE TABLE artifact_link (
          repo TEXT NOT NULL,
          subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
          subject_id TEXT NOT NULL,
          artifact_kind TEXT NOT NULL CHECK (
            artifact_kind IN (
              'bundle',
              'analysis',
              'signal',
              'upstream_impact',
              'social_post',
              'release_delta',
              'archive_manifest',
              'ledger_export'
            )
          ),
          path TEXT NOT NULL,
          sha256 TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
        );

        INSERT OR REPLACE INTO artifact_link (
          repo,
          subject_kind,
          subject_id,
          artifact_kind,
          path,
          sha256,
          size_bytes,
          created_at
        )
        SELECT
          repo,
          subject_kind,
          subject_id,
          CASE artifact_kind
            WHEN 'social_draft' THEN 'social_post'
            ELSE artifact_kind
          END,
          path,
          sha256,
          size_bytes,
          created_at
        FROM artifact_link_old;

        DROP TABLE artifact_link_old;
        """
    )


def path_for_storage(path: str | Path) -> str:
    resolved = Path(path).resolve()
    cwd = Path.cwd().resolve()
    try:
        return str(resolved.relative_to(cwd))
    except ValueError:
        return str(resolved)


def file_digest(path: str | Path) -> tuple[str, int]:
    payload = Path(path).read_bytes()
    return hashlib.sha256(payload).hexdigest(), len(payload)


def require_member(value: str, allowed: set[str], label: str) -> None:
    if value not in allowed:
        raise ValueError(f"{label} must be one of {sorted(allowed)}")


def record_commit(
    connection: sqlite3.Connection,
    *,
    repo: str,
    sha: str,
    title: str,
    url: str,
    committed_at: str | None = None,
    pr_number: int | None = None,
    seen_at: str | None = None,
) -> None:
    timestamp = seen_at or utc_now_iso()
    connection.execute(
        """
        INSERT INTO upstream_commit (
          repo,
          sha,
          title,
          url,
          committed_at,
          pr_number,
          first_seen_at,
          last_seen_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(repo, sha) DO UPDATE SET
          title = excluded.title,
          url = excluded.url,
          committed_at = COALESCE(excluded.committed_at, upstream_commit.committed_at),
          pr_number = COALESCE(excluded.pr_number, upstream_commit.pr_number),
          last_seen_at = excluded.last_seen_at
        """,
        (repo, sha, title, url, committed_at, pr_number, timestamp, timestamp),
    )


def record_review(
    connection: sqlite3.Connection,
    *,
    repo: str,
    subject_kind: str,
    subject_id: str,
    status: str,
    reason: str,
    confidence: str | None = None,
    reviewed_at: str | None = None,
) -> None:
    require_member(subject_kind, SUBJECT_KINDS, "subject_kind")
    require_member(status, REVIEW_STATUSES, "status")
    if confidence is not None:
        require_member(confidence, CONFIDENCE_VALUES, "confidence")
    timestamp = reviewed_at or utc_now_iso()
    connection.execute(
        """
        INSERT INTO radar_review (
          repo,
          subject_kind,
          subject_id,
          status,
          reason,
          confidence,
          reviewed_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(repo, subject_kind, subject_id) DO UPDATE SET
          status = excluded.status,
          reason = excluded.reason,
          confidence = excluded.confidence,
          reviewed_at = excluded.reviewed_at,
          updated_at = excluded.updated_at
        """,
        (repo, subject_kind, subject_id, status, reason, confidence, timestamp, timestamp),
    )


def record_artifact(
    connection: sqlite3.Connection,
    *,
    repo: str,
    subject_kind: str,
    subject_id: str,
    artifact_kind: str,
    path: str | Path,
    created_at: str | None = None,
) -> None:
    require_member(subject_kind, SUBJECT_KINDS, "subject_kind")
    require_member(artifact_kind, ARTIFACT_KINDS, "artifact_kind")
    digest, size_bytes = file_digest(path)
    connection.execute(
        """
        INSERT INTO artifact_link (
          repo,
          subject_kind,
          subject_id,
          artifact_kind,
          path,
          sha256,
          size_bytes,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(repo, subject_kind, subject_id, artifact_kind, path) DO UPDATE SET
          sha256 = excluded.sha256,
          size_bytes = excluded.size_bytes,
          created_at = excluded.created_at
        """,
        (
            repo,
            subject_kind,
            subject_id,
            artifact_kind,
            path_for_storage(path),
            digest,
            size_bytes,
            created_at or utc_now_iso(),
        ),
    )


def subject_for_bundle(bundle: dict[str, Any]) -> tuple[str, str, str]:
    repo = bundle["repo"]
    primary_pr = bundle.get("primary_pr")
    if isinstance(primary_pr, dict) and isinstance(primary_pr.get("number"), int):
        return repo, "pr", str(primary_pr["number"])
    first_commit = bundle["commits"][0]
    return repo, "commit", first_commit["sha"]


def record_bundle(
    connection: sqlite3.Connection,
    bundle: dict[str, Any],
    bundle_path: str | Path,
    *,
    status: str = "watch",
    reason: str = "Imported from GitHub change bundle.",
) -> tuple[str, str, str]:
    validation = validate_bundle(bundle)
    if not validation.ok:
        raise ValueError("Bundle validation failed:\n- " + "\n- ".join(validation.errors))

    repo, subject_kind, subject_id = subject_for_bundle(bundle)
    primary_pr = bundle.get("primary_pr")
    pr_number = primary_pr.get("number") if isinstance(primary_pr, dict) else None
    for commit in bundle["commits"]:
        record_commit(
            connection,
            repo=repo,
            sha=commit["sha"],
            title=commit["message"],
            url=commit["url"],
            committed_at=commit.get("committed_at"),
            pr_number=pr_number if isinstance(pr_number, int) else None,
        )
    record_review(
        connection,
        repo=repo,
        subject_kind=subject_kind,
        subject_id=subject_id,
        status=status,
        reason=reason,
        confidence="confirmed" if status == "signal" else None,
    )
    record_artifact(
        connection,
        repo=repo,
        subject_kind=subject_kind,
        subject_id=subject_id,
        artifact_kind="bundle",
        path=bundle_path,
    )
    return repo, subject_kind, subject_id


def subject_refs_for_signal(signal: dict[str, Any]) -> list[tuple[str, str, str]]:
    refs = signal.get("source_refs", {})
    repo = refs.get("repo")
    if not isinstance(repo, str):
        return []
    subjects: list[tuple[str, str, str]] = []
    pr_url = refs.get("pr_url")
    if isinstance(pr_url, str):
        match = PR_URL_RE.search(pr_url)
        if match:
            subjects.append((repo, "pr", match.group(1)))
    for url in refs.get("commit_urls", []):
        if not isinstance(url, str):
            continue
        match = COMMIT_URL_RE.search(url)
        if match:
            subjects.append((repo, "commit", match.group(1)))
    return subjects


def record_signal_artifact(connection: sqlite3.Connection, signal_path: str | Path) -> list[tuple[str, str, str]]:
    signal = load_json(signal_path)
    validation = validate_signal(signal)
    if not validation.ok:
        raise ValueError(f"Signal validation failed for {signal_path}:\n- " + "\n- ".join(validation.errors))

    subjects = subject_refs_for_signal(signal)
    for repo, subject_kind, subject_id in subjects:
        record_review(
            connection,
            repo=repo,
            subject_kind=subject_kind,
            subject_id=subject_id,
            status="signal",
            reason=f"Published signal_entry/v1: {signal['slug']}",
            confidence=signal["confidence"],
        )
        record_artifact(
            connection,
            repo=repo,
            subject_kind=subject_kind,
            subject_id=subject_id,
            artifact_kind="signal",
            path=signal_path,
        )
    return subjects


def ingest_artifact_set(
    connection: sqlite3.Connection,
    *,
    bundle_path: str | Path,
    analysis_path: str | Path | None = None,
    signal_path: str | Path | None = None,
) -> None:
    bundle = load_json(bundle_path)
    signal_exists = signal_path is not None and Path(signal_path).exists()
    repo, subject_kind, subject_id = record_bundle(
        connection,
        bundle,
        bundle_path,
        status="signal" if signal_exists else "watch",
        reason="Imported from generated Radar artifacts.",
    )
    if analysis_path is not None and Path(analysis_path).exists():
        record_artifact(
            connection,
            repo=repo,
            subject_kind=subject_kind,
            subject_id=subject_id,
            artifact_kind="analysis",
            path=analysis_path,
        )
    if signal_exists:
        signal_subjects = record_signal_artifact(connection, signal_path)
        if (repo, subject_kind, subject_id) not in signal_subjects:
            record_artifact(
                connection,
                repo=repo,
                subject_kind=subject_kind,
                subject_id=subject_id,
                artifact_kind="signal",
                path=signal_path,
            )


def ingest_existing(
    connection: sqlite3.Connection,
    *,
    bundles_dir: str | Path,
    analysis_dir: str | Path,
    signals_dir: str | Path,
) -> dict[str, int]:
    bundles_path = Path(bundles_dir)
    analysis_path = Path(analysis_dir)
    signals_path = Path(signals_dir)
    ingested = 0
    for bundle_path in sorted(bundles_path.glob("*.json")):
        stem = bundle_path.stem
        candidate_analysis = analysis_path / f"{stem}.analysis.json"
        candidate_signal = signals_path / f"{stem}.json"
        ingest_artifact_set(
            connection,
            bundle_path=bundle_path,
            analysis_path=candidate_analysis if candidate_analysis.exists() else None,
            signal_path=candidate_signal if candidate_signal.exists() else None,
        )
        ingested += 1

    linked_signal_paths = {signals_path / f"{path.stem}.json" for path in bundles_path.glob("*.json")}
    for signal_path in sorted(signals_path.glob("*.json")):
        if signal_path in linked_signal_paths:
            continue
        record_signal_artifact(connection, signal_path)

    connection.commit()
    return {**summary_counts(connection), "bundles_ingested": ingested}


def summary_counts(connection: sqlite3.Connection) -> dict[str, int]:
    tables = {
        "upstream_commits": "upstream_commit",
        "radar_reviews": "radar_review",
        "artifact_links": "artifact_link",
        "source_cache_entries": "source_cache",
    }
    result: dict[str, int] = {}
    for key, table in tables.items():
        row = connection.execute(f"SELECT COUNT(*) AS count FROM {table}").fetchone()
        result[key] = int(row["count"])
    return result


def print_summary(connection: sqlite3.Connection, *, as_json: bool) -> None:
    payload = summary_counts(connection)
    if as_json:
        print(json.dumps(payload, indent=2, sort_keys=True))
        return
    for key, value in payload.items():
        print(f"{key}\t{value}")


def main() -> None:
    args = parse_args()
    connection = connect(args.db)
    try:
        if args.command == "init":
            print(args.db)
        elif args.command == "ingest":
            ingest_artifact_set(
                connection,
                bundle_path=args.bundle,
                analysis_path=args.analysis,
                signal_path=args.signal,
            )
            connection.commit()
            print_summary(connection, as_json=True)
        elif args.command == "ingest-existing":
            payload = ingest_existing(
                connection,
                bundles_dir=args.bundles_dir,
                analysis_dir=args.analysis_dir,
                signals_dir=args.signals_dir,
            )
            print(json.dumps(payload, indent=2, sort_keys=True))
        elif args.command == "summary":
            print_summary(connection, as_json=args.json)
        else:
            raise SystemExit(f"unknown command: {args.command}")
    finally:
        connection.close()


if __name__ == "__main__":
    main()
