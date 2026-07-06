"""Module-boundary surface heuristics."""

from __future__ import annotations

from pathlib import Path

from .constants import FAKE_RUST_MODULARIZATION_PATTERNS, GENERIC_MODULE_BUCKET_SEGMENTS
from .git_state import changed_paths, git_root
from .surface_base import path_excluded_from_large_change, path_is_source_file


def fake_modularization_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    root = git_root()
    if not root:
        return []
    offenders = []
    for path in paths:
        if not path.endswith(".rs") or path_excluded_from_large_change(path):
            continue
        full_path = Path(root) / path
        try:
            text = full_path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        if any(pattern in text for pattern in FAKE_RUST_MODULARIZATION_PATTERNS):
            offenders.append(path)
    return offenders


def path_has_generic_module_bucket(path: str) -> bool:
    lowered = path.lower()
    parts = [part for part in Path(lowered).parts if part]
    stem = Path(lowered).stem
    return (
        any(part in GENERIC_MODULE_BUCKET_SEGMENTS for part in parts)
        or stem in GENERIC_MODULE_BUCKET_SEGMENTS
    )


def module_boundary_risk_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [
        path
        for path in paths
        if path_is_source_file(path) and path_has_generic_module_bucket(path)
    ]
