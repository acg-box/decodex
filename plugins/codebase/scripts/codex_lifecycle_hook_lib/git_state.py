"""Git and changed-file read helpers for lifecycle hook checks."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

from .constants import COMMIT_SCHEMA

def git_output(args: list[str], timeout: int = 3, cwd: Path | None = None) -> str | None:
    try:
        result = subprocess.run(
            args,
            check=True,
            cwd=str(cwd) if cwd is not None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return result.stdout


def git_root() -> str | None:
    output = git_output(["git", "rev-parse", "--show-toplevel"], timeout=2)
    return output.strip() if output else None


def git_current_branch() -> str | None:
    output = git_output(["git", "branch", "--show-current"], timeout=2)
    branch = output.strip() if output else ""
    return branch or None


def git_default_branch() -> str | None:
    output = git_output(["git", "symbolic-ref", "--short", "refs/remotes/origin/HEAD"], timeout=2)
    if output:
        branch = output.strip()
        if "/" in branch:
            return branch.split("/", maxsplit=1)[1]
        if branch:
            return branch
    output = git_output(["git", "config", "--get", "init.defaultBranch"], timeout=2)
    branch = output.strip() if output else ""
    return branch or "main"


def git_is_root_worktree() -> bool:
    git_dir = git_output(["git", "rev-parse", "--path-format=absolute", "--git-dir"], timeout=2)
    common_dir = git_output(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        timeout=2,
    )
    if not git_dir or not common_dir:
        return False
    try:
        return Path(git_dir.strip()).resolve() == Path(common_dir.strip()).resolve()
    except OSError:
        return git_dir.strip() == common_dir.strip()


def git_branch_ref_exists(target: str) -> bool:
    if not target:
        return False
    refs = [target, f"refs/heads/{target}", f"refs/remotes/{target}", f"refs/remotes/origin/{target}"]
    return any(git_output(["git", "rev-parse", "--verify", "--quiet", ref], timeout=2) for ref in refs)


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

def commit_subject_is_valid(subject: str) -> bool:
    if "\n" in subject:
        return False
    try:
        value = json.loads(subject)
    except json.JSONDecodeError:
        return False
    if not isinstance(value, dict):
        return False
    return (
        value.get("schema") == COMMIT_SCHEMA
        and isinstance(value.get("summary"), str)
        and bool(value["summary"].strip())
        and isinstance(value.get("authority"), str)
        and bool(value["authority"].strip())
    )


def ahead_commit_subjects(limit: int = 20) -> list[str]:
    output = git_output(["git", "rev-list", "--format=%s", "@{u}..HEAD"], timeout=4)
    if output is None:
        return []
    subjects = [
        line.strip()
        for line in output.splitlines()
        if line.strip() and not line.startswith("commit ")
    ]
    return subjects[:limit]


def invalid_ahead_commit_subjects() -> list[str]:
    return [subject for subject in ahead_commit_subjects() if not commit_subject_is_valid(subject)]
