"""Large-change path heuristics for lifecycle hook safeguards."""

from __future__ import annotations

from typing import Any

from .constants import (
    LARGE_ADDITION_THRESHOLD,
    LARGE_CHANGE_THRESHOLD,
    LARGE_SOURCE_FILE_LINE_THRESHOLD,
    LARGE_SOURCE_TOUCH_THRESHOLD,
    NEW_SOURCE_FILE_THRESHOLD,
    SOURCE_ADDITION_THRESHOLD,
)
from .git_state import changed_file_stats, file_line_count, git_root
from .surface_base import path_excluded_from_large_change, path_is_source_file


def stat_int(item: dict[str, Any], key: str) -> int:
    try:
        return int(item.get(key) or 0)
    except (TypeError, ValueError):
        return 0


def source_touch_is_large(line_count: int | None, changed: int) -> bool:
    return (
        line_count is not None
        and line_count >= LARGE_SOURCE_FILE_LINE_THRESHOLD
        and changed >= LARGE_SOURCE_TOUCH_THRESHOLD
    )


def change_is_large(path: str, added: int, removed: int, changed: int, line_count: int | None) -> bool:
    return any(
        (
            changed >= LARGE_CHANGE_THRESHOLD,
            added >= LARGE_ADDITION_THRESHOLD,
            path_is_source_file(path) and added >= SOURCE_ADDITION_THRESHOLD,
            path_is_source_file(path) and removed == 0 and added >= NEW_SOURCE_FILE_THRESHOLD,
            source_touch_is_large(line_count, changed),
        )
    )


def large_change_paths(stats: list[dict[str, Any]] | None = None) -> list[str]:
    stats = changed_file_stats() if stats is None else stats
    large_paths = []
    root = git_root()
    for item in stats:
        path = str(item["path"])
        if path_excluded_from_large_change(path):
            continue
        added = stat_int(item, "added")
        changed = stat_int(item, "changed")
        removed = stat_int(item, "removed")
        line_count = file_line_count(path, root) if path_is_source_file(path) else None
        if change_is_large(path, added, removed, changed, line_count):
            large_paths.append(path)
    return large_paths
