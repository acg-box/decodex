"""Shell mutation classification helpers for lifecycle hook safeguards."""

from __future__ import annotations

from typing import Any

from .constants import ALWAYS_MUTATING_TOOL_NAMES, MUTATING_COMMAND_TERMS, SHELL_MUTATING_COMMANDS
from .shell_git import (
    checkout_branch_target,
    git_command_cwd_targets_root,
    split_git_command,
    target_is_default_branch,
)
from .shell_tokens import shell_token_segments, unwrap_shell_command_tokens


def payload_tool_name(payload: dict[str, Any]) -> str:
    for key in ("tool_name", "toolName", "tool"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    return ""


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


def has_shell_control_or_redirection(text: str) -> bool:
    return has_unquoted_shell_control(text) or has_unquoted_redirection(text) or has_shell_substitution(text)


def has_shell_substitution(text: str) -> bool:
    single_quote = False
    double_quote = False
    escaped = False
    index = 0
    while index < len(text):
        char = text[index]
        if escaped:
            escaped = False
            index += 1
            continue
        if char == "\\":
            escaped = True
            index += 1
            continue
        if char == "'" and not double_quote:
            single_quote = not single_quote
            index += 1
            continue
        if char == '"' and not single_quote:
            double_quote = not double_quote
            index += 1
            continue
        if not single_quote and (char == "`" or text.startswith("$(", index)):
            return True
        index += 1
    return False


def has_unquoted_shell_control(text: str) -> bool:
    single_quote = False
    double_quote = False
    escaped = False
    index = 0
    while index < len(text):
        char = text[index]
        if escaped:
            escaped = False
            index += 1
            continue
        if char == "\\":
            escaped = True
            index += 1
            continue
        if char == "'" and not double_quote:
            single_quote = not single_quote
            index += 1
            continue
        if char == '"' and not single_quote:
            double_quote = not double_quote
            index += 1
            continue
        if not single_quote and not double_quote:
            if text.startswith("&&", index) or text.startswith("||", index):
                return True
            if char in {"|", ";", "&", "\n"}:
                return True
        index += 1
    return False


def has_unquoted_redirection(text: str) -> bool:
    single_quote = False
    double_quote = False
    escaped = False
    for char in text:
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if char == "'" and not double_quote:
            single_quote = not single_quote
            continue
        if char == '"' and not single_quote:
            double_quote = not double_quote
            continue
        if not single_quote and not double_quote and char in {"<", ">"}:
            return True
    return False


def is_only_root_switch_back(text: str, default_branch: str, root: str | None = None) -> bool:
    if has_shell_control_or_redirection(text):
        return False
    segments = shell_token_segments(text)
    return bool(segments) and all(is_switch_back_segment(segment, default_branch, root) for segment in segments)


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
        return len(tokens) > 1 and (tokens[1] in {"fmt", "fix"} or (tokens[1] == "clippy" and "--fix" in tokens))
    if command in {"npm", "pnpm", "yarn", "bun"}:
        return len(tokens) > 1 and tokens[1] in {"add", "install", "remove", "update"}
    if command in {"perl", "sed"}:
        return any(token in {"-i", "--in-place"} or token.startswith("-i") for token in tokens[1:])
    return False
