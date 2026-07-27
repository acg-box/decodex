#!/usr/bin/env python3
"""Evaluate live Codex app automations against repo-local authority."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

CONFIG_ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(CONFIG_ROOT))

from automation_eval.constants import MANIFEST_PATH, REPO_ROOT
from automation_eval.evaluation import evaluate_automation
from automation_eval.io import default_codex_home, load_toml
from automation_eval.render import render_text
from automation_eval.validators import validate_manifest_shape


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", default=str(MANIFEST_PATH), help="Automation manifest path.")
    parser.add_argument("--codex-home", default=default_codex_home(), help="Codex home path.")
    parser.add_argument("--automation-id", action="append", help="Evaluate only this automation id. Repeatable.")
    parser.add_argument(
        "--repo-only",
        action="store_true",
        help="Validate checked-in manifest, prompt, path, and cache boundaries without reading active Codex home configs.",
    )
    parser.add_argument("--json", action="store_true", help="Write machine-readable JSON.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest = load_toml(Path(args.manifest))
    manifest_errors = validate_manifest_shape(manifest)
    if manifest_errors:
        payload = {
            "status": "fail",
            "manifest_errors": manifest_errors,
            "results": [],
        }
        if args.json:
            print(json.dumps(payload, indent=2, sort_keys=True))
        else:
            for error in manifest_errors:
                print(f"manifest error: {error}", file=sys.stderr)
        return 1

    defaults = manifest["defaults"]
    selected_ids = set(args.automation_id or [])
    automations = [
        automation
        for automation in manifest["automations"]
        if not selected_ids or automation["id"] in selected_ids
    ]
    known_ids = {automation["id"] for automation in manifest["automations"]}
    missing_ids = selected_ids - known_ids
    if missing_ids:
        print(f"unknown automation id(s): {', '.join(sorted(missing_ids))}", file=sys.stderr)
        return 2

    results = [
        evaluate_automation(automation, defaults, Path(args.codex_home), args.repo_only)
        for automation in automations
    ]
    status = "pass" if all(result.status == "pass" for result in results) else "fail"
    payload = {
        "status": status,
        "repo_root": str(REPO_ROOT),
        "codex_home": str(Path(args.codex_home)),
        "results": [result.__dict__ for result in results],
    }

    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(render_text(results), end="")

    return 0 if status == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
