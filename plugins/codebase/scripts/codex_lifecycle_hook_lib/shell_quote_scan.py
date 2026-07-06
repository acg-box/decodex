"""Quote-aware shell substitution scanner."""

from __future__ import annotations

from .shell_redirection_scan import has_unquoted_redirection


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
