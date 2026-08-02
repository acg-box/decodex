#!/usr/bin/env python3
"""Evaluate the compact exact-five automation portfolio."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from portfolio import evaluate_runtime, load_manifest, validate_manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-only", action="store_true")
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    manifest = load_manifest()
    if args.repo_only:
        payload = {"status": "pass", "manifest_errors": validate_manifest(manifest)}
    else:
        payload = evaluate_runtime(args.codex_home)
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0 if payload["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
