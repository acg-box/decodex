#!/usr/bin/env python3
"""Shared contract helpers for reset-status tooling."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

RESET_STATUS_SCHEMA = "reset_status/v1"
RESET_STATUS_VALUES = {"reset", "not_reset", "unknown"}


@dataclass
class ValidationResult:
    ok: bool
    errors: list[str]


def load_json(path: str | Path) -> Any:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def dump_json(path: str | Path, payload: Any) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def iso_from_millis(value: int | float | None) -> str | None:
    if value is None:
        return None
    return datetime.fromtimestamp(value / 1000, UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def validate_reset_status(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != RESET_STATUS_SCHEMA:
        errors.append(f"schema must be {RESET_STATUS_SCHEMA}")

    source_label = entry.get("source_label")
    if not isinstance(source_label, str) or not source_label:
        errors.append("source_label must be a non-empty string")

    if entry.get("source_kind") != "community":
        errors.append("source_kind must be community")

    for field in ("source_url", "source_api_url"):
        value = entry.get(field)
        if not isinstance(value, str) or not value.startswith("https://"):
            errors.append(f"{field} must be an https URL")

    status = entry.get("status")
    if status not in RESET_STATUS_VALUES:
        errors.append(f"status must be one of {sorted(RESET_STATUS_VALUES)}")

    if not isinstance(entry.get("stale"), bool):
        errors.append("stale must be a boolean")

    if not isinstance(entry.get("configured"), bool):
        errors.append("configured must be a boolean")

    upstream_state = entry.get("upstream_state")
    if upstream_state is not None and not isinstance(upstream_state, str):
        errors.append("upstream_state must be a string when present")

    updated_at = entry.get("updated_at")
    if not isinstance(updated_at, str) or not updated_at:
        errors.append("updated_at must be a non-empty string")

    auto_reset_hours = entry.get("auto_reset_hours")
    if auto_reset_hours is not None and not isinstance(auto_reset_hours, int):
        errors.append("auto_reset_hours must be an integer when present")

    reset_at = entry.get("reset_at")
    if reset_at is not None and not isinstance(reset_at, str):
        errors.append("reset_at must be a string when present")

    return ValidationResult(ok=not errors, errors=errors)
