"""Path classification and large-change heuristics."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .constants import (
    DEPENDENCY_SURFACE_NAMES,
    EXCLUDED_LARGE_CHANGE_PARTS,
    FAKE_RUST_MODULARIZATION_PATTERNS,
    LARGE_ADDITION_THRESHOLD,
    LARGE_CHANGE_THRESHOLD,
    LARGE_SOURCE_FILE_LINE_THRESHOLD,
    LARGE_SOURCE_TOUCH_THRESHOLD,
    NEW_SOURCE_FILE_THRESHOLD,
    PUBLIC_SURFACE_NAMES,
    PUBLIC_SURFACE_PREFIXES,
    PUBLIC_SURFACE_SEGMENTS,
    PUBLIC_SURFACE_STEMS,
    SOURCE_ADDITION_THRESHOLD,
    SOURCE_EXTENSIONS,
    TASK_RUNNER_NAMES,
)
from .git_state import changed_file_stats, changed_paths, file_line_count, git_root


def path_is_source_file(path: str) -> bool:
    return Path(path).suffix in SOURCE_EXTENSIONS and not path_excluded_from_large_change(path)

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


def path_is_task_runner_surface(path: str) -> bool:
    return path.rsplit("/", maxsplit=1)[-1] in TASK_RUNNER_NAMES


def task_runner_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [path for path in paths if path_is_task_runner_surface(path)]


def path_is_dependency_surface(path: str) -> bool:
    name = path.rsplit("/", maxsplit=1)[-1]
    if name in DEPENDENCY_SURFACE_NAMES:
        return True
    return path.startswith(".github/workflows/") and path.endswith((".yml", ".yaml"))


def dependency_surface_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [path for path in paths if path_is_dependency_surface(path)]


def path_excluded_from_large_change(path: str) -> bool:
    normalized = f"/{path}"
    return any(part in normalized for part in EXCLUDED_LARGE_CHANGE_PARTS)

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


def large_change_paths(stats: list[dict[str, Any]] | None = None) -> list[str]:
    stats = changed_file_stats() if stats is None else stats
    large_paths = []
    root = git_root()
    for item in stats:
        path = str(item["path"])
        if path_excluded_from_large_change(path):
            continue
        try:
            added = int(item["added"])
        except (TypeError, ValueError):
            added = 0
        changed = int(item.get("changed") or 0)
        removed_raw = item.get("removed")
        try:
            removed = int(removed_raw)
        except (TypeError, ValueError):
            removed = 0
        line_count = file_line_count(path, root) if path_is_source_file(path) else None
        is_large_source_touch = (
            line_count is not None
            and line_count >= LARGE_SOURCE_FILE_LINE_THRESHOLD
            and changed >= LARGE_SOURCE_TOUCH_THRESHOLD
        )
        is_new_source = path_is_source_file(path) and removed == 0 and added >= NEW_SOURCE_FILE_THRESHOLD
        is_source_growth = path_is_source_file(path) and added >= SOURCE_ADDITION_THRESHOLD
        if (
            changed >= LARGE_CHANGE_THRESHOLD
            or added >= LARGE_ADDITION_THRESHOLD
            or is_source_growth
            or is_new_source
            or is_large_source_touch
        ):
            large_paths.append(path)
    return large_paths
