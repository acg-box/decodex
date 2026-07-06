"""Quote-aware redirection scanner."""

from __future__ import annotations


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
