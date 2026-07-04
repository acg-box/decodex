"""Path and environment helpers for automation synchronization."""

from __future__ import annotations

import os
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[5]
DEFAULT_MANIFESTS = [
    REPO_ROOT / "automations/decodex/automations.toml",
    REPO_ROOT / "automations/radar/automations.toml",
]


def default_codex_home() -> str:
    return os.environ.get("CODEX_HOME") or str(Path.home() / ".codex")


def live_automation_path(codex_home: Path, automation_id: str) -> Path:
    return codex_home / "automations" / automation_id / "automation.toml"


def display_automation_path(automation_id: str) -> str:
    return f"$CODEX_HOME/automations/{automation_id}/automation.toml"


def manifest_paths(values: list[str]) -> list[Path]:
    if not values:
        return DEFAULT_MANIFESTS
    return [Path(value) for value in values]


def resolve_codex_home(value: str, repo_root: Path) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise ValueError("--codex-home must be an absolute path or a home-relative path like ~/.codex")
    resolved = path.resolve()
    if resolved == repo_root or repo_root in resolved.parents:
        raise ValueError("--codex-home must not point inside the repository checkout")
    return resolved
