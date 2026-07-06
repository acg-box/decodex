"""Git switch/checkout ownership checks."""

from __future__ import annotations

from .shell_git_checkout import checkout_branch_target, target_is_default_branch
from .shell_git_parse import split_git_command
from .shell_git_paths import git_command_cwd_targets_root
from .shell_tokens import shell_token_segments


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
