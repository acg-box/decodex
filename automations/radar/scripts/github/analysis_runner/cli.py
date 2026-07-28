"""Command-line orchestration for local Radar Codex analysis."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from analysis_runner.command import run_codex_analysis
from analysis_runner.paths import SCRIPT_HOME, repo_root_from

if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import load_json, validate_analysis_draft, validate_bundle  # noqa: E402


ALLOW_ANALYSIS_ENV = "DECODEX_ALLOW_CODEX_ANALYSIS"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allow-ai-analysis-boundary",
        action="store_true",
        help=(
            "Required acknowledgement that this helper is crossing the Codex AI analysis "
            "boundary. GitHub Actions must not set this."
        ),
    )
    parser.add_argument("--bundle", required=True, help="Path to github_change_bundle/v1 JSON.")
    parser.add_argument("--repo-root", help="Repository root for codex exec. Defaults to the current repo root.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    return parser.parse_args()


def analysis_boundary_allowed(args: argparse.Namespace) -> bool:
    return args.allow_ai_analysis_boundary or os.environ.get(ALLOW_ANALYSIS_ENV) == "1"


def main() -> None:
    args = parse_args()
    if not analysis_boundary_allowed(args):
        raise SystemExit(
            "Codex analysis helper requires --allow-ai-analysis-boundary or "
            f"{ALLOW_ANALYSIS_ENV}=1. Use Rust-owned radar commands for "
            "deterministic Radar workflows; GitHub Actions must not run this helper."
        )

    bundle_path = Path(args.bundle)
    if ".agent/automations/radar/cache" in bundle_path.as_posix():
        raise SystemExit("Python analysis helper must not read the private Radar cache directly")
    bundle = load_json(bundle_path)
    bundle_validation = validate_bundle(bundle)
    if not bundle_validation.ok:
        raise SystemExit("Bundle validation failed:\n- " + "\n- ".join(bundle_validation.errors))

    repo_root = Path(args.repo_root).resolve() if args.repo_root else repo_root_from(bundle_path)
    payload = run_codex_analysis(args, bundle_path, repo_root)
    validation = validate_analysis_draft(payload)
    if not validation.ok:
        raise SystemExit("Analysis draft validation failed:\n- " + "\n- ".join(validation.errors))

    print(json.dumps(payload, indent=2, sort_keys=True))
