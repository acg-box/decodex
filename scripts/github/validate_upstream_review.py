#!/usr/bin/env python3
"""Validate upstream review queue and review JSON files."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import (  # noqa: E402
    UPSTREAM_REVIEW_QUEUE_SCHEMA,
    UPSTREAM_REVIEW_SCHEMA,
    load_json,
    validate_upstream_review,
    validate_upstream_review_queue,
)


def iter_json_files(paths: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw in paths:
        path = Path(raw)
        if path.is_dir():
            files.extend(sorted(path.glob("*.json")))
        else:
            files.append(path)
    return files


def validate_payload(path: Path) -> list[str]:
    payload = load_json(path)
    schema = payload.get("schema")
    if schema == UPSTREAM_REVIEW_QUEUE_SCHEMA:
        validation = validate_upstream_review_queue(payload)
    elif schema == UPSTREAM_REVIEW_SCHEMA:
        validation = validate_upstream_review(payload)
    else:
        return [f"{path}: schema must be {UPSTREAM_REVIEW_QUEUE_SCHEMA} or {UPSTREAM_REVIEW_SCHEMA}"]
    return [f"{path}: {error}" for error in validation.errors]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", help="Review queue JSON files or directories.")
    args = parser.parse_args()

    errors: list[str] = []
    for path in iter_json_files(args.paths):
        errors.extend(validate_payload(path))

    if errors:
        raise SystemExit("Upstream review validation failed:\n- " + "\n- ".join(errors))

    print("OK")


if __name__ == "__main__":
    main()
