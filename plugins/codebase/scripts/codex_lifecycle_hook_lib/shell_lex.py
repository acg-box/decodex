"""Shell tokenization helpers."""

from __future__ import annotations

import re
import shlex


def text_has_any(text: str, terms: list[str]) -> bool:
    lowered = text.lower()
    return any(term in lowered for term in terms)


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


def shell_segments(text: str) -> list[str]:
    return [" ".join(tokens) for tokens in shell_token_segments(text)]


def shell_tokens(segment: str) -> list[str]:
    segments = shell_token_segments(segment)
    return segments[0] if segments else []


def is_assignment(token: str) -> bool:
    return bool(re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", token))
