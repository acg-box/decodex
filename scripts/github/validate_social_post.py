#!/usr/bin/env python3
"""Validate Decodex social post publication-record JSON files."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import SOCIAL_POST_SCHEMA, load_json, validate_social_post  # noqa: E402


def iter_json_files(paths: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw in paths:
        path = Path(raw)
        if path.is_dir():
            files.extend(sorted(path.rglob("*.json")))
        else:
            files.append(path)
    return files


def validate_payload(path: Path) -> list[str]:
    payload = load_json(path)
    if payload.get("schema") != SOCIAL_POST_SCHEMA:
        return [f"{path}: schema must be {SOCIAL_POST_SCHEMA}"]
    validation = validate_social_post(payload)
    return [f"{path}: {error}" for error in validation.errors]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", help="Social post JSON files or directories.")
    args = parser.parse_args()

    errors: list[str] = []
    for path in iter_json_files(args.paths):
        errors.extend(validate_payload(path))

    if errors:
        raise SystemExit("Social post validation failed:\n- " + "\n- ".join(errors))

    print("OK")


if __name__ == "__main__":
    main()
