"""Codex JSON output extraction for Radar analysis."""

from __future__ import annotations

import json
from typing import Any


def extract_json_payload(raw: str) -> dict[str, Any]:
    candidate = raw.strip()
    if candidate.startswith("```"):
        parts = candidate.split("```")
        if len(parts) >= 3:
            candidate = parts[1]
            if candidate.startswith("json"):
                candidate = candidate[4:]
            candidate = candidate.strip()
    try:
        payload = json.loads(candidate)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"Codex output was not valid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise SystemExit("Codex output must decode to a JSON object")
    return payload
