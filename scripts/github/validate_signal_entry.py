#!/usr/bin/env python3
"""Validate one or more rendered Decodex signal-entry JSON files."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import load_json, validate_signal  # noqa: E402


def iter_json_files(paths: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw in paths:
        path = Path(raw)
        if path.is_dir():
            files.extend(sorted(path.glob("*.json")))
        else:
            files.append(path)
    return files


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", help="Signal JSON files or directories.")
    args = parser.parse_args()

    errors: list[str] = []
    seen_slugs: dict[str, Path] = {}
    for path in iter_json_files(args.paths):
        payload = load_json(path)
        result = validate_signal(payload)
        if not result.ok:
            for error in result.errors:
                errors.append(f"{path}: {error}")
        slug = payload.get("slug")
        if isinstance(slug, str):
            if slug in seen_slugs:
                errors.append(f"{path}: duplicate slug {slug!r} also used by {seen_slugs[slug]}")
            else:
                seen_slugs[slug] = path

    if errors:
        raise SystemExit("Signal validation failed:\n- " + "\n- ".join(errors))

    print("OK")


if __name__ == "__main__":
    main()
