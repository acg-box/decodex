"""Shared path helpers for lifecycle hook surface checks."""

from __future__ import annotations

from pathlib import Path

from .constants import EXCLUDED_LARGE_CHANGE_PARTS, SOURCE_EXTENSIONS


def path_excluded_from_large_change(path: str) -> bool:
    normalized = f"/{path}"
    return any(part in normalized for part in EXCLUDED_LARGE_CHANGE_PARTS)


def path_is_source_file(path: str) -> bool:
    return Path(path).suffix in SOURCE_EXTENSIONS and not path_excluded_from_large_change(path)
