"""Git command classification helpers for lifecycle hook safeguards."""

from __future__ import annotations

from pathlib import Path

from .constants import (
    GIT_BRANCH_CREATE_OPTIONS,
    GIT_GLOBAL_OPTIONS_WITH_ARG,
    GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX,
    GIT_OPTIONS_WITH_ARG,
)
from .git_state import git_branch_ref_exists, git_output
from .shell_tokens import shell_token_segments, unwrap_shell_command_tokens


def split_git_command(tokens: list[str]) -> tuple[str, list[str], str | None] | None:
    tokens = unwrap_shell_command_tokens(tokens)
    if not tokens or tokens[0] != "git":
        return None
    index = 1
    command_cwd: str | None = None
    while index < len(tokens):
        token = tokens[index]
        if token == "--":
            index += 1
            break
        if token in GIT_GLOBAL_OPTIONS_WITH_ARG:
            if token == "-C" and index + 1 < len(tokens):
                command_cwd = tokens[index + 1]
            index += 2
            continue
        if any(token.startswith(prefix) for prefix in GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX):
            index += 1
            continue
        if token.startswith("-"):
            index += 1
            continue
        break
    if index >= len(tokens):
        return None
    return tokens[index], tokens[index + 1 :], command_cwd


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


def checkout_branch_target(args: list[str], command: str) -> tuple[str, bool] | None:
    args_before_pathspec = args[: args.index("--")] if "--" in args else args
    index = 0
    operands: list[str] = []
    while index < len(args_before_pathspec):
        token = args_before_pathspec[index]
        if token in GIT_BRANCH_CREATE_OPTIONS:
            target = args_before_pathspec[index + 1] if index + 1 < len(args_before_pathspec) else ""
            return target, True
        if token in GIT_OPTIONS_WITH_ARG:
            index += 2
            continue
        if token.startswith("--"):
            index += 1
            continue
        if token.startswith("-") and token not in {"-", "@{-1}"}:
            index += 1
            continue
        operands.append(token)
        index += 1

    if "--" in args:
        return None
    if not operands:
        return None
    target = operands[0]
    if command == "switch" or target in {"-", "@{-1}"} or git_branch_ref_exists(target):
        return target, False
    return None


def git_switch_targets(text: str, root: str | None = None) -> list[tuple[str, bool]]:
    targets: list[tuple[str, bool]] = []
    for tokens in shell_token_segments(text):
        parsed = split_git_command(tokens)
        if not parsed:
            continue
        command, args, command_cwd = parsed
        if not git_command_cwd_targets_root(command_cwd, root):
            continue
        if command not in {"switch", "checkout"}:
            continue
        target = checkout_branch_target(args, command)
        if target:
            targets.append(target)
    return targets


def target_is_default_branch(target: str, default_branch: str) -> bool:
    return target in {default_branch, f"origin/{default_branch}", f"refs/heads/{default_branch}"}


def switches_root_to_non_default_branch(text: str, default_branch: str, root: str | None = None) -> bool:
    for target, creates_branch in git_switch_targets(text, root):
        if creates_branch:
            return True
        if target and not target_is_default_branch(target, default_branch):
            return True
    return False


def switches_root_back_to_default_branch(text: str, default_branch: str, root: str | None = None) -> bool:
    targets = git_switch_targets(text, root)
    return bool(targets) and all(
        not creates_branch and target_is_default_branch(target, default_branch)
        for target, creates_branch in targets
    )
