"""Shell control-operator classification."""

from __future__ import annotations

from .shell_quote_scan import has_shell_substitution, has_unquoted_redirection


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


def has_shell_control_or_redirection(text: str) -> bool:
    return has_unquoted_shell_control(text) or has_unquoted_redirection(text) or has_shell_substitution(text)
