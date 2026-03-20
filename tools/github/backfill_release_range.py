#!/usr/bin/env python3
"""Backfill unpublished GitHub signals for a selected stable->preview compare range."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from build_change_bundle import build_pr_bundle, routed_token_env  # noqa: E402
from contracts import dump_json, load_json, validate_release_delta, validate_signal  # noqa: E402
from sync_latest_signals import run_script  # noqa: E402

PR_URL_RE = re.compile(r"/pull/(\d+)$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="openai/codex", help="GitHub repository in owner/name format.")
    parser.add_argument("--release-delta", default="site/src/content/release-deltas/openai-codex-latest.json")
    parser.add_argument("--stable-tag", required=True, help="Stable tag name to backfill from.")
    parser.add_argument("--preview-tag", help="Preview tag name to backfill to. Defaults to the top-level prerelease.")
    parser.add_argument("--signals-dir", default="site/src/content/signals")
    parser.add_argument("--bundles-dir", default="tools/github/bundles")
    parser.add_argument("--analysis-dir", default="tools/github/analysis")
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    parser.add_argument("--max-prs", type=int, help="Optional limit for debugging or partial runs.")
    parser.add_argument("--dry-run", action="store_true", help="Print target PRs without generating new content.")
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


def load_selected_comparison(path: Path, stable_tag: str, preview_tag: str | None) -> tuple[dict[str, Any], str]:
    payload = load_json(path)
    validation = validate_release_delta(payload)
    if not validation.ok:
        raise SystemExit("Release-delta validation failed:\n- " + "\n- ".join(validation.errors))

    target_preview = preview_tag or payload["prerelease"]["tag_name"]
    for item in payload.get("comparisons", []):
        if item["stable_tag_name"] == stable_tag and item["prerelease_tag_name"] == target_preview:
            return item, target_preview
    raise SystemExit(f"No comparison found for {stable_tag} -> {target_preview}")


def pr_lookup(repo: str, pr_number: int, token: str | None) -> dict[str, Any]:
    import urllib.request
    import urllib.error

    request = urllib.request.Request(
        f"https://api.github.com/repos/{repo}/pulls/{pr_number}",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}" if token else "",
            "User-Agent": "decodex-release-range-backfill",
        },
    )
    if not token:
        request.headers.pop("Authorization")
    try:
        with urllib.request.urlopen(request) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"GitHub API request failed for PR #{pr_number}: {exc.code} {details}") from exc


def signal_paths(pr_number: int, args: argparse.Namespace) -> tuple[Path, Path, Path]:
    stem = f"openai-codex-pr-{pr_number}"
    return (
        Path(args.bundles_dir) / f"{stem}.json",
        Path(args.analysis_dir) / f"{stem}.analysis.json",
        Path(args.signals_dir) / f"{stem}.json",
    )


def main() -> None:
    args = parse_args()
    root = repo_root()
    release_delta_path = (root / args.release_delta).resolve()
    comparison, preview_tag = load_selected_comparison(release_delta_path, args.stable_tag, args.preview_tag)
    token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
    token = os.environ.get(token_env)

    signals_dir = (root / args.signals_dir).resolve()
    published = published_pr_numbers(signals_dir)
    target_prs = [int(number) for number in comparison["compare"].get("pr_numbers", []) if int(number) not in published]

    pr_details: list[dict[str, Any]] = []
    for pr_number in target_prs:
        details = pr_lookup(args.repo, pr_number, token)
        pr_details.append(
            {
                "number": pr_number,
                "title": details.get("title") or f"PR #{pr_number}",
                "merged_at": details.get("merged_at") or "",
                "url": details.get("html_url") or "",
            }
        )
    pr_details.sort(key=lambda item: item["merged_at"])
    if args.max_prs is not None:
        pr_details = pr_details[: args.max_prs]

    if args.dry_run:
        print(
            json.dumps(
                {
                    "stable_tag": args.stable_tag,
                    "preview_tag": preview_tag,
                    "target_pr_count": len(pr_details),
                    "target_prs": pr_details,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return

    created = 0
    for pr in pr_details:
        bundle_path, analysis_path, signal_path = signal_paths(pr["number"], args)
        bundle = build_pr_bundle(
            args.repo,
            pr["number"],
            token,
            [f"Backfilled from compare range {args.stable_tag}...{preview_tag}"],
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
    run_script(
        "build_release_delta.py",
        "--repo",
        args.repo,
        "--signals-dir",
        args.signals_dir,
        "--out",
        "site/src/content/release-deltas/openai-codex-latest.json",
    )
    print(
        json.dumps(
            {
                "stable_tag": args.stable_tag,
                "preview_tag": preview_tag,
                "created": created,
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
