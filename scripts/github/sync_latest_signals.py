#!/usr/bin/env python3
"""Discover recent merged PRs, generate Decodex signals, and refresh release deltas."""

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

from build_change_bundle import build_pr_bundle, github_request, repo_default_branch, routed_token_env  # noqa: E402
from contracts import dump_json, load_json, validate_signal  # noqa: E402

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
    parser.add_argument("--search-limit", type=int, default=20, help="How many recent merged PRs to inspect.")
    parser.add_argument("--max-new-prs", type=int, default=3, help="Maximum unpublished PRs to publish per run.")
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
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


def recent_merged_prs(repo: str, token: str | None, search_limit: int) -> list[dict[str, Any]]:
    default_branch = repo_default_branch(repo, token)
    query = urllib.parse.quote_plus(f"repo:{repo} is:pr is:merged base:{default_branch}")
    payload, _ = github_request(
        f"https://api.github.com/search/issues?q={query}&sort=updated&order=desc&per_page={search_limit}",
        token,
    )
    items = payload.get("items")
    if not isinstance(items, list):
        raise SystemExit("Expected search/issues to return an items list")
    results: list[dict[str, Any]] = []
    for item in items:
        number = item.get("number")
        title = item.get("title")
        url = item.get("html_url")
        if isinstance(number, int) and isinstance(title, str) and isinstance(url, str):
            results.append({"number": number, "title": title, "url": url})
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


def signal_paths(pr_number: int, args: argparse.Namespace) -> tuple[Path, Path, Path]:
    stem = f"openai-codex-pr-{pr_number}"
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
    signals_dir = (root / args.signals_dir).resolve()
    published = published_pr_numbers(signals_dir)
    candidates = recent_merged_prs(args.repo, token, args.search_limit)
    unpublished = [item for item in candidates if item["number"] not in published]
    unpublished = [item for item in unpublished if candidate_score(item["title"]) > 0][: args.max_new_prs]

    created = 0
    for candidate in reversed(unpublished):
        bundle_path, analysis_path, signal_path = signal_paths(candidate["number"], args)
        bundle = build_pr_bundle(
            args.repo,
            candidate["number"],
            token,
            [f"Discovered via hourly merged-PR sync: {candidate['url']}"],
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
        created += 1

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
                "published_prs_seen": len(published),
                "recent_prs_scanned": len(candidates),
                "new_signals_created": created,
                "release_delta_refreshed": release_delta_refreshed,
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
