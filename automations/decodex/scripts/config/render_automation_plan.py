#!/usr/bin/env python3
"""Render full native update inputs for the exact-five automation portfolio."""

from __future__ import annotations

import argparse
import json

from portfolio import rendered_automations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true")
    parser.parse_args()
    payload = {
        "schema": "decodex/automation-portfolio-plan/1",
        "automations": rendered_automations(),
    }
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
