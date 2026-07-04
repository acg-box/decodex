"""Shell and Git command classification for lifecycle hook safeguards."""

from __future__ import annotations

import re
import shlex
from pathlib import Path
from typing import Any

from .constants import (
    ALWAYS_MUTATING_TOOL_NAMES,
    GIT_BRANCH_CREATE_OPTIONS,
    GIT_GLOBAL_OPTIONS_WITH_ARG,
    GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX,
    GIT_OPTIONS_WITH_ARG,
    MUTATING_COMMAND_TERMS,
    REDIRECTION_OPERATORS,
    SHELL_MUTATING_COMMANDS,
)
from .git_state import git_branch_ref_exists, git_output

def text_has_any(text: str, terms: list[str]) -> bool:
    lowered = text.lower()
    return any(term in lowered for term in terms)


def payload_tool_name(payload: dict[str, Any]) -> str:
    for key in ("tool_name", "toolName", "tool"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    return ""


def shell_segments(text: str) -> list[str]:
    return [" ".join(tokens) for tokens in shell_token_segments(text)]


def shell_token_segments(text: str) -> list[list[str]]:
    segments: list[list[str]] = []
    try:
        lexer = shlex.shlex(text.replace("\n", ";"), posix=True, punctuation_chars=";&|<>")
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError:
        return []
    current: list[str] = []
    for token in tokens:
        if token in {";", "&&", "||", "|", "&"}:
            if current:
                segments.append(current)
                current = []
            continue
        current.append(token)
    if current:
        segments.append(current)
    return segments


def shell_tokens(segment: str) -> list[str]:
    segments = shell_token_segments(segment)
    return segments[0] if segments else []


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


def unwrap_shell_command_tokens(tokens: list[str]) -> list[str]:
    index = 0
    while index < len(tokens) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", tokens[index]):
        index += 1
    if index < len(tokens) and tokens[index] == "env":
        index += 1
        while index < len(tokens):
            token = tokens[index]
            if token == "--":
                index += 1
                break
            if token in {"-S", "--split-string"} and index + 1 < len(tokens):
                try:
                    return shlex.split(tokens[index + 1]) + tokens[index + 2 :]
                except ValueError:
                    return []
            if token.startswith("-S") and len(token) > 2:
                try:
                    return shlex.split(token[2:]) + tokens[index + 1 :]
                except ValueError:
                    return []
            if token.startswith("--split-string="):
                try:
                    return shlex.split(token.split("=", maxsplit=1)[1]) + tokens[index + 1 :]
                except ValueError:
                    return []
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", token):
                index += 1
                continue
            if token in {"-i", "--ignore-environment", "-0", "--null"}:
                index += 1
                continue
            if token in {"-u", "--unset", "-C", "--chdir", "-P", "--path"}:
                index += 2
                continue
            if token.startswith(("--unset=", "--chdir=", "--path=")):
                index += 1
                continue
            if token.startswith("-"):
                index += 1
                continue
            break
        while index < len(tokens) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", tokens[index]):
            index += 1
    return tokens[index:]


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
