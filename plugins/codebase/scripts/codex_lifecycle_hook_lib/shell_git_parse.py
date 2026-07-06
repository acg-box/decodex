"""Git command parsing from shell tokens."""

from __future__ import annotations

from .constants import GIT_GLOBAL_OPTIONS_WITH_ARG, GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX
from .shell_tokens import unwrap_shell_command_tokens


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
