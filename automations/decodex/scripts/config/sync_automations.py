#!/usr/bin/env python3
"""Install Codex app automations from repo-local, privacy-safe manifests."""

from __future__ import annotations

import argparse
import json
import os
import time
import tomllib
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[4]
DEFAULT_MANIFESTS = [
    REPO_ROOT / "automations/decodex/automations.toml",
    REPO_ROOT / "automations/radar/automations.toml",
]
PRIVATE_CONFIG_FIELDS = {"created_at", "updated_at", "cwds"}


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


def default_codex_home() -> str:
    return os.environ.get("CODEX_HOME") or str(Path.home() / ".codex")


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def live_automation_path(codex_home: Path, automation_id: str) -> Path:
    return codex_home / "automations" / automation_id / "automation.toml"


def display_automation_path(automation_id: str) -> str:
    return f"$CODEX_HOME/automations/{automation_id}/automation.toml"


def manifest_paths(values: list[str]) -> list[Path]:
    if not values:
        return DEFAULT_MANIFESTS
    return [Path(value) for value in values]


def forbidden_fragments(defaults: dict[str, Any]) -> list[str]:
    return [str(value) for value in defaults.get("forbidden_prompt_fragments", [])]


def assert_public_manifest(manifest_path: Path, manifest: dict[str, Any]) -> None:
    defaults = manifest.get("defaults")
    if not isinstance(defaults, dict):
        raise ValueError(f"{manifest_path}: defaults must be a table")
    if defaults.get("cwd") != "{repo_root}":
        raise ValueError(f"{manifest_path}: defaults.cwd must stay the portable {{repo_root}} placeholder")
    forbidden = forbidden_fragments(defaults)
    for required in ["/Users/", "/home/", "accounts.jsonl", "auth.json", "runtime.sqlite3"]:
        if required not in forbidden:
            raise ValueError(f"{manifest_path}: forbidden_prompt_fragments must include {required!r}")


def assert_prompt_public(prompt_path: Path, prompt: str, forbidden: list[str]) -> None:
    for fragment in forbidden:
        if fragment and fragment in prompt:
            raise ValueError(f"{prompt_path}: prompt contains forbidden private fragment {fragment!r}")


def resolve_codex_home(value: str, repo_root: Path) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise ValueError("--codex-home must be an absolute path or a home-relative path like ~/.codex")
    resolved = path.resolve()
    if resolved == repo_root or repo_root in resolved.parents:
        raise ValueError("--codex-home must not point inside the repository checkout")
    return resolved


def automation_specs(manifest_path: Path) -> list[dict[str, Any]]:
    manifest_path = manifest_path if manifest_path.is_absolute() else REPO_ROOT / manifest_path
    manifest = load_toml(manifest_path)
    assert_public_manifest(manifest_path, manifest)
    defaults = manifest["defaults"]
    specs: list[dict[str, Any]] = []
    for automation in manifest.get("automations", []):
        prompt_file = REPO_ROOT / automation["prompt_file"]
        prompt = prompt_file.read_text(encoding="utf-8").strip()
        assert_prompt_public(prompt_file, prompt, forbidden_fragments(defaults))
        specs.append(
            {
                "id": automation["id"],
                "kind": defaults["kind"],
                "name": automation["name"],
                "prompt": prompt,
                "status": defaults["status"],
                "rrule": automation["rrule"],
                "model": defaults["model"],
                "reasoning_effort": defaults["reasoning_effort"],
                "execution_environment": defaults["execution_environment"],
                "source_manifest": str(manifest_path.relative_to(REPO_ROOT)),
                "prompt_file": automation["prompt_file"],
            }
        )
    return specs


def render_live_config(spec: dict[str, Any], repo_root: Path, created_at: int, updated_at: int) -> str:
    lines = [
        "version = 1",
        f"id = {toml_string(spec['id'])}",
        f"kind = {toml_string(spec['kind'])}",
        f"name = {toml_string(spec['name'])}",
        f"prompt = {toml_string(spec['prompt'])}",
        f"status = {toml_string(spec['status'])}",
        f"rrule = {toml_string(spec['rrule'])}",
        f"model = {toml_string(spec['model'])}",
        f"reasoning_effort = {toml_string(spec['reasoning_effort'])}",
        f"execution_environment = {toml_string(spec['execution_environment'])}",
        f"cwds = [{toml_string(str(repo_root))}]",
        f"created_at = {created_at}",
        f"updated_at = {updated_at}",
        "",
    ]
    return "\n".join(lines)


def normalized_snapshot(spec: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in spec.items() if key not in PRIVATE_CONFIG_FIELDS and key != "prompt"}


def existing_created_at(path: Path, fallback: int) -> int:
    if not path.exists():
        return fallback
    data = load_toml(path)
    value = data.get("created_at")
    return int(value) if isinstance(value, int) else fallback


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).expanduser().resolve()
    codex_home = resolve_codex_home(args.codex_home, repo_root)
    selected = set(args.automation_id or [])
    specs: list[dict[str, Any]] = []
    for manifest in manifest_paths(args.manifest):
        specs.extend(automation_specs(manifest))
    if selected:
        specs = [spec for spec in specs if spec["id"] in selected]
    known = {spec["id"] for spec in specs}
    missing = selected - known
    if missing:
        raise SystemExit(f"unknown automation id(s): {', '.join(sorted(missing))}")

    now_ms = int(time.time() * 1000)
    results = []
    for spec in specs:
        path = live_automation_path(codex_home, spec["id"])
        created_at = existing_created_at(path, now_ms)
        action = "would_write"
        if args.apply:
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


if __name__ == "__main__":
    raise SystemExit(main())
