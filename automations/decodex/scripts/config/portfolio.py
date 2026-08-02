"""Compact authority for the exact five native Decodex automations."""

from __future__ import annotations

import subprocess
import tomllib
from pathlib import Path
from typing import Any

from automation_model_policy import (
    MODEL_BY_AUTOMATION_ID,
    REASONING_EFFORT_BY_AUTOMATION_ID,
)

REPO_ROOT = Path(__file__).resolve().parents[4]
MANIFEST_PATH = REPO_ROOT / "automations/portfolio.toml"
AUTOMATION_IDS = tuple(MODEL_BY_AUTOMATION_ID)
EXPECTED_IDS = frozenset(AUTOMATION_IDS)
MANAGED_PREFIXES = ("codex-upstream-", "decodex-")
MANIFEST_STATUSES = frozenset({"PAUSED", "ACTIVE"})
ROOT_KEYS = {
    "automations",
    "execution_environment",
    "kind",
    "primary_cwd",
    "status",
    "version",
}
AUTOMATION_KEYS = {
    "id",
    "model",
    "name",
    "prompt_file",
    "reasoning_effort",
    "rrule",
}
class PortfolioError(ValueError):
    """The checked-in or native portfolio violates exact-five authority."""


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def load_manifest(path: Path = MANIFEST_PATH) -> dict[str, Any]:
    manifest = load_toml(path)
    errors = validate_manifest(manifest)
    if errors:
        raise PortfolioError("; ".join(errors))
    return manifest


def validate_manifest(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if set(manifest) != ROOT_KEYS:
        errors.append("portfolio root keys must match the compact contract")
    if manifest.get("version") != 1:
        errors.append("portfolio version must be 1")
    for field, expected in (
        ("primary_cwd", "{primary_worktree}"),
        ("kind", "cron"),
        ("execution_environment", "local"),
    ):
        if manifest.get(field) != expected:
            errors.append(f"portfolio {field} must be {expected!r}")
    if manifest.get("status") not in MANIFEST_STATUSES:
        errors.append("portfolio status must be one of 'ACTIVE', 'PAUSED'")

    automations = manifest.get("automations")
    if not isinstance(automations, list):
        errors.append("portfolio automations must be an array")
        return errors
    ids = [entry.get("id") for entry in automations if isinstance(entry, dict)]
    if len(automations) != 5 or set(ids) != EXPECTED_IDS or len(ids) != len(set(ids)):
        errors.append("portfolio must contain each exact managed automation once")
    for entry in automations:
        if not isinstance(entry, dict):
            errors.append("portfolio automation entries must be tables")
            continue
        if set(entry) != AUTOMATION_KEYS:
            errors.append(f"automation {entry.get('id')!r} keys must match the compact contract")
            continue
        automation_id = entry["id"]
        if entry["model"] != MODEL_BY_AUTOMATION_ID.get(automation_id):
            errors.append(f"automation {automation_id!r} has the wrong model")
        if entry["reasoning_effort"] != REASONING_EFFORT_BY_AUTOMATION_ID.get(automation_id):
            errors.append(f"automation {automation_id!r} has the wrong reasoning effort")
        prompt_path = REPO_ROOT / entry["prompt_file"]
        if not prompt_path.is_file():
            errors.append(f"automation {automation_id!r} prompt is missing")
            continue
        prompt = prompt_path.read_text(encoding="utf-8")
        lines = [line for line in prompt.splitlines() if line.strip()]
        if not 12 <= len(lines) <= 80:
            errors.append(f"automation {automation_id!r} prompt must have 12 to 80 nonempty lines")
        lowered = prompt.casefold()
        if "xhigh" in lowered:
            errors.append(f"automation {automation_id!r} prompt contains a forbidden effort")
    return errors


def primary_worktree(repo_root: Path = REPO_ROOT) -> Path:
    result = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    )
    records: list[dict[str, str]] = []
    current: dict[str, str] = {}
    for line in result.stdout.splitlines() + [""]:
        if not line:
            if current:
                records.append(current)
                current = {}
            continue
        key, _, value = line.partition(" ")
        current[key] = value
    candidates = [
        Path(record["worktree"]).resolve()
        for record in records
        if record.get("branch") == "refs/heads/main"
        and ".worktrees" not in Path(record["worktree"]).parts
    ]
    if len(candidates) == 1:
        return candidates[0]
    resolved = repo_root.resolve()
    if ".worktrees" not in resolved.parts and not candidates:
        return resolved
    raise PortfolioError("one primary main worktree outside .worktrees is required")


def rendered_automations(manifest: dict[str, Any] | None = None) -> list[dict[str, Any]]:
    manifest = load_manifest() if manifest is None else manifest
    cwd = str(primary_worktree())
    rendered = []
    for entry in manifest["automations"]:
        prompt = (REPO_ROOT / entry["prompt_file"]).read_text(encoding="utf-8").strip()
        rendered.append(
            {
                "id": entry["id"],
                "kind": manifest["kind"],
                "name": entry["name"],
                "prompt": prompt,
                "status": manifest["status"],
                "rrule": entry["rrule"],
                "model": entry["model"],
                "reasoning_effort": entry["reasoning_effort"],
                "execution_environment": manifest["execution_environment"],
                "cwds": [cwd],
            }
        )
    return rendered


def evaluate_runtime(codex_home: Path) -> dict[str, Any]:
    manifest = load_manifest()
    expected = {entry["id"]: entry for entry in rendered_automations(manifest)}
    runtime_root = codex_home / "automations"
    found_managed: set[str] = set()
    results = []
    for automation_id, wanted in expected.items():
        errors: list[str] = []
        path = runtime_root / automation_id / "automation.toml"
        if not path.is_file():
            errors.append("native definition is missing")
            actual: dict[str, Any] = {}
        else:
            actual = load_toml(path)
            found_managed.add(automation_id)
        for field in (
            "id",
            "kind",
            "name",
            "prompt",
            "status",
            "rrule",
            "model",
            "reasoning_effort",
            "execution_environment",
            "cwds",
        ):
            if actual.get(field) != wanted[field]:
                errors.append(f"native {field} differs from portfolio")
        for field in ("created_at", "updated_at"):
            if not isinstance(actual.get(field), int) or actual[field] <= 0:
                errors.append(f"native {field} metadata is missing")
        results.append({"id": automation_id, "status": "pass" if not errors else "fail", "errors": errors})

    if runtime_root.is_dir():
        for path in runtime_root.glob("*/automation.toml"):
            try:
                automation_id = load_toml(path).get("id")
            except (OSError, tomllib.TOMLDecodeError):
                continue
            if isinstance(automation_id, str) and (
                automation_id in EXPECTED_IDS or automation_id.startswith(MANAGED_PREFIXES)
            ):
                found_managed.add(automation_id)
    extras = sorted(found_managed - EXPECTED_IDS)
    status = "pass" if all(item["status"] == "pass" for item in results) and not extras else "fail"
    return {"status": status, "exact_ids": list(AUTOMATION_IDS), "extra_managed_ids": extras, "results": results}
