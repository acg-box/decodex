#!/usr/bin/env python3
"""Fetch the upstream community reset tracker and build a Decodex reset-status artifact."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime
import urllib.error
import urllib.request
from typing import Any

from contracts import dump_json, iso_from_millis, validate_reset_status


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--url",
        default="https://hascodexratelimitreset.today/api/status",
        help="Upstream status API URL.",
    )
    parser.add_argument(
        "--source-url",
        default="https://hascodexratelimitreset.today/",
        help="Human-facing source site URL.",
    )
    parser.add_argument(
        "--source-label",
        default="Community tracker",
        help="Human-facing source label.",
    )
    parser.add_argument("--out", required=True, help="Path to write the reset-status artifact.")
    return parser.parse_args()


def fetch_json(url: str) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "decodex-reset-status-builder",
        },
    )
    try:
        with urllib.request.urlopen(request) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"Reset-status request failed for {url}: {exc.code} {details}") from exc

    if not isinstance(payload, dict):
        raise SystemExit("Expected a JSON object payload from the upstream status API")
    return payload


def normalize_status(upstream_state: str | None) -> str:
    if upstream_state == "yes":
        return "reset"
    if upstream_state == "no":
        return "not_reset"
    return "unknown"


def is_stale(updated_at: str, auto_reset_hours: int | None) -> bool:
    updated = datetime.fromisoformat(updated_at.replace("Z", "+00:00"))
    age_hours = (datetime.now(UTC) - updated).total_seconds() / 3600
    stale_after = (auto_reset_hours * 2) if auto_reset_hours is not None else 48
    return age_hours > stale_after


def main() -> None:
    args = parse_args()
    payload = fetch_json(args.url)

    upstream_state = payload.get("state")
    updated_at = iso_from_millis(payload.get("updatedAt"))
    if updated_at is None:
        raise SystemExit("Upstream payload did not include a usable updatedAt timestamp")

    reset_at_raw = payload.get("resetAt")
    reset_at: str | None
    if isinstance(reset_at_raw, (int, float)):
        reset_at = iso_from_millis(reset_at_raw)
    elif isinstance(reset_at_raw, str) and reset_at_raw:
        reset_at = reset_at_raw
    else:
        reset_at = None

    entry = {
        "schema": "reset_status/v1",
        "source_label": args.source_label,
        "source_kind": "community",
        "source_url": args.source_url,
        "source_api_url": args.url,
        "status": normalize_status(upstream_state if isinstance(upstream_state, str) else None),
        "stale": is_stale(
            updated_at,
            payload.get("autoResetHours") if isinstance(payload.get("autoResetHours"), int) else None,
        ),
        "configured": bool(payload.get("configured")),
        "upstream_state": upstream_state if isinstance(upstream_state, str) else None,
        "auto_reset_hours": payload.get("autoResetHours") if isinstance(payload.get("autoResetHours"), int) else None,
        "reset_at": reset_at,
        "updated_at": updated_at,
    }

    validation = validate_reset_status(entry)
    if not validation.ok:
        raise SystemExit("Reset-status validation failed:\n- " + "\n- ".join(validation.errors))

    dump_json(args.out, entry)
    print(args.out)


if __name__ == "__main__":
    main()
