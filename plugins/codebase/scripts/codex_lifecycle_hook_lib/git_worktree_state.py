"""Git branch and worktree state helpers."""

from __future__ import annotations

from pathlib import Path

from .git_core import git_output


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
