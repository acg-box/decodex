#!/usr/bin/env python3
"""Backfill unpublished GitHub signals for a stable->preview prerelease compare range."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from build_change_bundle import build_pr_bundle, github_request, routed_token_env  # noqa: E402
from contracts import dump_json, load_json, validate_release_delta, validate_signal  # noqa: E402
from sync_latest_signals import run_script  # noqa: E402

PR_URL_RE = re.compile(r"/pull/(\d+)$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="openai/codex", help="GitHub repository in owner/name format.")
    parser.add_argument("--release-delta", default="site/src/content/release-deltas/openai-codex-latest.json")
    parser.add_argument(
        "--stable-tag",
        help="Stable tag name to backfill from. Defaults to the top-level stable release.",
    )
    parser.add_argument("--preview-tag", help="Preview tag name to backfill to. Defaults to the top-level prerelease.")
    parser.add_argument("--signals-dir", default="site/src/content/signals")
    parser.add_argument("--bundles-dir", default="artifacts/github/bundles")
    parser.add_argument("--analysis-dir", default="artifacts/github/analysis")
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    parser.add_argument("--max-prs", type=int, help="Optional limit for debugging or partial runs.")
    parser.add_argument("--dry-run", action="store_true", help="Print target PRs without generating new content.")
    parser.add_argument(
        "--refresh-release-delta-first",
        action="store_true",
        help="Refresh the release-delta artifact before selecting the prerelease compare range.",
    )
    parser.add_argument(
        "--refresh-stable-limit",
        type=int,
        help="Stable release limit used only by --refresh-release-delta-first.",
    )
    parser.add_argument(
        "--refresh-preview-limit",
        type=int,
        help="Prerelease limit used only by --refresh-release-delta-first.",
    )
    parser.add_argument(
        "--refresh-pair-limit",
        type=int,
        help="Compare pair limit used only by --refresh-release-delta-first.",
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


def load_selected_comparison(
    path: Path,
    stable_tag: str | None,
    preview_tag: str | None,
) -> tuple[dict[str, Any], str, str]:
    payload = load_json(path)
    validation = validate_release_delta(payload)
    if not validation.ok:
        raise SystemExit("Release-delta validation failed:\n- " + "\n- ".join(validation.errors))

    target_stable = stable_tag or payload["stable_release"]["tag_name"]
    target_preview = preview_tag or payload["prerelease"]["tag_name"]
    for item in payload.get("comparisons", []):
        if item["stable_tag_name"] == target_stable and item["prerelease_tag_name"] == target_preview:
            return item, target_stable, target_preview
    raise SystemExit(f"No comparison found for {target_stable} -> {target_preview}")


def pr_lookup(repo: str, pr_number: int, token: str | None) -> dict[str, Any]:
    payload, _headers = github_request(f"https://api.github.com/repos/{repo}/pulls/{pr_number}", token)

    if not isinstance(payload, dict):
        raise SystemExit(f"Expected pull request object from GitHub for PR #{pr_number}")

    return payload


def signal_paths(pr_number: int, args: argparse.Namespace) -> tuple[Path, Path, Path]:
    stem = f"openai-codex-pr-{pr_number}"
    return (
        Path(args.bundles_dir) / f"{stem}.json",
        Path(args.analysis_dir) / f"{stem}.analysis.json",
        Path(args.signals_dir) / f"{stem}.json",
    )


def refresh_release_delta(args: argparse.Namespace) -> None:
    command = [
        "build_release_delta.py",
        "--repo",
        args.repo,
        "--signals-dir",
        args.signals_dir,
        "--out",
        args.release_delta,
    ]
    if args.token_env:
        command.extend(["--token-env", args.token_env])
    if args.refresh_stable_limit is not None:
        command.extend(["--stable-limit", str(args.refresh_stable_limit)])
    if args.refresh_preview_limit is not None:
        command.extend(["--preview-limit", str(args.refresh_preview_limit)])
    if args.refresh_pair_limit is not None:
        command.extend(["--pair-limit", str(args.refresh_pair_limit)])
    run_script(*command)


def prepare_release_delta_path(args: argparse.Namespace, root: Path) -> tuple[Path, tempfile.TemporaryDirectory[str] | None]:
    if not args.refresh_release_delta_first:
        return (root / args.release_delta).resolve(), None

    tmpdir = tempfile.TemporaryDirectory(prefix="decodex-prerelease-delta-")
    temp_release_delta = Path(tmpdir.name) / "release-delta.json"
    refresh_args = argparse.Namespace(**{**vars(args), "release_delta": str(temp_release_delta)})
    refresh_release_delta(refresh_args)
    return temp_release_delta.resolve(), tmpdir


def main() -> None:
    args = parse_args()
    root = repo_root()
    release_delta_path, tmpdir = prepare_release_delta_path(args, root)
    try:
        comparison, stable_tag, preview_tag = load_selected_comparison(
            release_delta_path,
            args.stable_tag,
            args.preview_tag,
        )
        token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
        token = os.environ.get(token_env)

        signals_dir = (root / args.signals_dir).resolve()
        published = published_pr_numbers(signals_dir)
        target_prs = [
            int(number)
            for number in comparison["compare"].get("pr_numbers", [])
            if int(number) not in published
        ]
        if args.max_prs is not None:
            target_prs = target_prs[: args.max_prs]

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

        if args.dry_run:
            print(
                json.dumps(
                    {
                        "stable_tag": stable_tag,
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
                [f"Backfilled from prerelease compare range {stable_tag}...{preview_tag}"],
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
            args.release_delta,
            *(["--token-env", args.token_env] if args.token_env else []),
        )
        print(
            json.dumps(
                {
                    "stable_tag": stable_tag,
                    "preview_tag": preview_tag,
                    "created": created,
                },
                sort_keys=True,
            )
        )
    finally:
        if tmpdir is not None:
            tmpdir.cleanup()


if __name__ == "__main__":
    main()
