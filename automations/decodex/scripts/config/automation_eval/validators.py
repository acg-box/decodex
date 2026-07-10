"""Validation rules for automation manifests, prompts, and active configs."""

from __future__ import annotations

import re
from typing import Any

from automation_eval.constants import (
    REQUIRED_FORBIDDEN_PROMPT_FRAGMENTS,
    REQUIRED_PREFLIGHT_FRAGMENTS,
    REPO_ROOT,
    VALID_SOURCE_ROOTS,
)
from automation_eval.io import expected_cwd
from automation_eval.model import AutomationResult


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
    if isinstance(active_cwds, list) and any(
        ".worktrees" in str(value).replace("\\", "/").split("/") for value in active_cwds
    ):
        result.fail("active cwds must not bind automations to a worktree")
    expected = expected_cwd(str(defaults.get("cwd", "{repo_root}")))
    if active_cwds != [expected]:
        result.fail(f"active cwds mismatch: expected {[expected]!r}, got {active_cwds!r}")

    active_prompt = active_config.get("prompt")
    if active_prompt != prompt.strip():
        result.fail("active prompt does not match prompt_file exactly")
