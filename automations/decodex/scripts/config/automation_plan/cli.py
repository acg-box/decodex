"""Render inputs for the native Codex automation lifecycle tool."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from automation_checkout import resolve_runtime_checkout
from automation_plan.manifest import automation_specs, retirement_ids
from automation_plan.paths import REPO_ROOT, manifest_paths


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        action="append",
        default=[],
        help="Automation manifest path. Defaults to the current upstream and content manifests.",
    )
    parser.add_argument(
        "--repo-root",
        default=None,
        help="Primary main checkout path for live cwds. Defaults to the checkout owning main.",
    )
    parser.add_argument(
        "--automation-id",
        action="append",
        help="Render only this id. Repeatable.",
    )
    parser.add_argument("--json", action="store_true", help="Write machine-readable output.")
    return parser.parse_args()


def selected_automation_specs(
    manifests: list[str], automation_ids: list[str] | None
) -> list[dict[str, Any]]:
    selected = set(automation_ids or [])
    specs: list[dict[str, Any]] = []
    for manifest in manifest_paths(manifests):
        specs.extend(automation_specs(manifest))
    all_ids = [spec["id"] for spec in specs]
    if len(all_ids) != len(set(all_ids)):
        raise SystemExit("automation ids must be unique across manifests")
    if selected:
        specs = [spec for spec in specs if spec["id"] in selected]
    known = {spec["id"] for spec in specs}
    missing = selected - known
    if missing:
        raise SystemExit(f"unknown automation id(s): {', '.join(sorted(missing))}")
    return specs


def selected_retirement_ids(manifests: list[str]) -> list[str]:
    paths = manifest_paths(manifests)
    retired: list[str] = []
    active: set[str] = set()
    for manifest in paths:
        retired.extend(retirement_ids(manifest))
        active.update(spec["id"] for spec in automation_specs(manifest))
    if len(retired) != len(set(retired)):
        raise SystemExit("retired automation ids must be unique across manifests")
    overlap = set(retired) & active
    if overlap:
        raise SystemExit(
            "active and retired automation ids overlap across manifests: "
            f"{', '.join(sorted(overlap))}"
        )
    return sorted(retired)


def native_fields(spec: dict[str, Any], repo_root: Path) -> dict[str, Any]:
    return {
        "kind": spec["kind"],
        "name": spec["name"],
        "prompt": spec["prompt"],
        "status": spec["status"],
        "rrule": spec["rrule"],
        "model": spec["model"],
        "reasoningEffort": spec["reasoning_effort"],
        "executionEnvironment": spec["execution_environment"],
        "destination": "local",
        "cwds": [str(repo_root)],
    }


def render_plan(
    specs: list[dict[str, Any]], repo_root: Path
) -> list[dict[str, Any]]:
    return [
        {
            "id": spec["id"],
            "source_manifest": spec["source_manifest"],
            "prompt_file": spec["prompt_file"],
            "native_fields": native_fields(spec, repo_root),
        }
        for spec in specs
    ]


def main() -> int:
    args = parse_args()
    try:
        repo_root = resolve_runtime_checkout(REPO_ROOT, args.repo_root)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    plan = render_plan(
        selected_automation_specs(args.manifest, args.automation_id),
        repo_root,
    )
    payload = {
        "status": "pass",
        "mode": "native-lifecycle-plan",
        "manifests": [
            str(path.relative_to(REPO_ROOT))
            if path.is_absolute() and path.is_relative_to(REPO_ROOT)
            else str(path)
            for path in manifest_paths(args.manifest)
        ],
        "definitions": plan,
        "retirements": selected_retirement_ids(args.manifest),
    }
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        for item in plan:
            print(f"native_update_required {item['id']}")
        print("Apply only with the Codex native automation lifecycle tool, then read back.")
    return 0
