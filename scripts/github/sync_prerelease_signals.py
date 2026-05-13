#!/usr/bin/env python3
"""Sync unpublished Decodex signals from the latest Codex prerelease compare."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

SCRIPT_HOME = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="openai/codex", help="GitHub repository in owner/name format.")
    parser.add_argument("--release-delta", default="site/src/content/release-deltas/openai-codex-latest.json")
    parser.add_argument("--stable-tag", help="Stable tag to compare from. Defaults to the latest stable release.")
    parser.add_argument("--preview-tag", help="Prerelease tag to compare to. Defaults to the latest prerelease.")
    parser.add_argument("--signals-dir", default="site/src/content/signals")
    parser.add_argument("--bundles-dir", default="artifacts/github/bundles")
    parser.add_argument("--analysis-dir", default="artifacts/github/analysis")
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    parser.add_argument("--max-prs", type=int, help="Optional limit for debugging or partial runs.")
    parser.add_argument("--dry-run", action="store_true", help="Print target PRs without generating new content.")
    return parser.parse_args()


def append_optional(command: list[str], flag: str, value: str | int | None) -> None:
    if value is None:
        return
    command.extend([flag, str(value)])


def main() -> None:
    args = parse_args()
    command = [
        sys.executable,
        str(SCRIPT_HOME / "backfill_release_range.py"),
        "--repo",
        args.repo,
        "--release-delta",
        args.release_delta,
        "--signals-dir",
        args.signals_dir,
        "--bundles-dir",
        args.bundles_dir,
        "--analysis-dir",
        args.analysis_dir,
        "--codex-bin",
        args.codex_bin,
        "--refresh-release-delta-first",
        "--refresh-stable-limit",
        "1",
        "--refresh-preview-limit",
        "1",
        "--refresh-pair-limit",
        "1",
    ]
    append_optional(command, "--stable-tag", args.stable_tag)
    append_optional(command, "--preview-tag", args.preview_tag)
    append_optional(command, "--token-env", args.token_env)
    append_optional(command, "--model", args.model)
    append_optional(command, "--max-prs", args.max_prs)
    if args.dry_run:
        command.append("--dry-run")

    completed = subprocess.run(command, check=False)
    raise SystemExit(completed.returncode)


if __name__ == "__main__":
    main()
