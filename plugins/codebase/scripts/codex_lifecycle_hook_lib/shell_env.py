"""Shell environment wrapper unwrapping facade."""

from __future__ import annotations

from .shell_env_parse import unwrap_env_tokens
from .shell_lex import is_assignment


def unwrap_shell_command_tokens(tokens: list[str]) -> list[str]:
    index = 0
    while index < len(tokens) and is_assignment(tokens[index]):
        index += 1
    if index < len(tokens) and tokens[index] == "env":
        return unwrap_env_tokens(tokens, index)
    return tokens[index:]
