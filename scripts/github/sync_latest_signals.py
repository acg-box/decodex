#!/usr/bin/env python3
"""Discover recent upstream commits, generate Decodex signals, and refresh release deltas."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.parse
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from build_change_bundle import (  # noqa: E402
    build_commit_bundle,
    build_pr_bundle,
    github_request,
    maybe_promote_commit_to_pr,
    repo_default_branch,
    routed_token_env,
)
from contracts import dump_json, load_json, validate_signal  # noqa: E402
from radar_ledger import (  # noqa: E402
    DEFAULT_LEDGER_PATH,
    connect as connect_ledger,
    ingest_artifact_set,
    record_commit,
    record_review,
)

COMMIT_URL_RE = re.compile(r"/commit/([0-9a-f]{7,40})$")
PR_URL_RE = re.compile(r"/pull/(\d+)$")
POSITIVE_TITLE_TERMS = (
    "feat",
    "feature",
    "add",
    "support",
    "change",
    "plugin",
    "image",
    "agent",
    "tui",
    "config",
    "flag",
    "preview",
    "history",
    "sync",
    "menu",
    "startup",
    "remote",
    "save",
    "behavior",
    "command",
    "model",
)
NEGATIVE_TITLE_TERMS = (
    "bump ",
    "deps",
    "dependency",
    "bazel",
    "ci",
    "lint",
    "typo",
    "docs",
    "refactor",
    "cleanup",
    "chore",
    "test ",
    "tests ",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="openai/codex", help="GitHub repository in owner/name format.")
    parser.add_argument("--signals-dir", default="site/src/content/signals", help="Published signal directory.")
    parser.add_argument("--bundles-dir", default="artifacts/github/bundles", help="Bundle directory.")
    parser.add_argument("--analysis-dir", default="artifacts/github/analysis", help="Analysis draft directory.")
    parser.add_argument(
        "--release-delta-out",
        default="site/src/content/release-deltas/openai-codex-latest.json",
        help="Path to write the release delta artifact.",
    )
    parser.add_argument("--search-limit", type=int, default=20, help="How many recent commits to inspect.")
    parser.add_argument("--max-new-prs", type=int, default=3, help="Maximum unpublished changes to publish per run.")
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    parser.add_argument(
        "--ledger",
        default=DEFAULT_LEDGER_PATH,
        help="Local SQLite Radar ledger path. Defaults to .decodex/radar.sqlite3.",
    )
    parser.add_argument("--no-ledger", action="store_true", help="Disable local Radar ledger writes.")
    parser.add_argument(
        "--refresh-release-delta",
        action="store_true",
        help="Refresh the release-delta artifact even when no new signal entry was created.",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return SCRIPT_HOME.parents[1]


def published_pr_numbers(signals_dir: Path) -> set[int]:
    published: set[int] = set()
    for path in sorted(signals_dir.glob("*.json")):
        payload = load_json(path)
        validation = validate_signal(payload)
        if not validation.ok:
            raise SystemExit(f"Signal validation failed for {path}:\n- " + "\n- ".join(validation.errors))
        pr_url = payload.get("source_refs", {}).get("pr_url")
        if not isinstance(pr_url, str):
            continue
        match = PR_URL_RE.search(pr_url)
        if match:
            published.add(int(match.group(1)))
    return published


def published_commit_shas(signals_dir: Path) -> set[str]:
    published: set[str] = set()
    for path in sorted(signals_dir.glob("*.json")):
        payload = load_json(path)
        validation = validate_signal(payload)
        if not validation.ok:
            raise SystemExit(f"Signal validation failed for {path}:\n- " + "\n- ".join(validation.errors))
        for url in payload.get("source_refs", {}).get("commit_urls", []):
            if not isinstance(url, str):
                continue
            match = COMMIT_URL_RE.search(url)
            if match:
                published.add(match.group(1))
    return published


def recent_commits(repo: str, token: str | None, search_limit: int) -> list[dict[str, Any]]:
    default_branch = repo_default_branch(repo, token)
    payload, _ = github_request(
        f"https://api.github.com/repos/{repo}/commits?sha={urllib.parse.quote(default_branch)}&per_page={search_limit}",
        token,
    )
    if not isinstance(payload, list):
        raise SystemExit("Expected commits list payload from GitHub API")
    results: list[dict[str, Any]] = []
    for item in payload:
        sha = item.get("sha")
        commit = item.get("commit")
        url = item.get("html_url")
        if not isinstance(sha, str) or not isinstance(commit, dict) or not isinstance(url, str):
            continue
        message = commit.get("message")
        if not isinstance(message, str) or not message:
            continue
        results.append(
            {
                "sha": sha,
                "title": message.strip().splitlines()[0],
                "url": url,
                "committed_at": (commit.get("committer") or {}).get("date"),
            }
        )
    return results


def candidate_score(title: str) -> int:
    lowered = title.lower()
    score = 0
    for term in POSITIVE_TITLE_TERMS:
        if term in lowered:
            score += 2
    for term in NEGATIVE_TITLE_TERMS:
        if term in lowered:
            score -= 3
    return score


def signal_paths(candidate: dict[str, Any], args: argparse.Namespace) -> tuple[Path, Path, Path]:
    pr_number = candidate.get("pr_number")
    stem = (
        f"openai-codex-pr-{pr_number}"
        if isinstance(pr_number, int)
        else f"openai-codex-commit-{candidate['sha'][:12]}"
    )
    bundles_dir = Path(args.bundles_dir)
    analysis_dir = Path(args.analysis_dir)
    signals_dir = Path(args.signals_dir)
    return (
        bundles_dir / f"{stem}.json",
        analysis_dir / f"{stem}.analysis.json",
        signals_dir / f"{stem}.json",
    )


def run_script(script: str, *extra: str) -> None:
    cmd = [sys.executable, str(SCRIPT_HOME / script), *extra]
    completed = subprocess.run(cmd, check=False, text=True, capture_output=True, cwd=repo_root())
    if completed.returncode != 0:
        stderr = completed.stderr.strip()
        stdout = completed.stdout.strip()
        details = stderr or stdout or "unknown error"
        raise SystemExit(f"{script} failed: {details}")


def refresh_release_delta(args: argparse.Namespace) -> None:
    run_script(
        "build_release_delta.py",
        "--repo",
        args.repo,
        "--signals-dir",
        args.signals_dir,
        "--out",
        args.release_delta_out,
    )


def main() -> None:
    args = parse_args()
    token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
    token = os.environ.get(token_env)
    root = repo_root()
    ledger_path = None if args.no_ledger else Path(args.ledger)
    if ledger_path is not None and not ledger_path.is_absolute():
        ledger_path = root / ledger_path
    ledger = connect_ledger(ledger_path) if ledger_path is not None else None
    signals_dir = (root / args.signals_dir).resolve()
    published_prs = published_pr_numbers(signals_dir)
    published_shas = published_commit_shas(signals_dir)
    commits = recent_commits(args.repo, token, args.search_limit)
    candidates: list[dict[str, Any]] = []
    seen_candidate_keys: set[tuple[str, int | str]] = set()
    try:
        for commit in commits:
            pr_number = maybe_promote_commit_to_pr(args.repo, commit["sha"], token)
            subject_kind = "pr" if pr_number is not None else "commit"
            subject_id = str(pr_number) if pr_number is not None else commit["sha"]
            if ledger is not None:
                record_commit(
                    ledger,
                    repo=args.repo,
                    sha=commit["sha"],
                    title=commit["title"],
                    url=commit["url"],
                    committed_at=commit.get("committed_at"),
                    pr_number=pr_number,
                )
            if commit["sha"] in published_shas or (pr_number is not None and pr_number in published_prs):
                if ledger is not None:
                    record_review(
                        ledger,
                        repo=args.repo,
                        subject_kind=subject_kind,
                        subject_id=subject_id,
                        status="signal",
                        reason="Already present in published signal collection.",
                        confidence="confirmed",
                    )
                continue
            candidate_key: tuple[str, int | str] = (
                ("pr", pr_number) if pr_number is not None else ("commit", commit["sha"])
            )
            if candidate_key in seen_candidate_keys:
                if ledger is not None:
                    record_review(
                        ledger,
                        repo=args.repo,
                        subject_kind=subject_kind,
                        subject_id=subject_id,
                        status="seen",
                        reason="Duplicate recent commit for an already considered PR.",
                    )
                continue
            seen_candidate_keys.add(candidate_key)
            score = candidate_score(commit["title"])
            if score <= 0:
                if ledger is not None:
                    record_review(
                        ledger,
                        repo=args.repo,
                        subject_kind=subject_kind,
                        subject_id=subject_id,
                        status="skipped",
                        reason=f"Recent commit title scored {score}; no public signal was generated.",
                        confidence="likely",
                    )
                continue
            candidates.append({**commit, "pr_number": pr_number, "score": score})

        unpublished = candidates[: args.max_new_prs]
        for candidate in candidates[args.max_new_prs :]:
            if ledger is None:
                continue
            pr_number = candidate.get("pr_number")
            subject_kind = "pr" if isinstance(pr_number, int) else "commit"
            subject_id = str(pr_number) if isinstance(pr_number, int) else candidate["sha"]
            record_review(
                ledger,
                repo=args.repo,
                subject_kind=subject_kind,
                subject_id=subject_id,
                status="watch",
                reason="Positive Radar candidate left for a later sync budget.",
                confidence="likely",
            )

        created = 0
        for candidate in reversed(unpublished):
            bundle_path, analysis_path, signal_path = signal_paths(candidate, args)
            notes = [f"Discovered via continuous upstream commit sync: {candidate['url']}"]
            pr_number = candidate.get("pr_number")
            bundle = (
                build_pr_bundle(args.repo, pr_number, token, notes)
                if isinstance(pr_number, int)
                else build_commit_bundle(args.repo, candidate["sha"], token, notes)
            )
            dump_json(root / bundle_path, bundle)

            run_script(
                "run_codex_analysis.py",
                "--bundle",
                str(root / bundle_path),
                "--out",
                str(root / analysis_path),
                "--repo-root",
                str(root),
                "--codex-bin",
                args.codex_bin,
                *(["--model", args.model] if args.model else []),
            )

            run_script(
                "render_signal_entry.py",
                "--bundle",
                str(root / bundle_path),
                "--analysis",
                str(root / analysis_path),
                "--out",
                str(root / signal_path),
            )
            if ledger is not None:
                ingest_artifact_set(
                    ledger,
                    bundle_path=root / bundle_path,
                    analysis_path=root / analysis_path,
                    signal_path=root / signal_path,
                )
            created += 1
        if ledger is not None:
            ledger.commit()
    finally:
        if ledger is not None:
            ledger.close()

    run_script("validate_signal_entry.py", str(root / args.signals_dir))
    release_delta_refreshed = (
        created > 0 or args.refresh_release_delta or not (root / args.release_delta_out).exists()
    )
    if release_delta_refreshed:
        refresh_release_delta(args)
    print(
        json.dumps(
            {
                "repo": args.repo,
                "published_prs_seen": len(published_prs),
                "published_commits_seen": len(published_shas),
                "recent_commits_scanned": len(commits),
                "unpublished_changes_considered": len(candidates),
                "new_signals_created": created,
                "release_delta_refreshed": release_delta_refreshed,
                "ledger": str(ledger_path) if ledger_path is not None else None,
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
