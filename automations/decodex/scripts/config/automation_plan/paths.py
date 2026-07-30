"""Repository path helpers for automation lifecycle plans."""

from __future__ import annotations

from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[5]
DEFAULT_MANIFESTS = [
    REPO_ROOT / "automations/upstream/automations.toml",
    REPO_ROOT / "automations/decodex/automations.toml",
]


def manifest_paths(values: list[str]) -> list[Path]:
    if not values:
        return DEFAULT_MANIFESTS
    return [Path(value) for value in values]
