"""Shell env command unwrapping implementation."""

from __future__ import annotations

import shlex

from .shell_lex import is_assignment

ENV_VALUE_OPTIONS = {"-u", "--unset", "-C", "--chdir", "-P", "--path"}
ENV_FLAG_OPTIONS = {"-i", "--ignore-environment", "-0", "--null"}
ENV_VALUE_PREFIXES = ("--unset=", "--chdir=", "--path=")


def split_env_string(value: str, suffix: list[str]) -> list[str]:
    try:
        return shlex.split(value) + suffix
    except ValueError:
        return []


def unwrap_env_tokens(tokens: list[str], index: int) -> list[str]:
    index += 1
    while index < len(tokens):
        token = tokens[index]
        if token == "--":
            index += 1
            break
        if token in {"-S", "--split-string"} and index + 1 < len(tokens):
            return split_env_string(tokens[index + 1], tokens[index + 2 :])
        if token.startswith("-S") and len(token) > 2:
            return split_env_string(token[2:], tokens[index + 1 :])
        if token.startswith("--split-string="):
            return split_env_string(token.split("=", maxsplit=1)[1], tokens[index + 1 :])
        if is_assignment(token) or token in ENV_FLAG_OPTIONS:
            index += 1
            continue
        if token in ENV_VALUE_OPTIONS:
            index += 2
            continue
        if token.startswith(ENV_VALUE_PREFIXES):
            index += 1
            continue
        if token.startswith("-"):
            index += 1
            continue
        break
    return tokens[index:]
