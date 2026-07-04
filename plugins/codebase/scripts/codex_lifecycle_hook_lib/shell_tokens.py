"""Shell tokenization helpers for lifecycle hook safeguards."""

from __future__ import annotations

import re
import shlex


def text_has_any(text: str, terms: list[str]) -> bool:
    lowered = text.lower()
    return any(term in lowered for term in terms)


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
