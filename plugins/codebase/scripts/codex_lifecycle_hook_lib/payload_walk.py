"""Payload tree traversal helpers."""

from __future__ import annotations

from typing import Any


def walk_strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        strings: list[str] = []
        for item in value.values():
            strings.extend(walk_strings(item))
        return strings
    if isinstance(value, list):
        strings = []
        for item in value:
            strings.extend(walk_strings(item))
        return strings
    return []


def first_payload_string_for_keys(value: Any, keys: set[str]) -> str | None:
    if isinstance(value, dict):
        for key, item in value.items():
            if key in keys and isinstance(item, str):
                return item
        for item in value.values():
            nested = first_payload_string_for_keys(item, keys)
            if nested is not None:
                return nested
    if isinstance(value, list):
        for item in value:
            nested = first_payload_string_for_keys(item, keys)
            if nested is not None:
                return nested
    return None
