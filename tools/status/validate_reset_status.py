#!/usr/bin/env python3
"""Validate Decodex reset-status artifacts."""

from __future__ import annotations

import argparse
from pathlib import Path

from contracts import load_json, validate_reset_status


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", help="Path to a reset-status JSON file or a directory of JSON files.")
    return parser.parse_args()


def iter_paths(path: Path) -> list[Path]:
    if path.is_dir():
        return sorted(path.glob("*.json"))
    return [path]


def main() -> None:
    args = parse_args()
    target = Path(args.path)
    failures: list[str] = []

    for path in iter_paths(target):
        payload = load_json(path)
        result = validate_reset_status(payload)
        if not result.ok:
            failures.append(f"{path}:\n- " + "\n- ".join(result.errors))

    if failures:
        raise SystemExit("Reset-status validation failed:\n" + "\n".join(failures))

    print("OK")


if __name__ == "__main__":
    main()
