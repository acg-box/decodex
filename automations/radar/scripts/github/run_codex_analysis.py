#!/usr/bin/env python3
"""Run Codex against a GitHub bundle and persist a validated analysis draft."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import dump_json, load_json, validate_analysis_draft, validate_bundle  # noqa: E402

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
    parser.add_argument("--out", required=True, help="Path to write the validated analysis JSON.")
    parser.add_argument("--repo-root", help="Repository root for codex exec. Defaults to the current repo root.")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable to invoke.")
    parser.add_argument("--model", help="Optional Codex model override.")
    return parser.parse_args()


def analysis_boundary_allowed(args: argparse.Namespace) -> bool:
    return args.allow_ai_analysis_boundary or os.environ.get(ALLOW_ANALYSIS_ENV) == "1"


def repo_root_from(bundle_path: Path) -> Path:
    resolved = bundle_path.resolve()
    for root in resolved.parents:
        if (
            root / "automations" / "radar" / "skills" / "github-signal" / "SKILL.md"
        ).exists():
            return root
    raise SystemExit(f"Unable to resolve repo root from {bundle_path}")


def build_prompt(bundle_path: Path, repo_root: Path) -> str:
    relative_bundle = bundle_path.resolve().relative_to(repo_root)
    return "\n".join(
        [
            "Read and follow these repo-local instructions before drafting:",
            "- automations/radar/skills/README.md",
            "- automations/radar/skills/codex-code-analysis/SKILL.md",
            "- automations/radar/skills/github-signal/SKILL.md",
            "- docs/spec/github-change-bundle.md",
            "- docs/spec/signal-entry.md",
            "- docs/runbook/local-github-signal-workflow.md",
            "",
            f"Analyze the bundle at `{relative_bundle}`.",
            "",
            "Return exactly one JSON object matching the provided output schema.",
            "Use the code-analysis skill as the in-session behavior-reading pass.",
            "Do not invent a separate checked-in code-analysis artifact.",
            "Treat the pull request as the main narrative container and the commits/files as evidence.",
            "Do not summarize every commit independently.",
            "Keep the output publishable for Decodex: concise, user-facing, and evidence-backed.",
            "Include every schema field. Use null when an optional string field does not apply, and use [] when no config flags apply.",
            "If `how_to_try` is not null, `expected_effect` must also be non-null.",
            "Use `impact=low` rather than overstating significance when the change is mostly incremental.",
        ]
    )


def extract_json_payload(raw: str) -> dict[str, Any]:
    candidate = raw.strip()
    if candidate.startswith("```"):
        parts = candidate.split("```")
        if len(parts) >= 3:
            candidate = parts[1]
            if candidate.startswith("json"):
                candidate = candidate[4:]
            candidate = candidate.strip()
    try:
        payload = json.loads(candidate)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"Codex output was not valid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise SystemExit("Codex output must decode to a JSON object")
    return payload


def main() -> None:
    args = parse_args()
    if not analysis_boundary_allowed(args):
        raise SystemExit(
            "Codex analysis helper requires --allow-ai-analysis-boundary or "
            f"{ALLOW_ANALYSIS_ENV}=1. Use Rust-owned radar commands for "
            "deterministic Radar workflows; GitHub Actions must not run this helper."
        )

    bundle_path = Path(args.bundle)
    bundle = load_json(bundle_path)
    bundle_validation = validate_bundle(bundle)
    if not bundle_validation.ok:
        raise SystemExit("Bundle validation failed:\n- " + "\n- ".join(bundle_validation.errors))

    repo_root = Path(args.repo_root).resolve() if args.repo_root else repo_root_from(bundle_path)
    output_schema = SCRIPT_HOME / "analysis_draft.schema.json"
    prompt = build_prompt(bundle_path, repo_root)

    with tempfile.NamedTemporaryFile(prefix="decodex-analysis-", suffix=".json", delete=False) as handle:
        tmp_output = Path(handle.name)

    cmd = [
        args.codex_bin,
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--color",
        "never",
        "--output-schema",
        str(output_schema),
        "-C",
        str(repo_root),
        "-o",
        str(tmp_output),
        prompt,
    ]
    if args.model:
        cmd[2:2] = ["--model", args.model]

    try:
        completed = subprocess.run(cmd, check=False, capture_output=True, text=True)
        if completed.returncode != 0:
            stderr = completed.stderr.strip()
            stdout = completed.stdout.strip()
            details = stderr or stdout or "unknown error"
            raise SystemExit(f"codex exec failed: {details}")

        payload = extract_json_payload(tmp_output.read_text(encoding="utf-8"))
        validation = validate_analysis_draft(payload)
        if not validation.ok:
            raise SystemExit("Analysis draft validation failed:\n- " + "\n- ".join(validation.errors))

        dump_json(args.out, payload)
    finally:
        tmp_output.unlink(missing_ok=True)
    print(args.out)


if __name__ == "__main__":
    main()
