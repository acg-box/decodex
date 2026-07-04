"""Codex command construction and execution for Radar analysis."""

from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
from typing import Any

from analysis_runner.paths import SCRIPT_HOME
from analysis_runner.payload import extract_json_payload
from analysis_runner.prompt import build_prompt


def codex_command(
    codex_bin: str,
    model: str | None,
    repo_root: Path,
    output_schema: Path,
    tmp_output: Path,
    prompt: str,
) -> list[str]:
    cmd = [
        codex_bin,
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
    if model:
        cmd[2:2] = ["--model", model]
    return cmd


def run_codex_analysis(args: Any, bundle_path: Path, repo_root: Path) -> dict[str, Any]:
    output_schema = SCRIPT_HOME / "analysis_draft.schema.json"
    prompt = build_prompt(bundle_path, repo_root)

    with tempfile.NamedTemporaryFile(prefix="decodex-analysis-", suffix=".json", delete=False) as handle:
        tmp_output = Path(handle.name)

    try:
        cmd = codex_command(args.codex_bin, args.model, repo_root, output_schema, tmp_output, prompt)
        completed = subprocess.run(cmd, check=False, capture_output=True, text=True)
        if completed.returncode != 0:
            stderr = completed.stderr.strip()
            stdout = completed.stdout.strip()
            details = stderr or stdout or "unknown error"
            raise SystemExit(f"codex exec failed: {details}")

        return extract_json_payload(tmp_output.read_text(encoding="utf-8"))
    finally:
        tmp_output.unlink(missing_ok=True)
