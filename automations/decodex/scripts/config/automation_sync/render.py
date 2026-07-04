"""Live automation config rendering and redacted snapshot helpers."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from automation_sync.manifest import PRIVATE_CONFIG_FIELDS
from automation_sync.toml_io import load_toml, toml_string


def render_live_config(spec: dict[str, Any], repo_root: Path, created_at: int, updated_at: int) -> str:
    lines = [
        "version = 1",
        f"id = {toml_string(spec['id'])}",
        f"kind = {toml_string(spec['kind'])}",
        f"name = {toml_string(spec['name'])}",
        f"prompt = {toml_string(spec['prompt'])}",
        f"status = {toml_string(spec['status'])}",
        f"rrule = {toml_string(spec['rrule'])}",
        f"model = {toml_string(spec['model'])}",
        f"reasoning_effort = {toml_string(spec['reasoning_effort'])}",
        f"execution_environment = {toml_string(spec['execution_environment'])}",
        f"cwds = [{toml_string(str(repo_root))}]",
        f"created_at = {created_at}",
        f"updated_at = {updated_at}",
        "",
    ]
    return "\n".join(lines)


def normalized_snapshot(spec: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in spec.items() if key not in PRIVATE_CONFIG_FIELDS and key != "prompt"}


def existing_created_at(path: Path, fallback: int) -> int:
    if not path.exists():
        return fallback
    data = load_toml(path)
    value = data.get("created_at")
    return int(value) if isinstance(value, int) else fallback
