"""Payload parsing helpers for Codex hook events."""

from __future__ import annotations

import json
import sys
from typing import Any

from .payload_walk import first_payload_string_for_keys, walk_strings

def load_payload() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    try:
        value = json.loads(raw)
    except json.JSONDecodeError:
        return {"raw": raw[:1000]}
    return value if isinstance(value, dict) else {"value": value}


def payload_command_text(payload: dict[str, Any]) -> str:
    command = first_payload_string_for_keys(payload, {"cmd", "command", "script"})
    if command is not None:
        return command
    return " ".join(walk_strings(payload))
