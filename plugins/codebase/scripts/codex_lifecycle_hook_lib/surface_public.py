"""Public-surface path classification for lifecycle hook safeguards."""

from __future__ import annotations

from pathlib import Path

from .constants import (
    PUBLIC_SURFACE_NAMES,
    PUBLIC_SURFACE_PREFIXES,
    PUBLIC_SURFACE_SEGMENTS,
    PUBLIC_SURFACE_STEMS,
)
from .git_state import changed_paths


def path_is_public_surface(path: str) -> bool:
    if path in PUBLIC_SURFACE_NAMES or path.rsplit("/", maxsplit=1)[-1] in PUBLIC_SURFACE_NAMES:
        return True
    if path.startswith(PUBLIC_SURFACE_PREFIXES):
        return True
    lowered = path.lower()
    parts = [part for part in lowered.split("/") if part]
    stem = Path(path).stem.lower()
    return any(part in PUBLIC_SURFACE_SEGMENTS for part in parts) or stem in PUBLIC_SURFACE_STEMS


def public_surface_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [path for path in paths if path_is_public_surface(path)]
