"""Root worktree switch-back classification."""

from __future__ import annotations

from .shell_control import has_shell_control_or_redirection
from .shell_git import (
    checkout_branch_target,
    git_command_cwd_targets_root,
    split_git_command,
    target_is_default_branch,
)
from .shell_tokens import shell_token_segments


def is_switch_back_segment(tokens: list[str], default_branch: str, root: str | None = None) -> bool:
    parsed = split_git_command(tokens)
    if not parsed:
        return False
    command, args, command_cwd = parsed
    if not git_command_cwd_targets_root(command_cwd, root):
        return False
    if command not in {"switch", "checkout"}:
        return False
    target = checkout_branch_target(args, command)
    return bool(
        target
        and not target[1]
        and target_is_default_branch(target[0], default_branch)
    )


def is_only_root_switch_back(text: str, default_branch: str, root: str | None = None) -> bool:
    if has_shell_control_or_redirection(text):
        return False
    segments = shell_token_segments(text)
    return bool(segments) and all(is_switch_back_segment(segment, default_branch, root) for segment in segments)
