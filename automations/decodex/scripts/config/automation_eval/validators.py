"""Validation rules for automation manifests, prompts, and active configs."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import stat
import subprocess
from typing import Any

from automation_model_policy import (
    DEFAULT_REASONING_EFFORT,
    expected_model,
    expected_reasoning_effort,
)
from automation_eval.constants import (
    REQUIRED_FORBIDDEN_PROMPT_FRAGMENTS,
    REQUIRED_PREFLIGHT_FRAGMENTS,
    REPO_ROOT,
    VALID_SOURCE_ROOTS,
)
from automation_eval.io import expected_cwd
from automation_eval.model import AutomationResult


_PUBLISHER_PROBE_MAX_BYTES = 64 * 1024
_RUNTIME_MEMORY_MAX_BYTES = 4 * 1024
_MEMORY_DISABLED_AUTOMATION_IDS = {
    "codex-upstream-maintainer",
    "codex-upstream-reviewer",
}
_PROBE_REPORT_KEYS = {
    "status",
    "ready",
    "xurl_version",
    "xurl_app",
    "account_label",
    "authorization_contract",
    "pricing_policy",
}
_AUTHORIZATION_CONTRACT_KEYS = {
    "policy_id",
    "status",
    "target_account",
    "xurl_app",
    "required_operator_authorized_scopes",
    "xurl_version",
    "xurl_binary_sha256",
    "sealed_at",
}


def validate_runtime_memory(
    automation_id: str,
    automation_root: Path,
    result: AutomationResult,
) -> None:
    """Validate one optional bounded runtime-memory file without following links."""

    memory_path = automation_root / "memory.md"
    if not os.path.lexists(memory_path):
        return
    if automation_id in _MEMORY_DISABLED_AUTOMATION_IDS:
        result.fail("runtime memory must be absent for this automation")
        return
    descriptor: int | None = None
    try:
        descriptor = os.open(memory_path, os.O_RDONLY | os.O_NOFOLLOW)
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or not 1 <= metadata.st_size <= _RUNTIME_MEMORY_MAX_BYTES
        ):
            result.fail("runtime memory file is not private and bounded")
            return
        payload = os.read(descriptor, _RUNTIME_MEMORY_MAX_BYTES + 1)
        if len(payload) != metadata.st_size or os.read(descriptor, 1):
            result.fail("runtime memory file changed during read")
            return
    except (OSError, ValueError):
        result.fail("runtime memory file is not safely readable")
        return
    finally:
        if descriptor is not None:
            os.close(descriptor)
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError:
        result.fail("runtime memory file is not valid UTF-8")
        return
    lines = text.splitlines()
    if (
        not 2 <= len(lines) <= 32
        or any(not line or len(line) > 512 for line in lines)
        or any(
            fragment in text
            for fragment in (
                "/Users/",
                "/home/",
                "access_token",
                "refresh_token",
                "auth.json",
            )
        )
    ):
        result.fail("runtime memory content is not bounded and private")
        return
    if automation_id == "codex-upstream-health" and (
        not lines[0].startswith("# ")
        or lines[1] != "Schema: decodex/automation-memory/1"
    ):
        result.fail("runtime health memory grammar is invalid")


_PRICING_POLICY_KEYS = {
    "policy_id",
    "official_source",
    "reviewed_at",
    "effective_at",
    "expires_at",
    "status",
    "user_read_cost_microusd",
    "url_free_content_create_cost_microusd",
    "post_read_cost_ceiling_microusd",
    "monthly_reservation_cap_microusd",
}
_AUTOMATION_ID = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")


def _bounded_probe_text(value: Any) -> bool:
    return (
        isinstance(value, str)
        and 1 <= len(value) <= 256
        and "\x00" not in value
        and "\n" not in value
        and "\r" not in value
    )


def _valid_xurl_version(value: Any) -> bool:
    return value == "1.3.1"


def _valid_publisher_probe_report(value: Any) -> bool:
    if not isinstance(value, dict) or set(value) != _PROBE_REPORT_KEYS:
        return False
    auth = value.get("authorization_contract")
    pricing = value.get("pricing_policy")
    return bool(
        value.get("status") == "ready"
        and value.get("ready") is True
        and _valid_xurl_version(value.get("xurl_version"))
        and value.get("xurl_app") == "default"
        and value.get("account_label") == "decodexspace"
        and isinstance(auth, dict)
        and set(auth) == _AUTHORIZATION_CONTRACT_KEYS
        and auth.get("policy_id") == "xurl-oauth-least-privilege/3"
        and auth.get("status") == "current"
        and auth.get("target_account") == "decodexspace"
        and auth.get("xurl_app") == "default"
        and auth.get("required_operator_authorized_scopes")
        == ["tweet.read", "users.read", "tweet.write", "offline.access"]
        and auth.get("xurl_version") == "1.3.1"
        and auth.get("xurl_binary_sha256")
        == "7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e"
        and _bounded_probe_text(auth.get("sealed_at"))
        and isinstance(pricing, dict)
        and set(pricing) == _PRICING_POLICY_KEYS
        and pricing.get("policy_id") == "x-api-pay-per-usage/2026-07-27"
        and pricing.get("official_source")
        == "https://docs.x.com/x-api/getting-started/pricing.md"
        and pricing.get("status") == "current"
        and pricing.get("user_read_cost_microusd") == 10_000
        and pricing.get("url_free_content_create_cost_microusd") == 15_000
        and pricing.get("post_read_cost_ceiling_microusd") == 5_000
        and pricing.get("monthly_reservation_cap_microusd") == 1_250_000
        and all(
            _bounded_probe_text(pricing.get(field))
            for field in ("reviewed_at", "effective_at", "expires_at")
        )
    )


def validate_xurl_runtime(
    result: AutomationResult,
    *,
    repo_only: bool,
    publisher: Path | None = None,
) -> None:
    if repo_only:
        return
    executable = (
        REPO_ROOT / "target/debug/decodex-publisher"
        if publisher is None
        else publisher
    )
    try:
        metadata = executable.lstat()
        if (
            executable.is_symlink()
            or not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
            or metadata.st_mode & 0o022
            or not metadata.st_mode & stat.S_IXUSR
        ):
            result.fail("Publisher probe executable is not trusted")
            return
    except OSError:
        result.fail("Publisher probe executable is unavailable")
        return
    try:
        probe = subprocess.run(
            [str(executable), "social", "probe-xurl"],
            check=False,
            capture_output=True,
            cwd=REPO_ROOT,
            timeout=60,
        )
    except (OSError, subprocess.SubprocessError):
        result.fail("Publisher xurl readiness probe failed")
        return
    if (
        probe.returncode != 0
        or probe.stderr
        or not 1 <= len(probe.stdout) <= _PUBLISHER_PROBE_MAX_BYTES
    ):
        result.fail("Publisher xurl readiness probe failed")
        return
    try:
        report = json.loads(probe.stdout.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        result.fail("Publisher xurl readiness report is invalid")
        return
    if not _valid_publisher_probe_report(report):
        result.fail("Publisher xurl readiness report is not ready")


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
        if "model" in defaults:
            errors.append(
                "manifest.defaults.model is forbidden; each automation "
                "must declare its exact model"
            )
        if defaults.get("reasoning_effort") != DEFAULT_REASONING_EFFORT:
            errors.append(
                "manifest.defaults.reasoning_effort must be "
                f"{DEFAULT_REASONING_EFFORT}"
            )
    automations = manifest.get("automations")
    valid_active_ids: list[str] = []
    if not isinstance(automations, list) or not automations:
        errors.append("manifest.automations must be a non-empty array")
        active_ids: list[str] = []
    else:
        active_ids = [
            automation.get("id")
            for automation in automations
            if isinstance(automation, dict)
        ]
        valid_active_ids = [
            value
            for value in active_ids
            if isinstance(value, str) and _AUTOMATION_ID.fullmatch(value)
        ]
        if len(active_ids) != len(automations) or any(
            not isinstance(value, str) or not _AUTOMATION_ID.fullmatch(value)
            for value in active_ids
        ):
            errors.append("manifest.automations must contain valid ids")
        elif len(active_ids) != len(set(active_ids)):
            errors.append("manifest automation ids must be unique")
        for automation in automations:
            if not isinstance(automation, dict):
                continue
            automation_id = automation.get("id")
            if not isinstance(automation_id, str):
                continue
            try:
                expected = expected_model(automation_id)
            except ValueError as error:
                errors.append(str(error))
                continue
            if automation.get("model") != expected:
                errors.append(
                    f"manifest automation {automation_id} model must be "
                    f"{expected}"
                )
            expected_effort = expected_reasoning_effort(automation_id)
            actual_effort = automation.get(
                "reasoning_effort",
                defaults.get("reasoning_effort")
                if isinstance(defaults, dict)
                else None,
            )
            if actual_effort != expected_effort:
                errors.append(
                    f"manifest automation {automation_id} "
                    "reasoning_effort must be "
                    f"{expected_effort}"
                )
    retired = manifest.get("retired_automation_ids", [])
    if not isinstance(retired, list) or any(
        not isinstance(value, str) or not _AUTOMATION_ID.fullmatch(value)
        for value in retired
    ):
        errors.append("manifest.retired_automation_ids must be an array of valid ids")
    elif len(retired) != len(set(retired)):
        errors.append("manifest retired automation ids must be unique")
    elif set(retired) & set(valid_active_ids):
        errors.append("manifest active and retired automation ids must not overlap")
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
        present = (
            required in prompt.casefold()
            if required == "fail closed"
            else required in prompt
        )
        if not present:
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
        "model": automation.get("model"),
        "reasoning_effort": automation.get(
            "reasoning_effort",
            defaults.get("reasoning_effort"),
        ),
        "execution_environment": defaults.get("execution_environment"),
    }
    for field_name, expected in field_map.items():
        actual = active_config.get(field_name)
        if actual != expected:
            result.fail(f"active {field_name} mismatch: expected {expected!r}, got {actual!r}")

    destination = active_config.get("destination", defaults.get("execution_environment"))
    target = active_config.get("target")
    if (
        destination != "local"
        or not isinstance(target, dict)
        or set(target) != {"type", "project_id"}
        or target.get("type") != "project"
        or not isinstance(target.get("project_id"), str)
        or not target["project_id"]
    ):
        result.fail("active destination must be a local project")

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

    created_at = active_config.get("created_at")
    updated_at = active_config.get("updated_at")
    if not isinstance(created_at, int) or isinstance(created_at, bool) or created_at <= 0:
        result.fail("active created_at must be a positive integer")
    if not isinstance(updated_at, int) or isinstance(updated_at, bool) or updated_at <= 0:
        result.fail("active updated_at must be a positive integer")
    if (
        isinstance(created_at, int)
        and not isinstance(created_at, bool)
        and isinstance(updated_at, int)
        and not isinstance(updated_at, bool)
        and updated_at < created_at
    ):
        result.fail("active updated_at must not be earlier than created_at")
