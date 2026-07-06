"""Package-manager mutation classification."""

from __future__ import annotations


def cargo_command_is_mutating(tokens: list[str]) -> bool:
    return len(tokens) > 1 and (
        tokens[1] in {"fmt", "fix"}
        or (tokens[1] == "clippy" and "--fix" in tokens)
    )


def package_manager_command_is_mutating(tokens: list[str]) -> bool:
    return len(tokens) > 1 and tokens[1] in {"add", "install", "remove", "update"}


def inplace_editor_command_is_mutating(tokens: list[str]) -> bool:
    return any(token in {"-i", "--in-place"} or token.startswith("-i") for token in tokens[1:])
