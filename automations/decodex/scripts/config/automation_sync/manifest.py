"""Manifest loading and public-safety validation for automation sync."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from automation_sync.paths import REPO_ROOT
from automation_sync.toml_io import load_toml


PRIVATE_CONFIG_FIELDS = {"created_at", "updated_at", "cwds"}


def forbidden_fragments(defaults: dict[str, Any]) -> list[str]:
    return [str(value) for value in defaults.get("forbidden_prompt_fragments", [])]


def assert_public_manifest(manifest_path: Path, manifest: dict[str, Any]) -> None:
    defaults = manifest.get("defaults")
    if not isinstance(defaults, dict):
        raise ValueError(f"{manifest_path}: defaults must be a table")
    if defaults.get("cwd") != "{repo_root}":
        raise ValueError(f"{manifest_path}: defaults.cwd must stay the portable {{repo_root}} placeholder")
    forbidden = forbidden_fragments(defaults)
    for required in ["/Users/", "/home/", "accounts.jsonl", "auth.json", "runtime.sqlite3"]:
        if required not in forbidden:
            raise ValueError(f"{manifest_path}: forbidden_prompt_fragments must include {required!r}")


def assert_prompt_public(prompt_path: Path, prompt: str, forbidden: list[str]) -> None:
    for fragment in forbidden:
        if fragment and fragment in prompt:
            raise ValueError(f"{prompt_path}: prompt contains forbidden private fragment {fragment!r}")


def automation_specs(manifest_path: Path) -> list[dict[str, Any]]:
    manifest_path = manifest_path if manifest_path.is_absolute() else REPO_ROOT / manifest_path
    manifest = load_toml(manifest_path)
    assert_public_manifest(manifest_path, manifest)
    defaults = manifest["defaults"]
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
