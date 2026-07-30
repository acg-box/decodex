"""Load and validate portable automation manifests."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any

from automation_plan.paths import REPO_ROOT

_AUTOMATION_ID = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def forbidden_fragments(defaults: dict[str, Any]) -> list[str]:
    return [str(value) for value in defaults.get("forbidden_prompt_fragments", [])]


def assert_public_manifest(manifest_path: Path, manifest: dict[str, Any]) -> None:
    defaults = manifest.get("defaults")
    if not isinstance(defaults, dict):
        raise ValueError(f"{manifest_path}: defaults must be a table")
    if defaults.get("cwd") != "{repo_root}":
        raise ValueError(
            f"{manifest_path}: defaults.cwd must stay the portable "
            "{repo_root} placeholder"
        )
    for required in ["/Users/", "/home/", "accounts.jsonl", "auth.json", "runtime.sqlite3"]:
        if required not in forbidden_fragments(defaults):
            raise ValueError(
                f"{manifest_path}: forbidden_prompt_fragments must include "
                f"{required!r}"
            )


def assert_prompt_public(prompt_path: Path, prompt: str, forbidden: list[str]) -> None:
    for fragment in forbidden:
        if fragment and fragment in prompt:
            raise ValueError(
                f"{prompt_path}: prompt contains forbidden private fragment "
                f"{fragment!r}"
            )


def _manifest_automation_ids(
    manifest_path: Path, manifest: dict[str, Any]
) -> list[str]:
    automations = manifest.get("automations")
    if not isinstance(automations, list) or not automations:
        raise ValueError(f"{manifest_path}: automations must be a non-empty array")
    ids: list[str] = []
    for automation in automations:
        if not isinstance(automation, dict):
            raise ValueError(f"{manifest_path}: each automation must be a table")
        automation_id = automation.get("id")
        if not isinstance(automation_id, str) or not _AUTOMATION_ID.fullmatch(
            automation_id
        ):
            raise ValueError(f"{manifest_path}: invalid automation id")
        ids.append(automation_id)
    if len(ids) != len(set(ids)):
        raise ValueError(f"{manifest_path}: automation ids must be unique")
    return ids


def retirement_ids(manifest_path: Path) -> list[str]:
    manifest_path = (
        manifest_path if manifest_path.is_absolute() else REPO_ROOT / manifest_path
    )
    manifest = load_toml(manifest_path)
    assert_public_manifest(manifest_path, manifest)
    values = manifest.get("retired_automation_ids", [])
    if not isinstance(values, list):
        raise ValueError(f"{manifest_path}: retired_automation_ids must be an array")
    retired: list[str] = []
    for value in values:
        if not isinstance(value, str) or not _AUTOMATION_ID.fullmatch(value):
            raise ValueError(f"{manifest_path}: invalid retired automation id")
        retired.append(value)
    if len(retired) != len(set(retired)):
        raise ValueError(f"{manifest_path}: retired automation ids must be unique")
    overlap = set(retired) & set(_manifest_automation_ids(manifest_path, manifest))
    if overlap:
        raise ValueError(
            f"{manifest_path}: active and retired automation ids overlap: "
            f"{', '.join(sorted(overlap))}"
        )
    return retired


def automation_specs(manifest_path: Path) -> list[dict[str, Any]]:
    manifest_path = (
        manifest_path if manifest_path.is_absolute() else REPO_ROOT / manifest_path
    )
    manifest = load_toml(manifest_path)
    assert_public_manifest(manifest_path, manifest)
    defaults = manifest["defaults"]
    _manifest_automation_ids(manifest_path, manifest)
    specs: list[dict[str, Any]] = []
    for automation in manifest.get("automations", []):
        prompt_file = REPO_ROOT / automation["prompt_file"]
        prompt = prompt_file.read_text(encoding="utf-8").strip()
        assert_prompt_public(prompt_file, prompt, forbidden_fragments(defaults))
        specs.append(
            {
                "id": automation["id"],
                "kind": defaults["kind"],
                "name": automation["name"],
                "prompt": prompt,
                "status": defaults["status"],
                "rrule": automation["rrule"],
                "model": defaults["model"],
                "reasoning_effort": defaults["reasoning_effort"],
                "execution_environment": defaults["execution_environment"],
                "source_manifest": str(manifest_path.relative_to(REPO_ROOT)),
                "prompt_file": automation["prompt_file"],
            }
        )
    return specs
