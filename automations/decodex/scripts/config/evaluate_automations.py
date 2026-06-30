#!/usr/bin/env python3
"""Evaluate live Codex app automations against repo-local authority."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[4]
MANIFEST_PATH = REPO_ROOT / "automations/decodex/automations.toml"
VALID_SOURCE_ROOTS = {"automations/decodex", "automations/radar"}
REQUIRED_FORBIDDEN_PROMPT_FRAGMENTS = [
    "/Users/x/Documents/automations",
    "Documents/automations",
    ".github/workflows",
    "site/src/content",
    ".agent/decodex",
    "~/.codex/decodex",
    ".codex/decodex",
    "accounts.jsonl",
    "auth.json",
    "runtime.sqlite3",
    "DECODEX_AGENT_HOME",
    "migrate-agent-home",
]
REQUIRED_PREFLIGHT_FRAGMENTS = [
    "pwd",
    "git status --short --branch",
    "git rev-parse HEAD",
    "fail closed",
]


@dataclass
class AutomationResult:
    automation_id: str
    status: str = "pass"
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def fail(self, message: str) -> None:
        self.status = "fail"
        self.errors.append(message)

    def warn(self, message: str) -> None:
        self.warnings.append(message)


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


def default_codex_home() -> str:
    return os.environ.get("CODEX_HOME") or str(Path.home() / ".codex")


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def expected_cwd(value: str) -> str:
    return value.replace("{repo_root}", str(REPO_ROOT))


def active_automation_path(codex_home: Path, automation_id: str) -> Path:
    return codex_home / "automations" / automation_id / "automation.toml"


def validate_manifest_shape(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if manifest.get("version") != 1:
        errors.append("manifest.version must be 1")
    defaults = manifest.get("defaults")
    if not isinstance(defaults, dict):
        errors.append("manifest.defaults must be a table")
    else:
        source_root = defaults.get("source_root")
        if source_root not in VALID_SOURCE_ROOTS:
            errors.append(
                "manifest.defaults.source_root must be one of "
                f"{', '.join(sorted(VALID_SOURCE_ROOTS))}"
            )
        expected_cache_root = f".agent/{source_root}/cache" if isinstance(source_root, str) else None
        if defaults.get("cache_root") != expected_cache_root:
            errors.append(f"manifest.defaults.cache_root must be {expected_cache_root}")
        external_prefixes = defaults.get("allowed_external_cache_prefixes", [])
        if not isinstance(external_prefixes, list):
            errors.append("manifest.defaults.allowed_external_cache_prefixes must be an array when present")
        forbidden_fragments = defaults.get("forbidden_prompt_fragments", [])
        for fragment in REQUIRED_FORBIDDEN_PROMPT_FRAGMENTS:
            if fragment not in forbidden_fragments:
                errors.append(
                    "manifest.defaults.forbidden_prompt_fragments must forbid "
                    f"{fragment}"
                )
    automations = manifest.get("automations")
    if not isinstance(automations, list) or not automations:
        errors.append("manifest.automations must be a non-empty array")
    return errors


def validate_prompt_text(
    prompt: str,
    cache_root: str,
    forbidden_fragments: list[str],
    result: AutomationResult,
) -> None:
    if not prompt.strip():
        result.fail("prompt file is empty")
    if "Codex app automation" not in prompt:
        result.fail("prompt must explicitly identify Codex app automation ownership")
    if cache_root not in prompt:
        result.fail(f"prompt must keep generated automation state under {cache_root}")
    if "GitHub Actions" not in prompt:
        result.fail("prompt must explicitly exclude GitHub Actions ownership")
    for required in REQUIRED_PREFLIGHT_FRAGMENTS:
        if required not in prompt:
            result.fail(f"prompt must include preflight requirement: {required}")
    for fragment in forbidden_fragments:
        if fragment in prompt:
            result.fail(f"prompt contains forbidden fragment: {fragment}")
    if ".agent/decodex" in prompt:
        result.fail("prompt must not reference Decodex private runtime paths under .agent/decodex")


def validate_required_paths(automation: dict[str, Any], result: AutomationResult) -> None:
    for value in automation.get("required_paths", []):
        path = REPO_ROOT / value
        if not path.exists():
            result.fail(f"required path does not exist: {value}")


def validate_prompt_required_reads(
    prompt: str,
    automation: dict[str, Any],
    result: AutomationResult,
) -> None:
    required_paths = set(automation.get("required_paths", []))
    in_required_reads = False
    for line in prompt.splitlines():
        stripped = line.strip()
        if stripped == "Required reads:":
            in_required_reads = True
            continue
        if in_required_reads and not stripped:
            break
        if not in_required_reads or not stripped.startswith("- "):
            continue
        match = re.search(r"`([^`]+)`", stripped)
        if match and match.group(1) not in required_paths:
            result.fail(f"prompt required read is missing from manifest required_paths: {match.group(1)}")


def validate_cache_prefixes(
    automation: dict[str, Any],
    cache_root: str,
    allowed_external_prefixes: list[str],
    result: AutomationResult,
) -> None:
    for value in automation.get("required_cache_prefixes", []):
        allowed = value.startswith(cache_root) or any(
            value.startswith(prefix) for prefix in allowed_external_prefixes
        )
        if not allowed:
            result.fail(f"cache prefix is outside allowed cache roots: {value}")
        if ".agent/decodex" in value:
            result.fail(f"cache prefix must not use Decodex private runtime path: {value}")


def validate_active_config(
    automation: dict[str, Any],
    defaults: dict[str, Any],
    prompt: str,
    active_config: dict[str, Any],
    result: AutomationResult,
) -> None:
    field_map = {
        "kind": defaults.get("kind"),
        "name": automation.get("name"),
        "status": defaults.get("status"),
        "rrule": automation.get("rrule"),
        "model": defaults.get("model"),
        "reasoning_effort": defaults.get("reasoning_effort"),
        "execution_environment": defaults.get("execution_environment"),
    }
    for field_name, expected in field_map.items():
        actual = active_config.get(field_name)
        if actual != expected:
            result.fail(f"active {field_name} mismatch: expected {expected!r}, got {actual!r}")

    active_cwds = active_config.get("cwds")
    expected = expected_cwd(str(defaults.get("cwd", "{repo_root}")))
    if active_cwds != [expected]:
        result.fail(f"active cwds mismatch: expected {[expected]!r}, got {active_cwds!r}")

    active_prompt = active_config.get("prompt")
    if active_prompt != prompt.strip():
        result.fail("active prompt does not match prompt_file exactly")


def evaluate_automation(
    automation: dict[str, Any],
    defaults: dict[str, Any],
    codex_home: Path,
    repo_only: bool,
) -> AutomationResult:
    automation_id = automation["id"]
    result = AutomationResult(automation_id=automation_id)
    prompt_path = REPO_ROOT / automation["prompt_file"]
    forbidden_fragments = list(defaults.get("forbidden_prompt_fragments", []))
    cache_root = str(defaults.get("cache_root", ".agent/automations/decodex/cache"))
    allowed_external_prefixes = [
        str(value) for value in defaults.get("allowed_external_cache_prefixes", [])
    ]

    if not prompt_path.exists():
        result.fail(f"prompt_file does not exist: {automation['prompt_file']}")
        prompt = ""
    else:
        prompt = read_text(prompt_path).strip()
        validate_prompt_text(prompt, cache_root, forbidden_fragments, result)

    validate_required_paths(automation, result)
    validate_prompt_required_reads(prompt, automation, result)
    validate_cache_prefixes(automation, cache_root, allowed_external_prefixes, result)

    if repo_only:
        return result

    active_path = active_automation_path(codex_home, automation_id)
    if not active_path.exists():
        result.fail(f"active automation config does not exist: {active_path}")
        return result

    active_config = load_toml(active_path)
    if active_config.get("id") != automation_id:
        result.fail(f"active id mismatch: {active_config.get('id')!r}")
    validate_active_config(automation, defaults, prompt, active_config, result)

    return result


def render_text(results: list[AutomationResult]) -> str:
    lines = []
    for result in results:
        lines.append(f"{result.automation_id}: {result.status}")
        for error in result.errors:
            lines.append(f"  error: {error}")
        for warning in result.warnings:
            lines.append(f"  warning: {warning}")
    return "\n".join(lines) + "\n"


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
