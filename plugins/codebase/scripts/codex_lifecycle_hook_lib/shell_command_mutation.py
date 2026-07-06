"""Shell command mutation classification."""

from __future__ import annotations

from typing import Any

from .constants import ALWAYS_MUTATING_TOOL_NAMES, MUTATING_COMMAND_TERMS, SHELL_MUTATING_COMMANDS
from .shell_git import git_command_cwd_targets_root, split_git_command
from .shell_package_mutation import (
    cargo_command_is_mutating,
    inplace_editor_command_is_mutating,
    package_manager_command_is_mutating,
)
from .shell_quote_scan import has_shell_substitution, has_unquoted_redirection
from .shell_tokens import shell_token_segments, unwrap_shell_command_tokens


def payload_tool_name(payload: dict[str, Any]) -> str:
    for key in ("tool_name", "toolName", "tool"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    return ""


def shell_segment_is_mutating(tokens: list[str], root: str | None = None) -> bool:
    if not tokens:
        return False
    tokens = unwrap_shell_command_tokens(tokens)
    if not tokens:
        return False
    parsed = split_git_command(tokens)
    if parsed:
        command, _, command_cwd = parsed
        if not git_command_cwd_targets_root(command_cwd, root):
            return False
        return command in MUTATING_COMMAND_TERMS
    command = tokens[0]
    if command in SHELL_MUTATING_COMMANDS:
        return True
    if command == "cargo":
        return cargo_command_is_mutating(tokens)
    if command in {"npm", "pnpm", "yarn", "bun"}:
        return package_manager_command_is_mutating(tokens)
    if command in {"perl", "sed"}:
        return inplace_editor_command_is_mutating(tokens)
    return False


def payload_is_mutating(payload: dict[str, Any], text: str, root: str | None = None) -> bool:
    tool_name = payload_tool_name(payload).lower()
    if any(name in tool_name for name in ALWAYS_MUTATING_TOOL_NAMES):
        return True
    if has_shell_substitution(text) or has_unquoted_redirection(text):
        return True
    for tokens in shell_token_segments(text):
        if shell_segment_is_mutating(tokens, root):
            return True
    return False
