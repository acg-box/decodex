"""Git changed-file read helpers."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .git_core import git_output, git_root


def changed_file_stats() -> list[dict[str, Any]]:
    output = git_output(["git", "diff", "HEAD", "--numstat"])
    if output is None:
        return []
    stats = []
    for line in output.splitlines():
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        added, removed, path = parts
        try:
            total = int(added) + int(removed)
        except ValueError:
            total = 0
        stats.append({"path": path, "added": added, "removed": removed, "changed": total})
    return stats


def changed_paths() -> list[str]:
    output = git_output(["git", "diff", "HEAD", "--name-only"])
    if output is None:
        return []
    return sorted({line.strip() for line in output.splitlines() if line.strip()})


def file_line_count(path: str, root: str | None = None) -> int | None:
    root = git_root() if root is None else root
    if not root:
        return None
    full_path = Path(root) / path
    try:
        with full_path.open("r", encoding="utf-8", errors="ignore") as handle:
            return sum(1 for _ in handle)
    except OSError:
        return None
