"""Automation manifest and active config evaluation."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from automation_eval.constants import REPO_ROOT
from automation_eval.io import active_automation_path, load_toml, read_text
from automation_eval.model import AutomationResult
from automation_eval.validators import (
    validate_active_config,
    validate_cache_prefixes,
    validate_prompt_required_reads,
    validate_prompt_text,
    validate_required_paths,
    validate_xurl_runtime,
)


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
    if automation_id == "decodex-xurl-publisher":
        validate_xurl_runtime(result, repo_only=repo_only)

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
