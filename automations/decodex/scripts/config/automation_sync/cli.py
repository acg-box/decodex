"""Command-line entrypoint for automation synchronization."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any

from automation_sync.manifest import automation_specs
from automation_sync.paths import (
    REPO_ROOT,
    default_codex_home,
    display_automation_path,
    live_automation_path,
    manifest_paths,
    resolve_codex_home,
)
from automation_sync.render import existing_created_at, normalized_snapshot, render_live_config


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        action="append",
        default=[],
        help="Automation manifest path. Defaults to Decodex and Radar manifests.",
    )
    parser.add_argument("--codex-home", default=default_codex_home(), help="Codex home path.")
    parser.add_argument("--repo-root", default=str(REPO_ROOT), help="Repo checkout path for live cwds.")
    parser.add_argument("--automation-id", action="append", help="Install only this id. Repeatable.")
    parser.add_argument("--apply", action="store_true", help="Write live automation.toml files.")
    parser.add_argument("--json", action="store_true", help="Write machine-readable output.")
    return parser.parse_args()


def selected_automation_specs(manifests: list[str], automation_ids: list[str] | None) -> list[dict[str, Any]]:
    selected = set(automation_ids or [])
    specs: list[dict[str, Any]] = []
    for manifest in manifest_paths(manifests):
        specs.extend(automation_specs(manifest))
    if selected:
        specs = [spec for spec in specs if spec["id"] in selected]
    known = {spec["id"] for spec in specs}
    missing = selected - known
    if missing:
        raise SystemExit(f"unknown automation id(s): {', '.join(sorted(missing))}")
    return specs


def sync_automations(
    specs: list[dict[str, Any]],
    codex_home: Path,
    repo_root: Path,
    apply: bool,
) -> list[dict[str, Any]]:
    now_ms = int(time.time() * 1000)
    results = []
    for spec in specs:
        path = live_automation_path(codex_home, spec["id"])
        created_at = existing_created_at(path, now_ms)
        action = "would_write"
        if apply:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(render_live_config(spec, repo_root, created_at, now_ms), encoding="utf-8")
            action = "wrote"
        results.append(
            {
                "id": spec["id"],
                "action": action,
                "path": display_automation_path(spec["id"]),
                "snapshot": normalized_snapshot(spec),
            }
        )
    return results


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).expanduser().resolve()
    codex_home = resolve_codex_home(args.codex_home, repo_root)
    specs = selected_automation_specs(args.manifest, args.automation_id)
    results = sync_automations(specs, codex_home, repo_root, args.apply)

    payload = {
        "status": "pass",
        "mode": "apply" if args.apply else "dry-run",
        "repo_root": "{repo_root}",
        "codex_home": "$CODEX_HOME",
        "results": results,
    }
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        for result in results:
            print(f"{result['action']} {result['id']} -> {result['path']}")
    return 0
