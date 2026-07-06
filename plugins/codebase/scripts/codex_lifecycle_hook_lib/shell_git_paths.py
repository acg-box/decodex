"""Git command working-directory ownership checks."""

from __future__ import annotations

from pathlib import Path

from .git_state import git_output


def git_command_cwd_targets_root(command_cwd: str | None, root: str | None) -> bool:
    if not command_cwd or not root:
        return True
    try:
        root_path = Path(root).resolve()
        cwd_path = Path(command_cwd)
        if not cwd_path.is_absolute():
            cwd_path = root_path / cwd_path
        cwd_path = cwd_path.resolve()
    except OSError:
        return True
    worktrees_path = root_path / ".worktrees"
    if cwd_path == worktrees_path or worktrees_path not in cwd_path.parents:
        return True
    target_root = git_output(["git", "rev-parse", "--show-toplevel"], timeout=2, cwd=cwd_path)
    if not target_root:
        return True
    try:
        return Path(target_root.strip()).resolve() == root_path
    except OSError:
        return True
